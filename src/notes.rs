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
    /// Paths under docs.root changed in the range (tier classification is W8).
    pub paths: Vec<String>,
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

// Written by W4; schema only.
#[derive(Serialize, Deserialize, Debug)]
#[allow(dead_code)]
pub struct MrNote {
    pub schema: String, // "mr/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub signer: String,
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
/// judge one range: `--target R`, else a `wu/<e>/..` tip's `epic/<e>`, else main.
pub(crate) fn target(cfg: &Config, tip_ref: &str, args: &[&str]) -> String {
    if let Some(t) = args.windows(2).find(|w| w[0] == "--target") {
        return t[1].to_string();
    }
    match tip_ref.strip_prefix("wu/").and_then(|r| r.split_once('/')) {
        Some((epic, _)) if !epic.is_empty() => format!("epic/{epic}"),
        _ => cfg.branches.main.clone(),
    }
}

pub(crate) fn names(repo: &Path, base: &str, tip: &str) -> Result<Vec<String>, String> {
    Ok(git(repo, &["diff", "--name-only", base, tip], &[])?.lines().map(String::from).collect())
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
    let names = names(repo, &base, &tip)?;
    Ok(Range { id: identity(repo, tip, base, branch)?, names })
}

/// A path the gate treats as code (and cr binds). Every path is code when code_paths is empty.
pub(crate) fn is_code(cfg: &Config, path: &str) -> bool {
    cfg.code_paths.is_empty() || cfg.code_paths.iter().any(|p| path.starts_with(p.as_str()))
}

/// One function for the writer (W2) and check 5 (W3), so the two can never disagree.
pub(crate) fn code_diff_hash(repo: &Path, cfg: &Config, base: &str, tip: &str) -> Result<String, String> {
    let mut a = vec!["diff", base, tip, "--"];
    a.extend(cfg.code_paths.iter().map(String::as_str));
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
    println!("wrote refs/notes/{kind} on {}", &tip[..8]);
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
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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
            let paths = r.names.iter().filter(|n| n.starts_with(&cfg.docs.root)).cloned().collect();
            store(repo, "doc", &tip, &DocNote { schema: "doc/v1".into(), id: r.id, paths })?;
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
