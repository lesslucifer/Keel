//! Versioned note schemas + `keel notes write <test|doc|cr>`.
//! Notes live in refs/notes/<kind> (same naming as Trawler), JSON, attached to the tip commit.
use crate::{git, no_git_env, Config};
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

impl Identity {
    /// The JSON keys the identity block flattens into every note, from the type itself: whoever
    /// strips the identity from a note (the waiver code) never keeps its own copy of the list.
    pub(crate) fn keys() -> Vec<String> {
        let blank = Identity { tip: String::new(), base: String::new(), branch: None, committer: String::new(), timestamp: 0 };
        serde_json::to_value(blank).ok().and_then(|v| v.as_object().map(|m| m.keys().cloned().collect())).unwrap_or_default()
    }
}

/// How every fix line names the owner's waiver: the owner's own act, never an agent's.
pub(crate) const OWNER_WAIVES: &str = "the owner waives a review at sign-off (keel mr confirm ... --waive-review \"<reason>\", typed by the owner, never staged)";

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
    pub verdict: String, // PASS | FAIL | WAIVED
    /// WAIVED only: why no review passed - the owner's ruling, or the review cap's reason (UC-REV-2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<String>,
    /// On WAIVED: the findings left open.
    pub blockers: u32,
    pub importants: u32,
    /// git blob hash of `git diff base tip -- <code_paths>`: binds the verdict to the code reviewed.
    pub code_diff_hash: String,
    /// Epic tips only: seams of the epic walk waived on the record (edited in by hand).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub walk_waivers: Vec<WalkWaiver>,
    /// Every other field the review skill records (`findings`, `refuted`, `nits`, `fleet`, ...): kept
    /// as read, so a note rewritten in place (a waiver at sign-off) never drops what a round recorded.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl CrNote {
    /// The waiver's reason (possibly blank, when hand-edited) iff the verdict is WAIVED.
    pub(crate) fn waiver(&self) -> Option<&str> {
        (self.verdict == "WAIVED").then(|| self.waiver.as_deref().unwrap_or(""))
    }

    /// `sha256:<hex>` of the waiver reason iff the verdict is WAIVED: what a sign-off accepts and signs,
    /// and what check 8 rebuilds - one rule for both.
    pub(crate) fn waiver_hash(&self) -> Option<String> {
        self.waiver().map(|w| sha256(w.as_bytes()))
    }

    /// A waiver written where no review ran on the tip: its counts of 0 mean nothing.
    pub(crate) fn unreviewed(&self) -> bool {
        self.extra.get("unreviewed") == Some(&serde_json::Value::Bool(true))
    }

    /// The round's verified findings left open: the review skill's `findings` entries with no `waived`
    /// reason in words (a blank, null or non-string `waived` waives nothing, as everywhere else).
    pub(crate) fn open_findings(&self) -> Vec<&serde_json::Value> {
        let open = |f: &&serde_json::Value| !f.get("waived").and_then(|w| w.as_str()).is_some_and(|w| !w.trim().is_empty());
        self.extra.get("findings").and_then(|f| f.as_array()).into_iter().flatten().filter(open).collect()
    }
}

/// `sha256:<hex>`: the algorithm is named in the data.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// The one rule for a waiver's reason, whoever writes it: trimmed, not blank, and never the next flag
/// (`--reason --blockers 2` names no reason).
pub(crate) fn waiver_reason(raw: Option<&str>) -> Result<String, String> {
    match raw.map(str::trim) {
        Some(r) if !r.is_empty() && !r.starts_with("--") => Ok(r.into()),
        _ => Err("a waiver needs its reason, in words (a following flag is not a reason)".into()),
    }
}

/// The one cr/v1 waiver writer, for the owner's sign-off and the review tool's cap alike, given the
/// tip's cr note as stored (`existing_raw`, `None` when there is none). Never over a note it cannot
/// parse, nor over a PASS. Over a failed or waived round of this very range (same base and code hash)
/// the note is kept whole (counts, walk waivers, every field the review recorded) and only identity,
/// verdict, reason and binding change; a count the caller gives replaces that one count, an absent one
/// keeps it. A round recorded against another range is no review of this one: the waiver is written
/// `unreviewed`, that note kept whole under `replaced` as history only, and only its `walk_waivers`
/// (they name seams, which a later code change does not move) carried. With no note at all it is
/// `unreviewed` too, so its 0/0 never reads as a clean review. Err = (what, fix).
pub(crate) fn waive(existing_raw: Option<&str>, id: Identity, reason: Option<&str>, code_diff_hash: String, counts: (Option<u32>, Option<u32>)) -> Result<CrNote, (String, String)> {
    let t = id.tip.get(..10).unwrap_or(&id.tip).to_string();
    let reason = waiver_reason(reason).map_err(|w| (w, "give the reason in words".to_string()))?;
    let existing = existing_raw
        .map(serde_json::from_str::<CrNote>)
        .transpose()
        .map_err(|e| (format!("the cr note on {t} does not parse as cr/v1 ({e}); a waiver never overwrites a note it cannot read"), "repair that note in place, or run a new review on the tip, then waive again".to_string()))?;
    let unreviewed = |replaced: Option<CrNote>| {
        let mut extra = serde_json::Map::new();
        extra.insert("unreviewed".into(), serde_json::Value::Bool(true));
        let walk_waivers = replaced.as_ref().map(|r| r.walk_waivers.iter().map(|w| WalkWaiver { seam: w.seam.clone(), why: w.why.clone() }).collect()).unwrap_or_default();
        if let Some(r) = replaced {
            extra.insert("replaced".into(), serde_json::to_value(r).unwrap_or_default());
        }
        CrNote {
            schema: String::new(),
            id: Identity { tip: String::new(), base: String::new(), branch: None, committer: String::new(), timestamp: 0 },
            verdict: String::new(),
            waiver: None,
            blockers: 0,
            importants: 0,
            code_diff_hash: String::new(),
            walk_waivers,
            extra,
        }
    };
    let mut n = match existing {
        Some(n) if n.verdict == "PASS" => {
            return Err((
                format!("the review of {t} is a PASS; a review that passed is never waived over"),
                "sign it as reviewed (keel mr confirm without --waive-review); if that PASS no longer binds this tip (checks 4-5 red), run a new review on it".into(),
            ))
        }
        Some(n) if n.id.base == id.base && n.code_diff_hash == code_diff_hash => n,
        other => unreviewed(other),
    };
    n.schema = "cr/v1".into();
    n.id = id;
    n.verdict = "WAIVED".into();
    n.waiver = Some(reason);
    n.code_diff_hash = code_diff_hash;
    n.blockers = counts.0.unwrap_or(n.blockers);
    n.importants = counts.1.unwrap_or(n.importants);
    Ok(n)
}

/// The tip's cr note as stored, unparsed: `None` only when there is none.
pub(crate) fn cr_raw(repo: &Path, rev: &str) -> Option<String> {
    git(repo, &["notes", "--ref=cr", "show", rev], &[]).ok()
}

/// The cr note on `rev`, if one parses; the gate's check 1 reports a bad one.
pub(crate) fn cr_note(repo: &Path, rev: &str) -> Option<CrNote> {
    serde_json::from_str(&cr_raw(repo, rev)?).ok()
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
    /// Informational: authority is re-derived from the target's committed .keel/mr-allowed at check time.
    pub tier: String,
    /// The attested result report; absent when `[mr] report` asked for none and none was attested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// `sha256:<hex>` of the attested report's bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_hash: Option<String>,
    /// `sha256:<hex>` of the waiver reason the signer accepted (`--accept-waiver`); absent when the
    /// review was not waived. Check 8 rebuilds it from the tip's current cr note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver_hash: Option<String>,
    /// `sha256:<hex>` of the full `git diff base tip`.
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
    /// The report the verified mr/v1 sign-off attested, copied from it; absent when it attested none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_hash: Option<String>,
    /// "WAIVED" when the tip's own review was waived; absent after a reviewed PASS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<String>,
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
/// local diff config so every machine hashes the same bytes (cr and dod bindings).
pub(crate) fn diff_hash(repo: &Path, base: &str, tip: &str, pathspecs: &[String]) -> Result<String, String> {
    hash_blob(repo, &diff_text(repo, base, tip, pathspecs)?)
}

/// The bytes every diff binding hashes (cr and dod as a git blob, mr as SHA-256): `git diff base tip -- <pathspecs>`, local diff config off.
pub(crate) fn diff_text(repo: &Path, base: &str, tip: &str, pathspecs: &[String]) -> Result<String, String> {
    let mut a = vec!["diff", "--no-color", "--no-ext-diff", "--no-textconv", base, tip, "--"];
    a.extend(pathspecs.iter().map(String::as_str));
    git(repo, &a, &[])
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
    let mut h = no_git_env(Command::new("git"), true)
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

const CR_USAGE: &str = "usage: keel notes write cr --verdict PASS|FAIL [--blockers N] [--importants N]\n       keel notes write cr --verdict WAIVED --reason \"<the stop the review tool fixed>\" [--blockers N] [--importants N]   (the review cap only)";

/// The cr writer's rules on its flags; `Ok` carries the waiver reason to record, if any.
/// `reason`: `None` when `--reason` is absent, `Some("")` when it has no value.
fn cr_verdict(verdict: &str, reason: Option<&str>, blockers: u32, importants: u32) -> Result<Option<String>, String> {
    match (verdict, reason.map(str::trim)) {
        ("PASS" | "FAIL", Some(_)) => refuse(
            format!("--reason given with --verdict {verdict}: only a waiver carries a reason"),
            &format!("drop --reason. A waiver is the owner's: {OWNER_WAIVES}"),
        ),
        ("PASS", None) if blockers + importants > 0 => {
            refuse("PASS claimed with verified blockers/importants".into(), "record FAIL, or fix the findings first")
        }
        ("PASS" | "FAIL", None) => Ok(None),
        ("WAIVED", r) => match waiver_reason(r) {
            Ok(r) => Ok(Some(r)),
            Err(what) => refuse(
                what,
                &format!("give the reason the review tool stopped for (the review cap). Never write a waiver for the owner: {OWNER_WAIVES}"),
            ),
        },
        _ => Err(format!("{CR_USAGE}\n       an owner's waiver is not written here: {OWNER_WAIVES}")),
    }
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
            let reason = flag("--reason").or_else(|| args.contains(&"--reason").then_some(""));
            let waiver = cr_verdict(verdict, reason, blockers, importants)?;
            let code_diff_hash = code_diff_hash(repo, cfg, &r.id.base, &tip)?;
            let note = if waiver.is_some() {
                let counts = (args.contains(&"--blockers").then_some(blockers), args.contains(&"--importants").then_some(importants));
                waive(cr_raw(repo, &tip).as_deref(), r.id, waiver.as_deref(), code_diff_hash, counts).or_else(|(what, fix)| refuse(what, &fix))?
            } else {
                CrNote {
                    schema: "cr/v1".into(),
                    id: r.id,
                    verdict: verdict.into(),
                    waiver,
                    blockers,
                    importants,
                    code_diff_hash,
                    walk_waivers: vec![],
                    extra: Default::default(),
                }
            };
            store(repo, "cr", &tip, &note)?;
        }
        _ => return Err("usage: keel notes write <test|doc|cr>".into()),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{cr_verdict, waive, waiver_reason, CrNote, Identity};

    fn raw(n: &CrNote) -> String {
        serde_json::to_string(n).unwrap()
    }

    #[test]
    fn the_cr_writer_takes_a_reason_exactly_on_a_waiver() {
        assert_eq!(cr_verdict("PASS", None, 0, 0), Ok(None));
        assert_eq!(cr_verdict("FAIL", None, 2, 1), Ok(None));
        assert_eq!(cr_verdict("WAIVED", Some("  ruling \n"), 1, 0), Ok(Some("ruling".into())));
        assert_eq!(cr_verdict("WAIVED", Some("cap"), 0, 0), Ok(Some("cap".into())));
        for (v, r, b) in [
            ("PASS", None, 1),
            ("PASS", Some("x"), 0),
            ("FAIL", Some("x"), 1),
            ("FAIL", Some(""), 1),
            ("WAIVED", None, 0),
            ("WAIVED", Some(""), 0),
            ("WAIVED", Some(" \t\n"), 2),
        ] {
            let e = cr_verdict(v, r, b, 0).unwrap_err();
            assert!(e.contains("refused") && e.contains("fix ->"), "{v} {r:?} {b}: {e}");
        }
        for v in ["", "pass", "WAIVE", "CAPPED"] {
            assert!(cr_verdict(v, None, 0, 0).unwrap_err().starts_with("usage:"), "{v}");
        }
    }

    #[test]
    fn a_waiver_is_read_only_from_a_waived_verdict() {
        let note = |v: &str, w: Option<&str>| -> CrNote {
            let mut j = serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
                "timestamp": 0, "verdict": v, "blockers": 0, "importants": 0, "code_diff_hash": "h"});
            if let Some(w) = w {
                j["waiver"] = w.into();
            }
            serde_json::from_value(j).unwrap()
        };
        assert_eq!(note("WAIVED", Some("r")).waiver(), Some("r"));
        assert_eq!(note("WAIVED", None).waiver(), Some(""), "a hand-stripped reason reads blank, never absent");
        assert_eq!(note("PASS", Some("r")).waiver(), None, "a stray field on a PASS waives nothing");
        assert_eq!(note("FAIL", None).waiver(), None);
        let n = note("WAIVED", Some("r"));
        let back = serde_json::to_value(&n).unwrap();
        assert!(back.get("walk_waivers").is_none() && back.get("findings").is_none(), "{back}");
    }

    #[test]
    fn a_cr_note_keeps_the_fields_it_does_not_know_and_reads_its_open_findings() {
        let raw = serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c", "timestamp": 0,
            "verdict": "FAIL", "blockers": 1, "importants": 1, "code_diff_hash": "h",
            "findings": [{"claim": "open", "severity": "blocker"}, {"claim": "waived", "severity": "important", "waived": "r"}],
            "refuted": [{"claim": "x"}], "nits": [], "fleet": {"finders": 2}});
        let n: CrNote = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(n.open_findings().iter().map(|f| f["claim"].as_str().unwrap()).collect::<Vec<_>>(), ["open"]);
        assert_eq!(serde_json::to_value(&n).unwrap(), raw, "a round trip keeps every field");
        let bare: CrNote = serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null,
            "committer": "c", "timestamp": 0, "verdict": "PASS", "blockers": 0, "importants": 0, "code_diff_hash": "h", "findings": "prose"})).unwrap();
        assert!(bare.open_findings().is_empty(), "a non-array findings field lists nothing");
    }

    #[test]
    fn no_refusal_tells_an_agent_to_write_a_waiver_for_the_owner() {
        for (v, r, b) in [("PASS", Some("x"), 0), ("FAIL", Some("x"), 1), ("WAIVED", None, 0), ("WAIVED", Some(" "), 0)] {
            let e = cr_verdict(v, r, b, 0).unwrap_err();
            let fix = e.split("fix ->").nth(1).unwrap();
            assert!(fix.contains("--waive-review") && !fix.contains("--verdict WAIVED"), "{v} {r:?}: {e}");
        }
    }

    #[test]
    fn one_reason_rule_for_every_waiver_writer() {
        assert_eq!(waiver_reason(Some("  cap  ")), Ok("cap".into()));
        for bad in [None, Some(""), Some(" \t"), Some("--blockers"), Some(" --as")] {
            assert!(waiver_reason(bad).is_err(), "{bad:?}");
            assert!(cr_verdict("WAIVED", bad, 0, 0).is_err(), "the cr writer follows it: {bad:?}");
        }
    }

    #[test]
    fn the_one_waiver_writer_keeps_this_ranges_round_whole_marks_anything_else_unreviewed_and_never_replaces_a_pass() {
        let id = || Identity { tip: "t0123456789".into(), base: "b".into(), branch: None, committer: "o".into(), timestamp: 7 };
        let note = |v: &str, base: &str, hash: &str| -> CrNote {
            serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": base, "branch": null, "committer": "r",
                "timestamp": 0, "verdict": v, "waiver": "cap", "blockers": 2, "importants": 1, "code_diff_hash": hash,
                "findings": [{"severity": "blocker", "claim": "f"}], "refuted": [{"claim": "r"}], "nits": [], "fleet": {"finders": 2},
                "walk_waivers": [{"seam": "s", "why": "w"}]}))
            .unwrap()
        };
        let fresh = waive(None, id(), Some("ruling"), "h".into(), (None, None)).unwrap();
        assert_eq!((fresh.verdict.as_str(), fresh.waiver(), fresh.blockers, fresh.importants, fresh.unreviewed()), ("WAIVED", Some("ruling"), 0, 0, true));
        assert_eq!(fresh.waiver_hash().unwrap(), super::sha256(b"ruling"), "the hash is the stored reason's");
        let counted = waive(None, id(), Some("cap"), "h".into(), (Some(3), None)).unwrap();
        assert!(counted.unreviewed() && (counted.blockers, counted.importants) == (3, 0), "no review on the tip: unreviewed whatever the counts");
        for v in ["FAIL", "WAIVED"] {
            let same = raw(&note(v, "b", "h"));
            let w = waive(Some(&same), id(), Some(" ruling "), "h".into(), (None, None)).unwrap();
            assert_eq!((w.waiver(), w.blockers, w.importants, w.code_diff_hash.as_str(), w.id.base.as_str(), w.id.committer.as_str(), w.id.timestamp), (Some("ruling"), 2, 1, "h", "b", "o", 7), "{v}");
            assert!(!w.unreviewed(), "{v}");
            let (j, before) = (serde_json::to_value(&w).unwrap(), serde_json::to_value(note(v, "b", "h")).unwrap());
            for f in ["findings", "refuted", "nits", "fleet", "walk_waivers"] {
                assert_eq!(j[f], before[f], "{v}: {f} kept");
            }
            for (counts, want) in [((Some(0), None), (0, 1)), ((None, Some(4)), (2, 4)), ((Some(5), Some(6)), (5, 6))] {
                let c = waive(Some(&same), id(), Some("r"), "h".into(), counts).unwrap();
                assert_eq!((c.blockers, c.importants), want, "{v}: a given count replaces, an absent one keeps");
            }
            for (base, hash) in [("other", "h"), ("b", "other")] {
                let stale = waive(Some(&raw(&note(v, base, hash))), id(), Some("r"), "h".into(), (None, None)).unwrap();
                let j = serde_json::to_value(&stale).unwrap();
                assert!(stale.unreviewed() && (stale.blockers, stale.importants) == (0, 0) && stale.open_findings().is_empty(), "{v} {base} {hash}: {j}");
                assert_eq!((j["replaced"]["findings"][0]["claim"].as_str(), j["replaced"]["base"].as_str()), (Some("f"), Some(base)), "{v}: history kept whole");
                assert_eq!(j["walk_waivers"][0]["seam"], "s", "{v}: seam waivers carried");
            }
        }
        let (what, fix) = waive(Some(&raw(&note("PASS", "b", "h"))), id(), Some("ruling"), "h".into(), (None, None)).unwrap_err();
        assert!(what.contains("t012345678 is a PASS") && fix.contains("new review"), "{what} / {fix}");
        assert!(waive(Some(&raw(&note("PASS", "old", "old"))), id(), Some("r"), "h".into(), (None, None)).is_err(), "a stale PASS is not waived over either");
        assert!(waive(None, id(), Some("--blockers"), "h".into(), (None, None)).is_err());
        let (what, _) = waive(Some("{not json"), id(), Some("r"), "h".into(), (None, None)).unwrap_err();
        assert!(what.contains("does not parse"), "{what}");
    }

    #[test]
    fn the_identity_keys_come_from_the_type() {
        let mut k = Identity::keys();
        k.sort();
        assert_eq!(k, ["base", "branch", "committer", "timestamp", "tip"], "Identity changed: every place that strips it follows Identity::keys");
    }

    /// B5: a blank or non-string `waived` waives nothing, as everywhere else.
    #[test]
    fn a_finding_is_waived_only_by_a_waiver_in_words() {
        let n: CrNote = serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
            "timestamp": 0, "verdict": "FAIL", "blockers": 5, "importants": 0, "code_diff_hash": "h",
            "findings": [{"claim": "null", "waived": null}, {"claim": "false", "waived": false}, {"claim": "blank", "waived": "  "},
                {"claim": "none"}, {"claim": "words", "waived": "owner: fine"}]})).unwrap();
        let open: Vec<_> = n.open_findings().iter().map(|f| f["claim"].as_str().unwrap()).collect();
        assert_eq!(open, ["null", "false", "blank", "none"]);
    }
}
