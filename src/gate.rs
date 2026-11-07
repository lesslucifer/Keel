//! `keel verify-dod` + `keel preflight`: the merge gate (UC-GATE-2/3/5, UC-DOC-3 in part).
//! Ported from Trawler's xtask gate: same check ids, same `FAIL / fix ->` blocks, no cargo.
use crate::notes::{self, CrNote, DocNote, DodNote, TestNote};
use crate::{gate0, git, mr, Config};
use serde::de::DeserializeOwned;
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Commit,
    Wu,
    Epic,
}

impl Level {
    fn name(self) -> &'static str {
        match self {
            Level::Commit => "commit",
            Level::Wu => "wu",
            Level::Epic => "epic",
        }
    }
    /// The receipt names of the checks this level runs.
    fn checks(self) -> &'static [&'static str] {
        match self {
            Level::Commit => &["notes", "manifest", "doc-note"],
            Level::Wu => &["notes", "manifest", "review", "review-base", "diff-binding", "doc-note", "manual-review"],
            Level::Epic => &["notes", "manifest", "review", "review-base", "diff-binding", "doc-note", "walk", "manual-review"],
        }
    }
}

struct Failure {
    check: String,
    what: String,
    fix: String,
}

struct Report {
    level: Level,
    tip_ref: String,
    tip: String,
    base: String,
    exempt: bool,
    failures: Vec<Failure>,
    waived: Vec<String>,
}

impl Report {
    fn fail(&mut self, check: &str, what: String, fix: impl Into<String>) {
        self.failures.push(Failure { check: check.into(), what, fix: fix.into() });
    }
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

fn is_ancestor(repo: &Path, commit: &str, of: &str) -> bool {
    git(repo, &["merge-base", "--is-ancestor", commit, of], &[]).is_ok()
}

fn open(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str) -> Result<(Report, Vec<String>), String> {
    let tip = git(repo, &["rev-parse", &format!("{tip_ref}^{{commit}}")], &[])?;
    let base = git(repo, &["merge-base", target, &tip], &[])?;
    let names = notes::names(repo, &base, &tip)?;
    // UC-GATE-5: docs/infra = anything outside code_paths.
    let exempt = !names.iter().any(|n| notes::is_code(cfg, n));
    let r = Report { level, tip_ref: tip_ref.into(), tip, base, exempt, failures: vec![], waived: vec![] };
    Ok((r, names))
}

/// Check 1 for one note: present (when required), JSON, declaring `<kind>/v1`.
fn read<T: DeserializeOwned>(repo: &Path, r: &mut Report, kind: &str, required: bool) -> Option<T> {
    let schema = format!("{kind}/v1");
    let Ok(raw) = git(repo, &["notes", &format!("--ref={kind}"), "show", &r.tip], &[]) else {
        if required {
            // A standalone commit's range is its parent, not the branch's target.
            let t = if r.level == Level::Commit { format!(" --target {}^", short(&r.tip)) } else { String::new() };
            let fix = match kind {
                "test" => format!("keel notes write test{t} < judgment.json   (on {})", short(&r.tip)),
                "doc" => format!("keel notes write doc{t}   (on {})", short(&r.tip)),
                _ => format!("run the code review on {}, then keel notes write cr --verdict PASS", short(&r.tip)),
            };
            r.fail("1·notes", format!("no {schema} note on {}", short(&r.tip)), fix);
        }
        return None;
    };
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| e.to_string()).and_then(|v| {
        match v["schema"].as_str() {
            Some(s) if s == schema => serde_json::from_value::<T>(v).map_err(|e| e.to_string()),
            other => Err(format!("declares schema {}, expected {schema}", other.unwrap_or("none"))),
        }
    });
    match parsed {
        Ok(n) => Some(n),
        Err(e) => {
            r.fail("1·notes", format!("{kind} note on {} is not valid {schema}: {e}", short(&r.tip)), format!("rewrite it with keel notes write {kind}"));
            None
        }
    }
}

/// Checks 1-2 (paperwork, all the preflight reads) or 1-6 on the tip itself.
fn check_tip(repo: &Path, cfg: &Config, r: &mut Report, names: &[String], full: bool) -> Result<(), String> {
    let review = full && r.level != Level::Commit;
    let docs: Vec<&String> = names.iter().filter(|n| n.starts_with(&cfg.docs.root)).collect();
    let test = read::<TestNote>(repo, r, "test", true);
    let cr = if review { read::<CrNote>(repo, r, "cr", true) } else { None };
    let doc = if full { read::<DocNote>(repo, r, "doc", r.level != Level::Commit || !docs.is_empty()) } else { None };

    // 2·manifest: the level's layer, and every note bound to this very commit.
    if let Some(t) = &test {
        let (layer, have) = match r.level {
            Level::Commit => ("unit", &t.unit),
            Level::Wu => ("integration", &t.integration),
            Level::Epic => ("e2e", &t.e2e),
        };
        if have.is_none() {
            r.fail(
                "2·manifest",
                format!("level {} needs the {layer} layer - the test note has none", r.level.name()),
                format!("add {layer} tests and rewrite the note, or record {{\"{layer}\":{{\"waived\":\"<why>\"}}}}"),
            );
        }
    }
    let tips = [test.as_ref().map(|n| &n.id.tip), cr.as_ref().map(|n| &n.id.tip), doc.as_ref().map(|n| &n.id.tip)];
    if tips.iter().flatten().any(|t| **t != r.tip) {
        r.fail("2·manifest", format!("a note on {} names a different tip (copied from another commit?)", short(&r.tip)), "rewrite the notes on this tip with keel notes write");
    }

    // 3-5: the review verdict, its base, its binding to the code diff.
    if let Some(cr) = &cr {
        if cr.verdict != "PASS" {
            r.fail("3·review", format!("review verdict is {}", cr.verdict), "fix the verified findings and re-run the review until PASS");
        }
        if cr.id.base != r.base {
            r.fail(
                "4·review-base",
                format!("review base {} != current merge-base {}", short(&cr.id.base), short(&r.base)),
                "the target moved since the review - re-run the review against the current base",
            );
        }
        if cr.code_diff_hash != notes::code_diff_hash(repo, cfg, &r.base, &r.tip)? {
            r.fail("5·diff-binding", "reviewed code is not byte-identical to the code this tip changes".into(), "the tip changed after review - re-run the review on this exact tip");
        }
    }

    // 6·doc-note: every changed doc listed, nothing phantom. Tier layout is W8's.
    if let Some(doc) = &doc {
        for p in docs.iter().filter(|p| !doc.paths.contains(p)) {
            r.fail("6·doc-note", format!("{p} changed in the range but is not in doc/v1"), "keel notes write doc   (on the tip)");
        }
        for p in doc.paths.iter().filter(|p| !docs.contains(p)) {
            r.fail("6·doc-note", format!("doc/v1 lists {p} but the range does not touch it"), "keel notes write doc   (on the tip)");
        }
    }
    Ok(())
}

fn verify(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str) -> Result<Report, String> {
    let (mut r, names) = open(repo, cfg, level, tip_ref, target)?;
    // UC-GATE-5 never exempts these: W8's tier layout goes here. W4's 8·manual-review runs in
    // `run()` instead - this fn is re-entered per walk seam and by mr confirm, neither of which may judge it.
    if !r.exempt {
        check_tip(repo, cfg, &mut r, &names, true)?;
    }
    if level == Level::Epic {
        walk(repo, cfg, &mut r, target)?;
    }
    r.exempt &= r.failures.is_empty();
    Ok(r)
}

/// 7·walk: every first-parent seam since the base must pass its own gate.
fn walk(repo: &Path, cfg: &Config, r: &mut Report, target: &str) -> Result<(), String> {
    let waivers = git(repo, &["notes", "--ref=cr", "show", &r.tip], &[])
        .ok()
        .and_then(|raw| serde_json::from_str::<CrNote>(&raw).ok())
        .map(|n| n.walk_waivers)
        .unwrap_or_default();
    let lines = git(repo, &["rev-list", "--first-parent", "--parents", &format!("{}..{}", r.base, r.tip)], &[])?;
    for line in lines.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let (seam, failures, level) = match p.as_slice() {
            // Parent 2 already reachable from the mainline or the target: a back-merge, not a seam.
            [_, main, wu] if is_ancestor(repo, wu, main) || is_ancestor(repo, wu, target) => continue,
            [m, main, wu] => (*m, verify(repo, cfg, Level::Wu, wu, main)?.failures, "wu"),
            [c, _] if *c == r.tip => continue, // the epic's own checks cover its tip
            [c, first] => (*c, verify(repo, cfg, Level::Commit, c, first)?.failures, "commit"),
            [m, ..] if p.len() > 3 => {
                let what = format!("{} is an octopus merge - parents beyond the second were never gated", short(m));
                (*m, vec![Failure { check: String::new(), what, fix: "merge one WU per --no-ff merge commit".into() }], "merge")
            }
            _ => continue,
        };
        if failures.is_empty() {
            continue;
        }
        let why = waivers.iter().find(|w| {
            !w.seam.is_empty() && !w.why.trim().is_empty() && (seam.starts_with(&w.seam) || w.seam.starts_with(seam))
        });
        if let Some(w) = why {
            r.waived.push(format!("7·walk seam {} waived: {}", short(seam), w.why.trim()));
            continue;
        }
        let hint = format!(
            "; or, if this seam is safe, waive it on the record: git notes --ref=cr edit {} and add \"walk_waivers\":[{{\"seam\":\"{}\",\"why\":\"<why safe>\"}}] - never --no-verify, which skips every gate",
            short(&r.tip),
            short(seam)
        );
        for f in failures {
            let check = if f.check.is_empty() { "7·walk".to_string() } else { format!("7·walk·{}", f.check) };
            r.fail(&check, format!("[{level} {}] {}", short(seam), f.what), format!("{}{hint}", f.fix));
        }
    }
    Ok(())
}

fn print(r: &Report, what: &str) {
    for w in &r.waived {
        println!("WAIVED {w}");
    }
    if r.exempt {
        println!("{what} {} exempt on {} - docs/infra-only, test/cr/doc evidence not required", r.level.name(), short(&r.tip));
    } else if r.failures.is_empty() {
        println!("{what} {} green on {} (checks: {})", r.level.name(), short(&r.tip), r.level.checks().join(", "));
    } else {
        for f in &r.failures {
            eprintln!("FAIL  {:<24} {}\n      fix -> {}", f.check, f.what, f.fix);
        }
        eprintln!("{what} {} red on {} - {} failed", r.level.name(), short(&r.tip), r.failures.len());
    }
}

/// Verify, print, and on a full green wu/epic write the dod/v1 receipt.
fn run(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str) -> Result<i32, String> {
    let mut r = verify(repo, cfg, level, tip_ref, target)?;
    // 8·manual-review: never exempt (UC-GATE-5), and only at the merge boundary - the walk never
    // re-judges a seam's sign-off against today's local roster (Trawler DEC-9/10).
    if level != Level::Commit {
        if let Err((what, fix)) = mr::manual_review(repo, cfg, level.name(), &r.tip, &r.base) {
            r.fail("8·manual-review", what, fix);
        }
        r.exempt &= r.failures.is_empty();
    }
    print(&r, "verify-dod");
    if !r.failures.is_empty() {
        return Ok(1);
    }
    if !r.exempt && level != Level::Commit {
        let diff_hash = notes::hash_blob(repo, &git(repo, &["diff", &r.base, &r.tip], &[])?)?;
        let branch = (r.tip_ref != r.tip).then(|| r.tip_ref.clone());
        let note = DodNote {
            schema: "dod/v1".into(),
            id: notes::identity(repo, r.tip.clone(), r.base.clone(), branch)?,
            level: level.name().into(),
            checks: level.checks().iter().map(|c| c.to_string()).collect(),
            diff_hash,
        };
        notes::store(repo, "dod", &r.tip, &note)?;
    }
    Ok(0)
}

/// mr confirm's sign-last rule (Trawler DEC-27): checks 1-7 must already be green on the tip.
/// Red -> the failures are printed and `None`; green -> `(tip, base)` of the range to sign.
pub(crate) fn sign_ready(repo: &Path, cfg: &Config, epic: bool, tip_ref: &str, rest: &[&str]) -> Result<Option<(String, String)>, String> {
    let level = if epic { Level::Epic } else { Level::Wu };
    let r = verify(repo, cfg, level, tip_ref, &target_of(cfg, level, tip_ref, rest))?;
    if !r.failures.is_empty() {
        print(&r, "mr confirm");
        return Ok(None);
    }
    Ok(Some((r.tip, r.base)))
}

/// pre-merge-commit: tip = the merge head, target = HEAD, level from HEAD's branch.
/// `cfg` is HEAD's committed config (main.rs `load_config_at_head`), not the merged tree's.
fn hook(repo: &Path, cfg: &Config) -> Result<i32, String> {
    let branch = git(repo, &["symbolic-ref", "--short", "-q", "HEAD"], &[]).unwrap_or_default();
    let level = if branch == cfg.branches.main {
        Level::Epic
    } else if branch.starts_with("epic/") {
        Level::Wu
    } else {
        return Ok(0); // merges into other branches are ungated
    };
    let mut heads: Vec<String> = std::fs::read_to_string(repo.join(git(repo, &["rev-parse", "--git-path", "MERGE_HEAD"], &[])?))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    if heads.is_empty() {
        // An automatic merge runs pre-merge-commit before MERGE_HEAD exists; the revs are in the reflog action.
        let action = std::env::var("GIT_REFLOG_ACTION").unwrap_or_default();
        for rev in action.strip_prefix("merge ").unwrap_or("").split_whitespace() {
            heads.push(git(repo, &["rev-parse", &format!("{rev}^{{commit}}")], &[])?);
        }
    }
    match heads.as_slice() {
        // Fail closed: e.g. `git pull` sets GIT_REFLOG_ACTION to "pull ...", so the heads are unknowable.
        [] => {
            eprintln!(
                "merge refused - cannot determine what is being merged into {branch} (no MERGE_HEAD, GIT_REFLOG_ACTION={:?}), so the gate cannot judge it\n  fix -> git merge --abort, then merge the branch directly: git merge --no-ff <branch>   (fetch first instead of git pull)",
                std::env::var("GIT_REFLOG_ACTION").unwrap_or_default()
            );
            Ok(1)
        }
        [tip] if level == Level::Wu && is_ancestor(repo, tip, &cfg.branches.main) => {
            println!("back-merge of landed history {} - not a WU seam, gate not required", short(tip));
            Ok(0)
        }
        [tip] => {
            let code = run(repo, cfg, level, tip, "HEAD")?;
            if code != 0 || level == Level::Wu {
                return Ok(code);
            }
            let code = gate0(repo, cfg)?; // the merged tree, before it lands on main
            if code != 0 {
                eprintln!("merge refused - the merged tree fails gate0; git merge --abort, fix it on the epic, merge again");
            }
            Ok(code)
        }
        _ => {
            eprintln!("octopus merge refused - the gate verifies one branch per merge; merge WUs one at a time");
            Ok(1)
        }
    }
}

/// Every first-parent plain commit in old..new must pass the commit gate (pre-push).
pub(crate) fn range(repo: &Path, cfg: &Config, old: &str, new: &str) -> Result<i32, String> {
    let mut red = false;
    for line in git(repo, &["rev-list", "--first-parent", "--parents", &format!("{old}..{new}")], &[])?.lines() {
        if let [c, first] = line.split_whitespace().collect::<Vec<_>>().as_slice() {
            let r = verify(repo, cfg, Level::Commit, c, first)?;
            if !r.failures.is_empty() {
                print(&r, "verify-dod");
                red = true;
            }
        }
    }
    Ok(red as i32)
}

/// Before a review starts: right checkout, clean tree, gate0 green, checks 1-2 on the test note.
fn preflight(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str) -> Result<i32, String> {
    let (mut r, names) = open(repo, cfg, level, tip_ref, target)?;
    if r.exempt {
        print(&r, "preflight");
        return Ok(0);
    }
    if git(repo, &["rev-parse", "HEAD"], &[])? != r.tip {
        r.fail("0·tree", format!("the checkout is not on the tip {}", short(&r.tip)), format!("git checkout {tip_ref}, then preflight again"));
    } else if !git(repo, &["status", "--porcelain", "--untracked-files=no"], &[])?.is_empty() {
        r.fail("0·tree", "the working tree has uncommitted changes".into(), "commit or stash them, then preflight again");
    } else if gate0(repo, cfg)? != 0 {
        r.fail("0·tree", "gate0 is red on the tip".into(), "get lint and tests green before the review runs");
    }
    check_tip(repo, cfg, &mut r, &names, false)?;
    print(&r, "preflight");
    Ok(!r.failures.is_empty() as i32)
}

const USAGE: &str = "usage: keel verify-dod <wu|epic> <tip> [--target R] | commit <sha> | hook | range <old> <new>\n       keel preflight <wu|epic> <tip> [--target R]";

fn level_of(s: &str) -> Result<Level, String> {
    match s {
        "wu" => Ok(Level::Wu),
        "epic" => Ok(Level::Epic),
        _ => Err(USAGE.into()),
    }
}

/// Epic default target is main; a WU's is inferred from its branch (notes::target).
fn target_of(cfg: &Config, level: Level, tip: &str, rest: &[&str]) -> String {
    notes::target(cfg, if level == Level::Epic { "" } else { tip }, rest)
}

pub fn verify_dod(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    match args {
        ["commit", sha] => run(repo, cfg, Level::Commit, sha, &format!("{sha}^")),
        ["hook"] => hook(repo, cfg),
        ["range", old, new] => range(repo, cfg, old, new),
        [lvl, tip, rest @ ..] => {
            let level = level_of(lvl)?;
            run(repo, cfg, level, tip, &target_of(cfg, level, tip, rest))
        }
        _ => Err(USAGE.into()),
    }
}

pub fn preflight_cmd(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    match args {
        [lvl, tip, rest @ ..] => {
            let level = level_of(lvl)?;
            preflight(repo, cfg, level, tip, &target_of(cfg, level, tip, rest))
        }
        _ => Err(USAGE.into()),
    }
}
