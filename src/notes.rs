//! Versioned note schemas + `keel notes write <test|doc|cr>`.
//! Notes live in refs/notes/<kind> (same naming as Trawler), JSON, attached to the tip commit.
use crate::{git, Config};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// Shared identity block: what was judged (tip over base), on which branch, by whom, when.
#[derive(Serialize, Deserialize, Debug)]
pub struct Identity {
    pub tip: String,
    pub base: String,
    pub branch: Option<String>,
    pub committer: String,
    /// unix seconds
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Layer {
    Counted { count: u32, covers: String, files: Vec<String>, languages: BTreeMap<String, u32> },
    Waived { waived: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TestNote {
    pub schema: String, // "test/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub unit: Option<Layer>,
    pub integration: Option<Layer>,
    pub e2e: Option<Layer>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DocNote {
    pub schema: String, // "doc/v1"
    #[serde(flatten)]
    pub id: Identity,
    /// One entry per path of `doc_paths` (UC-DOC-3).
    pub changes: Vec<DocChange>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DocChange {
    pub path: String,
    pub tier: String,
    /// Archived T3 docs only: the T1/T2 docs that absorbed it, or "none: <why>".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distilled_to: Option<DistilledTo>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum DistilledTo {
    Paths(Vec<String>),
    DeadEnd(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrNote {
    pub schema: String, // "cr/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub verdict: String, // PASS | FAIL
    pub blockers: u32,
    pub importants: u32,
    /// git blob hash of `git diff base tip -- <code_paths>`: binds the verdict to the code reviewed.
    pub code_diff_hash: String,
    /// Epic tips only: seams of the epic walk waived on the record (edited in by hand).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub walk_waivers: Vec<WalkWaiver>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WalkWaiver {
    pub seam: String,
    pub why: String,
}

/// The human sign-off (W4, `keel mr confirm`). Check 8 rebuilds the signed payload from the repo;
/// only `signer`, `report` and the signed `report_hash` are taken from here.
#[derive(Serialize, Deserialize, Debug)]
pub struct MrNote {
    pub schema: String, // "mr/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub level: String, // wu | epic
    pub signer: String,
    /// Informational: authority is re-derived from .mr-allowed at check time.
    pub tier: String,
    /// The attested result report; absent when `[mr] report` asked for none and none was attested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// git blob id of the attested report's bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_hash: Option<String>,
    /// git blob hash of the full `git diff base tip`.
    pub change_hash: String,
    /// Armored SSH SIGNATURE (namespace keel-mr), key-backed signers only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

/// The merge gate's receipt, written on green.
#[derive(Serialize, Deserialize, Debug)]
pub struct DodNote {
    pub schema: String, // "dod/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub level: String,
    pub checks: Vec<String>,
    /// git blob hash of the full `git diff base tip`.
    pub diff_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Judgment {
    unit: Option<JLayer>,
    integration: Option<JLayer>,
    e2e: Option<JLayer>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JLayer {
    #[serde(default)]
    files: Vec<String>,
    covers: Option<String>,
    waived: Option<String>,
}

fn refuse<T>(what: String, fix: &str) -> Result<T, String> {
    Err(format!("keel notes: refused - {what}\n  fix -> {fix}"))
}

struct Range {
    id: Identity,
    names: Vec<String>,
}

/// The ref a range is measured against, shared by the writer and the gate so both
/// judge one range: `--target R`, else a `<wu_prefix><e>/..` tip's `<epic_prefix><e>`, else main.
pub(crate) fn target(cfg: &Config, tip_ref: &str, args: &[&str]) -> String {
    if let Some(t) = args.windows(2).find(|w| w[0] == "--target") {
        return t[1].to_string();
    }
    let b = &cfg.branches;
    match tip_ref.strip_prefix(b.wu_prefix.as_str()).and_then(|r| r.split_once('/')) {
        Some((epic, _)) if !epic.is_empty() => format!("{}{epic}", b.epic_prefix),
        _ => cfg.branches.main.clone(),
    }
}

/// Paths from a `-z` listing: raw bytes, never C-quoted (a plain listing quotes non-ASCII paths).
pub(crate) fn zpaths(out: &str) -> Vec<String> {
    out.split('\0').map(|s| s.trim_matches('\n')).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Paths changed base..tip within `pathspecs` (none = all). No rename pairing: moving a file out of
/// a path is a change to that path.
pub(crate) fn names(repo: &Path, base: &str, tip: &str, pathspecs: &[String]) -> Result<Vec<String>, String> {
    let mut a = vec!["diff", "--name-only", "--no-renames", "-z", base, tip, "--"];
    a.extend(pathspecs.iter().map(String::as_str));
    Ok(zpaths(&git(repo, &a, &[])?))
}

pub(crate) fn identity(repo: &Path, tip: String, base: String, branch: Option<String>) -> Result<Identity, String> {
    // "Name <email> 1700000000 +0100"
    let ident = git(repo, &["var", "GIT_COMMITTER_IDENT"], &[])?;
    let (who, ts) = ident.split_once("> ").ok_or("keel: cannot parse committer identity")?;
    let timestamp = ts
        .split(' ')
        .next()
        .and_then(|t| t.parse().ok())
        .ok_or("keel: bad committer timestamp")?;
    Ok(Identity { tip, base, branch, committer: format!("{who}>"), timestamp })
}

fn range(repo: &Path, cfg: &Config, args: &[&str]) -> Result<Range, String> {
    let branch = git(repo, &["symbolic-ref", "--short", "-q", "HEAD"], &[]).ok();
    let target = target(cfg, branch.as_deref().unwrap_or("HEAD"), args);
    let tip = git(repo, &["rev-parse", "HEAD"], &[])?;
    let base = git(repo, &["merge-base", &target, "HEAD"], &[])?;
    if base == tip {
        return refuse(
            format!("HEAD is already on '{target}'; the range since the merge-base is empty"),
            "write notes from the work branch, with commits not yet merged (or pass --target <ref>)",
        );
    }
    let names = names(repo, &base, &tip, &[])?;
    Ok(Range { id: identity(repo, tip, base, branch)?, names })
}

/// UC-GATE-5 docs/infra-only: the range changes nothing under `code_paths` - git pathspecs, the very
/// diff cr binds (empty = every path is code) - and, when `infra_paths` is set, every changed path
/// matches one of its globs.
pub(crate) fn exempt(repo: &Path, cfg: &Config, base: &str, tip: &str, names: &[String]) -> Result<bool, String> {
    Ok(self::names(repo, base, tip, &cfg.code_paths)?.is_empty()
        && cfg.infra_paths.as_ref().is_none_or(|g| names.iter().all(|n| g.iter().any(|p| glob(p, n)))))
}

/// `*` = within one segment, `**` = any depth, `?` = one char.
fn glob(pattern: &str, path: &str) -> bool {
    let mut re = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                re.push_str(".*");
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    regex::Regex::new(&re).is_ok_and(|r| r.is_match(path))
}

/// T0-T3 by `docs.tiers` globs (default: Trawler's layout under docs.root).
/// T3 is tried first: a broad T1 glob must never hide a working doc from the archive check.
pub(crate) fn tier_of(cfg: &Config, path: &str) -> Option<&'static str> {
    let r = format!("{}/", cfg.docs.root.trim_end_matches('/'));
    let default;
    let t = match &cfg.docs.tiers {
        Some(t) => t,
        None => {
            default = crate::Tiers {
                t0: vec!["AGENTS.md".into(), "CLAUDE.md".into()],
                t1: vec![format!("{r}*")],
                t2: ["use-cases", "decisions", "backlog"].map(|d| format!("{r}{d}/**")).to_vec(),
                t3: vec![format!("{r}work/**"), format!("{r}.archive/**")],
            };
            &default
        }
    };
    let tier = [("T3", &t.t3), ("T2", &t.t2), ("T1", &t.t1), ("T0", &t.t0)]
        .into_iter()
        .find(|(_, globs)| globs.iter().any(|g| glob(g, path)))
        .map(|(name, _)| name);
    tier
}

pub(crate) fn at_tip(repo: &Path, tip: &str, path: &str) -> bool {
    git(repo, &["cat-file", "-e", &format!("{tip}:{path}")], &[]).is_ok()
}

fn archived(repo: &Path, cfg: &Config, tip: &str, path: &str) -> bool {
    tier_of(cfg, path) == Some("T3") && !at_tip(repo, tip, path)
}

/// The docs doc/v1 must account for: changed paths under docs.root or in T0, plus T3
/// docs deleted anywhere in the range - one created and archived inside it nets out
/// of the endpoint diff, but its lineage is still owed. Shared by the writer and check 6.
pub(crate) fn doc_paths(repo: &Path, cfg: &Config, base: &str, tip: &str, names: &[String]) -> Result<Vec<String>, String> {
    let mut v: Vec<String> =
        names.iter().filter(|n| n.starts_with(&cfg.docs.root) || tier_of(cfg, n) == Some("T0")).cloned().collect();
    let gone = git(repo, &["log", "--no-renames", "-z", "--diff-filter=D", "--name-only", "--format=", &format!("{base}..{tip}")], &[])?;
    for p in zpaths(&gone) {
        if !v.contains(&p) && archived(repo, cfg, tip, &p) {
            v.push(p);
        }
    }
    Ok(v)
}

pub(crate) fn archives_t3(repo: &Path, cfg: &Config, tip: &str, docs: &[String]) -> bool {
    docs.iter().any(|p| archived(repo, cfg, tip, p))
}

const REWRITE: &str = "keel notes write doc   (on the tip)";

/// Check 6 and the doc writer's refusals: (what, fix) per problem; empty = green.
pub(crate) fn doc_problems(repo: &Path, cfg: &Config, tip: &str, docs: &[String], changes: &[DocChange]) -> Vec<(String, String)> {
    let mut out = vec![];
    for p in docs.iter().filter(|p| !changes.iter().any(|c| &c.path == *p)) {
        out.push((format!("{p} changed in the range but is not in doc/v1"), REWRITE.into()));
    }
    for c in changes {
        let p = &c.path;
        if !docs.contains(p) {
            out.push((format!("doc/v1 lists {p} but the range does not touch it"), REWRITE.into()));
            continue;
        }
        let Some(tier) = tier_of(cfg, p) else {
            out.push((format!("{p} matches no docs.tiers pattern"), "add a glob for it under [docs.tiers] in .keel/config.toml, or move it".into()));
            continue;
        };
        if c.tier != tier {
            out.push((format!("{p} is tier {tier} by docs.tiers, doc/v1 says {}", c.tier), REWRITE.into()));
        }
        if tier != "T3" || at_tip(repo, tip, p) {
            continue;
        }
        let why = match &c.distilled_to {
            Some(DistilledTo::DeadEnd(s)) if s.strip_prefix("none:").is_some_and(|w| !w.trim().is_empty()) => None,
            Some(DistilledTo::Paths(ts)) if !ts.is_empty() => ts
                .iter()
                .find(|t| !matches!(tier_of(cfg, t), Some("T1" | "T2")) || !at_tip(repo, tip, t))
                .map(|t| format!("with distilled_to {t}, which is not a T1/T2 doc tracked at the tip")),
            _ => Some("without distillation lineage (distilled_to)".into()),
        };
        if let Some(why) = why {
            out.push((
                format!("{p} is archived {why}"),
                format!("distill it into a T1/T2 doc, then keel notes write doc --distill {p}=<doc>[,<doc>]   (or =none: <why nothing survives>)"),
            ));
        }
    }
    out
}

/// One function for the writer (W2) and check 5 (W3), so the two can never disagree.
pub(crate) fn code_diff_hash(repo: &Path, cfg: &Config, base: &str, tip: &str) -> Result<String, String> {
    diff_hash(repo, base, tip, &cfg.code_paths)
}

/// git blob hash of `git diff base tip -- <pathspecs>` (none = the full diff), independent of the
/// local diff config so every machine hashes the same bytes (cr, dod and mr bindings).
pub(crate) fn diff_hash(repo: &Path, base: &str, tip: &str, pathspecs: &[String]) -> Result<String, String> {
    let mut a = vec!["diff", "--no-color", "--no-ext-diff", "--no-textconv", base, tip, "--"];
    a.extend(pathspecs.iter().map(String::as_str));
    hash_blob(repo, &git(repo, &a, &[])?)
}

fn count_tests(adapter: &str, body: &str, pattern: Option<&str>) -> Result<u32, String> {
    let starts = |forms: &[&str]| {
        body.lines().filter(|l| forms.iter().any(|f| l.trim_start().starts_with(f))).count() as u32
    };
    Ok(match adapter {
        // ponytail: line-prefix rules; a declaration written another way is not counted (use the regex adapter)
        "rust" => starts(&["#[test]", "#[tokio::test"]),
        "ts" => starts(&["it(", "test(", "it.each(", "test.each("]),
        "python" => starts(&["def test_", "async def test_"]),
        "go" => starts(&["func Test"]),
        "regex" => {
            let p = pattern.ok_or("keel: tests.adapter = \"regex\" needs tests.pattern")?;
            let re = regex::Regex::new(p).map_err(|e| format!("keel: bad tests.pattern: {e}"))?;
            re.find_iter(body).count() as u32
        }
        other => return Err(format!("keel: unknown tests.adapter '{other}' (rust|ts|python|go|regex)")),
    })
}

fn adapter_for(cfg: &Config, file: &str) -> Result<String, String> {
    if let Some(a) = &cfg.tests.adapter {
        return Ok(a.clone());
    }
    match file.rsplit('.').next().unwrap_or("") {
        "rs" => Ok("rust".into()),
        "ts" | "tsx" | "js" | "jsx" => Ok("ts".into()),
        "py" => Ok("python".into()),
        "go" => Ok("go".into()),
        _ if cfg.tests.pattern.is_some() => Ok("regex".into()),
        _ => refuse(
            format!("no test counter for {file}; counting it as zero would be a false note"),
            "set tests.pattern (and optionally tests.adapter) in .keel/config.toml",
        ),
    }
}

fn layer(repo: &Path, cfg: &Config, r: &Range, name: &str, j: &JLayer) -> Result<Layer, String> {
    let waived = j.waived.as_deref().map(str::trim).unwrap_or("");
    if !waived.is_empty() {
        if !j.files.is_empty() {
            return refuse(format!("{name} has both files and a waiver"), "give files or waived, not both");
        }
        return Ok(Layer::Waived { waived: waived.into() });
    }
    let covers = j.covers.as_deref().map(str::trim).unwrap_or("");
    if j.files.is_empty() || covers.is_empty() {
        return refuse(
            format!("{name} needs `files` and a `covers` sentence, or a `waived` reason"),
            "list the test files that hold the tests and say what they establish",
        );
    }
    let (mut count, mut languages) = (0, BTreeMap::new());
    for f in &j.files {
        if !r.names.contains(f) {
            return refuse(
                format!("{name} claims {f}, but the range does not touch it"),
                "name only test files changed in this range, or waive the layer",
            );
        }
        let body = git(repo, &["show", &format!("{}:{f}", r.id.tip)], &[])
            .or_else(|_| refuse(format!("{f} is not a file at the tip"), "name a file that exists at HEAD"))?;
        let a = adapter_for(cfg, f)?;
        let n = count_tests(&a, &body, cfg.tests.pattern.as_deref())?;
        count += n;
        *languages.entry(a).or_insert(0) += n;
    }
    if count == 0 {
        return refuse(format!("{name} claims tests but its files declare none"), "name files with tests, or waive the layer");
    }
    Ok(Layer::Counted { count, covers: covers.into(), files: j.files.clone(), languages })
}

pub(crate) fn store<T: Serialize>(repo: &Path, kind: &str, tip: &str, note: &T) -> Result<(), String> {
    let json = serde_json::to_string(note).map_err(|e| e.to_string())?;
    git(repo, &["notes", &format!("--ref={kind}"), "add", "-f", "-m", &json, tip], &[])?;
    println!("wrote refs/notes/{kind} on {}", tip.get(..8).unwrap_or(tip));
    Ok(())
}

pub(crate) fn hash_blob(repo: &Path, data: &str) -> Result<String, String> {
    let mut h = Command::new("git")
        .args(["hash-object", "--stdin"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    h.stdin.take().unwrap().write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    let out = h.wait_with_output().map_err(|e| e.to_string())?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || id.is_empty() {
        return Err("keel: git hash-object --stdin failed".into());
    }
    Ok(id)
}

/// `args` are the flags after `notes write <kind>`.
pub fn write(repo: &Path, cfg: &Config, kind: &str, args: &[&str]) -> Result<i32, String> {
    let r = range(repo, cfg, args)?;
    let tip = r.id.tip.clone();
    match kind {
        "test" => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
            let j: Judgment =
                serde_json::from_str(&s).map_err(|e| format!("keel: bad test judgment on stdin: {e}"))?;
            let one = |n: &str, l: &Option<JLayer>| l.as_ref().map(|l| layer(repo, cfg, &r, n, l)).transpose();
            let (unit, integration, e2e) =
                (one("unit", &j.unit)?, one("integration", &j.integration)?, one("e2e", &j.e2e)?);
            if unit.is_none() && integration.is_none() && e2e.is_none() {
                return refuse("the judgment claims no test layer".into(), "give unit, integration and/or e2e");
            }
            store(repo, "test", &tip, &TestNote { schema: "test/v1".into(), id: r.id, unit, integration, e2e })?;
        }
        "doc" => {
            let docs = doc_paths(repo, cfg, &r.id.base, &tip, &r.names)?;
            let mut distill = BTreeMap::new();
            for v in args.windows(2).filter(|w| w[0] == "--distill").map(|w| w[1]) {
                let (p, to) = v.split_once('=').ok_or("usage: --distill <t3-path>=<doc>[,<doc>] | <t3-path>=none: <why>")?;
                distill.insert(p, to);
            }
            let changes: Vec<DocChange> = docs
                .iter()
                .map(|p| DocChange {
                    path: p.clone(),
                    tier: tier_of(cfg, p).unwrap_or("?").into(),
                    distilled_to: archived(repo, cfg, &tip, p).then(|| distill.remove(p.as_str())).flatten().map(|to| {
                        if to.starts_with("none:") {
                            DistilledTo::DeadEnd(to.into())
                        } else {
                            DistilledTo::Paths(to.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect())
                        }
                    }),
                })
                .collect();
            if let Some(p) = distill.keys().next() {
                return refuse(format!("--distill names {p}, which is not a T3 doc archived in this range"), "--distill only the T3 docs this range git rm's");
            }
            if let Some((what, fix)) = doc_problems(repo, cfg, &tip, &docs, &changes).into_iter().next() {
                return refuse(what, &fix);
            }
            store(repo, "doc", &tip, &DocNote { schema: "doc/v1".into(), id: r.id, changes })?;
        }
        "cr" => {
            let flag = |k: &str| args.windows(2).find(|w| w[0] == k).map(|w| w[1]);
            let num = |k: &str| {
                flag(k).map_or(Ok(0), |v| v.parse::<u32>().map_err(|_| format!("keel: {k} needs a number")))
            };
            let verdict = flag("--verdict").unwrap_or("");
            let (blockers, importants) = (num("--blockers")?, num("--importants")?);
            if !matches!(verdict, "PASS" | "FAIL") {
                return Err("usage: keel notes write cr --verdict PASS|FAIL [--blockers N] [--importants N]".into());
            }
            if verdict == "PASS" && blockers + importants > 0 {
                return refuse("PASS claimed with verified blockers/importants".into(), "record FAIL, or fix the findings first");
            }
            let code_diff_hash = code_diff_hash(repo, cfg, &r.id.base, &tip)?;
            let note = CrNote {
                schema: "cr/v1".into(),
                id: r.id,
                verdict: verdict.into(),
                blockers,
                importants,
                code_diff_hash,
                walk_waivers: vec![],
            };
            store(repo, "cr", &tip, &note)?;
        }
        _ => return Err("usage: keel notes write <test|doc|cr>".into()),
    }
    Ok(0)
}
