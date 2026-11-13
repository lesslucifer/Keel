//! `keel verify-dod` + `keel preflight`: the merge gate (UC-GATE-2/3/5, UC-DOC-3).
//! Ported from Trawler's xtask gate: same check ids, same `FAIL / fix ->` blocks, no cargo.
use crate::notes::{self, CrNote, DocNote, DodNote, TestNote};
use crate::{check, git, hooks_problem, mr, review, short, Config, HOOKS_FIX};
use serde::de::DeserializeOwned;
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Commit,
    Wu,
    Epic,
    /// A plain commit on an epic branch judged by the epic-range notes it carries (B20, B23).
    Close,
}

impl Level {
    fn name(self) -> &'static str {
        match self {
            Level::Commit => "commit",
            Level::Wu => "wu",
            Level::Epic => "epic",
            Level::Close => "close",
        }
    }
    /// The receipt names of the checks this level runs.
    fn checks(self) -> &'static [&'static str] {
        match self {
            Level::Commit | Level::Close => &["notes", "manifest", "doc-note"],
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
    /// The tip's own review was waived: its reason, for the dod receipt.
    review_waiver: Option<String>,
    /// The tip's cr note as checks 3-5 read it, for check 8's waiver binding (read once).
    tip_cr: Option<CrNote>,
}

impl Report {
    fn fail(&mut self, check: &str, what: String, fix: impl Into<String>) {
        self.failures.push(Failure { check: check.into(), what, fix: fix.into() });
    }
}

fn is_ancestor(repo: &Path, commit: &str, of: &str) -> bool {
    git(repo, &["merge-base", "--is-ancestor", commit, of], &[]).is_ok()
}

fn open(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str) -> Result<(Report, Vec<String>), String> {
    let tip = git(repo, &["rev-parse", &format!("{tip_ref}^{{commit}}")], &[])?;
    let base = git(repo, &["merge-base", target, &tip], &[])?;
    let names = notes::names(repo, &base, &tip, &[])?;
    let exempt = notes::exempt(repo, cfg, &base, &tip, &names)?;
    let r = Report { level, tip_ref: tip_ref.into(), tip, base, exempt, failures: vec![], waived: vec![], review_waiver: None, tip_cr: None };
    Ok((r, names))
}

/// Check 1 for one note: present (when required), JSON, declaring `<kind>/v1`.
fn read<T: DeserializeOwned>(repo: &Path, r: &mut Report, kind: &str, required: bool) -> Option<T> {
    let schema = format!("{kind}/v1");
    let stored = match notes::raw(repo, kind, &r.tip) {
        Ok(stored) => stored,
        Err(e) => {
            r.fail(
                "1·notes",
                format!("the {kind} note on {} cannot be read: {e}", short(&r.tip)),
                format!("repair refs/notes/{kind} (git fsck names what is broken), then re-run"),
            );
            return None;
        }
    };
    let Some(raw) = stored else {
        if required {
            // A standalone commit's range is its parent, not the branch's target.
            let t = if r.level == Level::Commit { format!(" --target {}^", short(&r.tip)) } else { String::new() };
            let fix = match kind {
                "test" => format!("keel notes write test{t} < judgment.json   (on {})", short(&r.tip)),
                "doc" => format!("keel notes write doc{t}   (on {})", short(&r.tip)),
                _ => format!("run the code review on {}, then keel notes write cr --verdict PASS   (or {})", short(&r.tip), notes::OWNER_WAIVES),
            };
            r.fail("1·notes", format!("no {schema} note on {}", short(&r.tip)), fix);
        }
        return None;
    };
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| e.to_string()).and_then(|v| match v["schema"].as_str() {
        Some(s) if s == schema => serde_json::from_value::<T>(v).map_err(|e| e.to_string()),
        other => Err(format!("declares schema {}, expected {schema}", other.unwrap_or("none"))),
    });
    match parsed {
        Ok(n) => Some(n),
        Err(e) => {
            r.fail("1·notes", format!("{kind} note on {} is not valid {schema}: {e}", short(&r.tip)), format!("rewrite it with keel notes write {kind}"));
            None
        }
    }
}

/// Checks 1-2 (paperwork, all the preflight reads) or 1-6 on the tip itself. `own_review`: false
/// only when the signer is about to waive the tip's review, so its cr note is neither required nor judged.
fn check_tip(repo: &Path, cfg: &Config, r: &mut Report, names: &[String], full: bool, own_review: bool) -> Result<(), String> {
    let review = full && own_review && matches!(r.level, Level::Wu | Level::Epic);
    let docs = notes::doc_paths(repo, cfg, &r.base, &r.tip, names)?;
    let test = read::<TestNote>(repo, r, "test", true);
    let cr = if review { read::<CrNote>(repo, r, "cr", true) } else { None };
    let doc = if full { read::<DocNote>(repo, r, "doc", r.level != Level::Commit || !docs.is_empty()) } else { None };

    // 2·manifest: the level's layer, and every note bound to this very commit.
    if let Some(t) = &test {
        let (layer, have) = match r.level {
            Level::Commit => ("unit", &t.unit),
            Level::Wu => ("integration", &t.integration),
            Level::Epic | Level::Close => ("e2e", &t.e2e),
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
        r.fail(
            "2·manifest",
            format!("a note on {} names a different tip (copied from another commit?)", short(&r.tip)),
            "rewrite the notes on this tip with keel notes write",
        );
    }

    // 3-5: the review verdict, its base, its binding to the code diff.
    if let Some(cr) = &cr {
        match review_verdict(cr) {
            Ok(None) => {}
            Ok(Some(why)) => {
                r.waived.push(format!("3·review: {why}"));
                r.review_waiver = Some(why);
            }
            Err((what, fix)) => r.fail("3·review", what, fix),
        }
        if cr.id.base != r.base {
            r.fail(
                "4·review-base",
                format!("review base {} != current merge-base {}", short(&cr.id.base), short(&r.base)),
                "the target moved since the review - re-run the review against the current base",
            );
        }
        if cr.code_diff_hash != notes::code_diff_hash(repo, cfg, &r.base, &r.tip)? {
            r.fail(
                "5·diff-binding",
                "reviewed code is not byte-identical to the code this tip changes".into(),
                "the tip changed after review - re-run the review on this exact tip",
            );
        }
    }

    if let Some(doc) = &doc {
        doc_note(repo, cfg, r, &docs, doc);
    }
    r.tip_cr = cr;
    Ok(())
}

/// 3·review's rule: PASS passes; WAIVED passes with a reason the one reason rule accepts
/// (notes::waiver_reason, `Ok(Some)`), never without one.
fn review_verdict(cr: &CrNote) -> Result<Option<String>, (String, String)> {
    match (cr.verdict.as_str(), cr.waiver()) {
        ("PASS", _) => Ok(None),
        (_, Some(why)) => notes::waiver_reason(Some(why))
            .map(Some)
            .map_err(|_| ("review verdict is WAIVED with no reason".into(), format!("a waiver needs its reason in words: {}", notes::OWNER_WAIVES))),
        (v, None) => Err((format!("review verdict is {v}"), "fix the verified findings and re-run the review until PASS".into())),
    }
}

/// 6·doc-note: every doc listed with its config tier, nothing phantom, archived T3 with lineage.
fn doc_note(repo: &Path, cfg: &Config, r: &mut Report, docs: &[String], doc: &DocNote) {
    for (what, fix) in notes::doc_problems(repo, cfg, &r.tip, docs, &doc.changes) {
        r.fail("6·doc-note", what, fix);
    }
}

/// 6·doc-note at an epic tip (UC-DOC-3): nothing tracked under docs.root outside T0-T2.
fn tracked_layout(repo: &Path, cfg: &Config, r: &mut Report) -> Result<(), String> {
    let root = cfg.docs.root.trim_end_matches('/');
    for p in notes::zpaths(&git(repo, &["ls-tree", "-r", "-z", "--name-only", &r.tip, "--", root], &[])?) {
        match notes::tier_of(cfg, &p) {
            Some("T3") => r.fail(
                "6·doc-note",
                format!("{p} is a T3 working doc still tracked at the epic tip"),
                format!("distill it, git rm it (keep the file in {root}/.archive/, gitignored), then keel notes write doc --distill {p}=<T1/T2 doc>"),
            ),
            None => r.fail(
                "6·doc-note",
                format!("{p} is tracked under {root}/ but matches no docs.tiers pattern"),
                "add a glob for it under [docs.tiers] in .keel/config.toml, or move or archive it",
            ),
            _ => {}
        }
    }
    Ok(())
}

fn verify(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str) -> Result<Report, String> {
    verify_with(repo, cfg, level, tip_ref, target, true)
}

fn verify_with(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str, own_review: bool) -> Result<Report, String> {
    let (mut r, names) = open(repo, cfg, level, tip_ref, target)?;
    // UC-GATE-5 never exempts W8's archive/tier-layout checks (below). W4's 8·manual-review runs in
    // `run()` instead - this fn is re-entered per walk seam and by mr confirm, neither of which may judge it.
    if !r.exempt {
        check_tip(repo, cfg, &mut r, &names, true, own_review)?;
    } else {
        // An archive commit is docs-only by nature; exempting it would leave distillation unchecked.
        let docs = notes::doc_paths(repo, cfg, &r.base, &r.tip, &names)?;
        if notes::archives_t3(repo, cfg, &r.tip, &docs) {
            if let Some(doc) = read::<DocNote>(repo, &mut r, "doc", true) {
                doc_note(repo, cfg, &mut r, &docs, &doc);
            }
        }
    }
    // The tracked-doc layout is the epic tip's alone: a close commit before the archive still tracks T3.
    if level == Level::Epic {
        tracked_layout(repo, cfg, &mut r)?;
        walk(repo, cfg, &mut r, target)?;
    }
    r.exempt &= r.failures.is_empty();
    Ok(r)
}

/// 7·walk: every first-parent seam since the base must pass its own gate.
fn walk(repo: &Path, cfg: &Config, r: &mut Report, target: &str) -> Result<(), String> {
    let waivers = notes::cr_note(repo, &r.tip).map(|n| n.walk_waivers).unwrap_or_default();
    let lines = git(repo, &["rev-list", "--first-parent", "--parents", &format!("{}..{}", r.base, r.tip)], &[])?;
    for line in lines.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let (seam, failures, level) = match p.as_slice() {
            // Parent 2 already reachable from the mainline or the target: a back-merge, not a seam.
            [_, main, wu] if is_ancestor(repo, wu, main) || is_ancestor(repo, wu, target) => continue,
            [m, main, wu] => {
                let seam = verify(repo, cfg, Level::Wu, wu, main)?;
                if seam.failures.is_empty() {
                    r.waived.extend(seam.waived.iter().map(|w| format!("7·walk [wu {}] {w}", short(m))));
                }
                (*m, seam.failures, "wu")
            }
            [c, _] if *c == r.tip => continue, // the epic's own checks cover its tip
            [c, first] => {
                let level = plain_seam(repo, cfg, c, first, target, false)?;
                let range = if level == Level::Close { target } else { first };
                (*c, verify(repo, cfg, level, c, range)?.failures, level.name())
            }
            [m, ..] if p.len() > 3 => {
                let what = format!("{} is an octopus merge - parents beyond the second were never gated", short(m));
                (*m, vec![Failure { check: String::new(), what, fix: "merge one WU per --no-ff merge commit".into() }], "merge")
            }
            _ => continue,
        };
        if failures.is_empty() {
            continue;
        }
        let why = waivers.iter().find(|w| !w.seam.is_empty() && !w.why.trim().is_empty() && (seam.starts_with(&w.seam) || w.seam.starts_with(seam)));
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

/// Guard D (UC-GATE-6), first in the report: a gate command run where git is not running Keel's hooks.
fn guard_hooks(repo: &Path, r: &mut Report) {
    if let Some(what) = hooks_problem(repo) {
        r.failures.insert(0, Failure { check: "0·hooks".into(), what, fix: HOOKS_FIX.into() });
        r.exempt = false;
    }
}

/// Verify, print, and on a full green wu/epic write the dod/v1 receipt. `guard`: run guard D
/// (verify-dod wu|epic by hand; the hook, range and commit gates run from Keel's hooks by construction).
fn run(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str, guard: bool) -> Result<i32, String> {
    let mut r = verify(repo, cfg, level, tip_ref, target)?;
    if guard {
        guard_hooks(repo, &mut r);
    }
    // 8·manual-review: never exempt (UC-GATE-5), and only at the merge boundary - the walk never
    // re-judges a seam's sign-off against the epic's roster (Trawler DEC-9/10; DEC-48 names the gap).
    let mut attested = None;
    if level != Level::Commit {
        match mr::manual_review(repo, cfg, level.name(), &r.tip, &r.base, target, r.tip_cr.as_ref()) {
            Ok(a) => attested = a,
            Err((what, fix)) => r.fail("8·manual-review", what, fix),
        }
        r.exempt &= r.failures.is_empty();
    }
    print(&r, "verify-dod");
    if !r.failures.is_empty() {
        return Ok(1);
    }
    if !r.exempt && level != Level::Commit {
        let diff_hash = notes::diff_hash(repo, &r.base, &r.tip, &[])?;
        let branch = (r.tip_ref != r.tip).then(|| r.tip_ref.clone());
        let note = DodNote {
            schema: "dod/v1".into(),
            id: notes::identity(repo, r.tip.clone(), r.base.clone(), branch)?,
            level: level.name().into(),
            checks: level.checks().iter().map(|c| c.to_string()).collect(),
            diff_hash,
            report: attested.as_ref().map(|(r, _)| r.clone()),
            report_hash: attested.map(|(_, h)| h),
            review: r.review_waiver.is_some().then(|| "WAIVED".into()),
            waiver: r.review_waiver.clone(),
        };
        notes::store(repo, "dod", &r.tip, &note)?;
    }
    Ok(0)
}

/// mr confirm's sign-last rule (Trawler DEC-27): checks 1-7 must already be green on the tip.
/// Red -> the failures are printed and `None`; green -> `(tip, base, target)` of the range to sign.
/// `waive`: the signer waives the tip's own review in this sign-off (UC-REV-2), so its review - the
/// cr note and checks 3-5 on it - is the one thing not yet required; everything else still is.
pub(crate) fn sign_ready(repo: &Path, cfg: &Config, epic: bool, tip_ref: &str, rest: &[&str], waive: bool) -> Result<Option<(String, String, String)>, String> {
    let level = if epic { Level::Epic } else { Level::Wu };
    let target = target_of(cfg, level, tip_ref, rest);
    let r = verify_with(repo, cfg, level, tip_ref, &target, !waive)?;
    if !r.failures.is_empty() {
        print(&r, "mr confirm");
        return Ok(None);
    }
    Ok(Some((r.tip, r.base, target)))
}

/// pre-merge-commit: tip = the merge head, target = HEAD, level from HEAD's branch.
/// `cfg` is HEAD's committed config (main.rs `load_config_at_head`), not the merged tree's.
fn hook(repo: &Path, cfg: &Config) -> Result<i32, String> {
    let branch = git(repo, &["symbolic-ref", "--short", "-q", "HEAD"], &[]).unwrap_or_default();
    let level = if branch == cfg.branches.main {
        Level::Epic
    } else if branch.starts_with(cfg.branches.epic_prefix.as_str()) {
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
            let code = run(repo, cfg, level, tip, "HEAD", false)?;
            if code != 0 || level == Level::Wu {
                return Ok(code);
            }
            let code = check(repo, cfg)?; // the merged tree, before it lands on main
            if code != 0 {
                eprintln!("merge refused - the merged tree fails keel check; git merge --abort, fix it on the epic, merge again");
            }
            Ok(code)
        }
        _ => {
            eprintln!("octopus merge refused - the gate verifies one branch per merge; merge WUs one at a time");
            Ok(1)
        }
    }
}

/// pre-push: every first-parent seam in old..new of `branch` passes its own gate - a plain commit
/// the commit gate, a merge its level's gate (into main: epic, else wu) and sign-off. A `--no-commit`
/// merge never ran pre-merge-commit and a `--ff-only` one made no merge commit, so this is the last gate.
pub(crate) fn range(repo: &Path, cfg: &Config, old: &str, new: &str, branch: &str) -> Result<i32, String> {
    let merges = if branch == cfg.branches.main { Level::Epic } else { Level::Wu };
    let main = cfg.branches.main.as_str();
    let tip = git(repo, &["rev-parse", &format!("{new}^{{commit}}")], &[])?;
    let mut red = false;
    for line in git(repo, &["rev-list", "--first-parent", "--parents", &format!("{old}..{new}")], &[])?.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let r = match p.as_slice() {
            [c, first] if merges == Level::Epic => verify(repo, cfg, Level::Commit, c, first)?,
            // On an epic branch: a commit already on main landed through a gated epic merge (B23).
            [c, _] if is_ancestor(repo, c, main) => continue,
            [c, first] => {
                let level = plain_seam(repo, cfg, c, first, main, *c == tip)?;
                verify(repo, cfg, level, c, if level == Level::Close { main } else { first })?
            }
            // Already landed on the remote branch, or a back-merge of main into an epic: not a seam.
            [_, main, wu]
                if is_ancestor(repo, wu, main) || is_ancestor(repo, wu, old) || (merges == Level::Wu && is_ancestor(repo, wu, &cfg.branches.main)) =>
            {
                continue
            }
            [_, main, wu] => {
                let mut r = verify(repo, cfg, merges, wu, main)?;
                if let Err((what, fix)) = mr::manual_review(repo, cfg, merges.name(), &r.tip, &r.base, main, r.tip_cr.as_ref()) {
                    r.fail("8·manual-review", what, fix);
                }
                r
            }
            [m, ..] if p.len() > 3 => {
                eprintln!("push refused - {} is an octopus merge; parents beyond the second were never gated", short(m));
                red = true;
                continue;
            }
            _ => continue,
        };
        if !r.failures.is_empty() {
            print(&r, "verify-dod");
            red = true;
        }
    }
    Ok(red as i32)
}

/// Which range a plain commit's test and doc notes were written for. Every note names its range's
/// base: the epic's when it equals where the branch left main and that is not the commit's own parent
/// (there the two ranges are one commit, judged as a commit).
#[derive(Clone, Copy, PartialEq, Debug)]
enum NoteRange {
    Commit,
    Epic,
}

fn note_range(bases: &[&str], parent: &str, epic_base: &str) -> NoteRange {
    if epic_base != parent && !bases.is_empty() && bases.iter().all(|b| *b == epic_base) {
        NoteRange::Epic
    } else {
        NoteRange::Commit
    }
}

/// B23, B20: epic-range notes judge a plain commit over the epic when it is the pushed tip or changes
/// no code (the archive commit). A code commit inside the range stays a commit (owner, 2026-11-13).
fn judged_as(notes: NoteRange, pushed_tip: bool, changes_code: bool) -> Level {
    if notes == NoteRange::Epic && (pushed_tip || !changes_code) {
        Level::Close
    } else {
        Level::Commit
    }
}

/// The level a plain first-parent commit `c` (parent `parent`) of an epic branch is judged at.
/// `main`: what the epic merges into. A note present but unreadable names no base, so the commit
/// gate judges it and reports why.
fn plain_seam(repo: &Path, cfg: &Config, c: &str, parent: &str, main: &str, pushed_tip: bool) -> Result<Level, String> {
    let epic_base = git(repo, &["merge-base", main, c], &[])?;
    let bases: Vec<String> = ["test", "doc"]
        .iter()
        .filter_map(|kind| notes::raw(repo, kind, c).transpose())
        .map(|raw| {
            let v = raw.ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
            v.and_then(|v| v["base"].as_str().map(String::from)).unwrap_or_default()
        })
        .collect();
    let range = note_range(&bases.iter().map(String::as_str).collect::<Vec<_>>(), parent, &epic_base);
    let changes_code = range == NoteRange::Epic && !pushed_tip && !notes::names(repo, parent, c, &cfg.code_paths)?.is_empty();
    Ok(judged_as(range, pushed_tip, changes_code))
}

/// Before a review starts: right checkout, clean tree, check green, checks 1-2 on the test note, and
/// the review schedule: the round about to run and its scope, or 0·review-cap (UC-REV-3).
fn preflight(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str) -> Result<i32, String> {
    let (mut r, names) = open(repo, cfg, level, tip_ref, target)?;
    guard_hooks(repo, &mut r);
    if r.exempt {
        print(&r, "preflight");
        return Ok(0);
    }
    if git(repo, &["rev-parse", "HEAD"], &[])? != r.tip {
        r.fail("0·tree", format!("the checkout is not on the tip {}", short(&r.tip)), format!("git checkout {tip_ref}, then preflight again"));
    } else if !git(repo, &["status", "--porcelain", "--untracked-files=no"], &[])?.is_empty() {
        r.fail("0·tree", "the working tree has uncommitted changes".into(), "commit or stash them, then preflight again");
    } else if check(repo, cfg)? != 0 {
        r.fail("0·tree", "keel check is red on the tip".into(), "get lint and tests green before the review runs");
    }
    check_tip(repo, cfg, &mut r, &names, false, true)?;
    let schedule =
        review::Schedule::read(repo, &r.base, &r.tip, target, &cfg.code_paths).map_err(|e| (e, "fix the repo state, then preflight again".to_string()));
    match schedule.and_then(|s| s.next()) {
        Ok(next) => {
            let (commands, changed) = match &next.since {
                Some(since) => {
                    let diffs = notes::own_diffs(repo, since, &r.tip)?;
                    (
                        notes::own_diff_commands(&diffs, &cfg.code_paths),
                        notes::changed_text(Ok(notes::diff_names(repo, &diffs, &cfg.code_paths)?), &format!("{since}..{}", r.tip)),
                    )
                }
                None => (String::new(), String::new()),
            };
            println!("{}", next.announce(&r.base, &r.tip, &commands, &changed));
        }
        Err((what, fix)) => r.fail("0·review-cap", what, fix),
    }
    print(&r, "preflight");
    Ok(!r.failures.is_empty() as i32)
}

const USAGE: &str =
    "usage: keel verify-dod <wu|epic> <tip> [--target R] | commit <sha> | hook | range <old> <new>\n       keel preflight <wu|epic> <tip> [--target R]";

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
        ["commit", sha] => run(repo, cfg, Level::Commit, sha, &format!("{sha}^"), false),
        ["hook"] => hook(repo, cfg),
        ["range", old, new] => range(repo, cfg, old, new, new),
        [lvl, tip, rest @ ..] => {
            let level = level_of(lvl)?;
            run(repo, cfg, level, tip, &target_of(cfg, level, tip, rest), true)
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

#[cfg(test)]
mod tests {
    use super::review_verdict as review;
    use super::{judged_as, note_range, Level, NoteRange};
    use crate::notes::CrNote;

    #[test]
    fn notes_are_the_epics_only_when_every_one_names_the_epic_base_and_that_is_not_the_parent() {
        let pool = ["parent", "epic", "other", ""];
        let mut sets: Vec<Vec<&str>> = vec![vec![]];
        sets.extend(pool.iter().map(|b| vec![*b]));
        sets.extend(pool.iter().flat_map(|a| pool.iter().map(move |b| vec![*a, *b])));
        for (parent, epic) in [("parent", "epic"), ("epic", "epic")] {
            for bases in &sets {
                let want = parent != epic && !bases.is_empty() && bases.iter().all(|b| *b == epic);
                let got = note_range(bases, parent, epic);
                assert_eq!(got == NoteRange::Epic, want, "bases {bases:?}, parent {parent}, epic base {epic}");
            }
        }
    }

    #[test]
    fn epic_notes_judge_the_pushed_tip_or_a_commit_without_code_over_the_epic_and_nothing_else() {
        for notes in [NoteRange::Commit, NoteRange::Epic] {
            for tip in [false, true] {
                for code in [false, true] {
                    let want = if notes == NoteRange::Epic && (tip || !code) { Level::Close } else { Level::Commit };
                    assert!(judged_as(notes, tip, code) == want, "{notes:?} tip={tip} code={code}");
                }
            }
        }
    }

    fn cr(verdict: &str, waiver: Option<&str>) -> CrNote {
        let mut j = serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
            "timestamp": 0, "verdict": verdict, "blockers": 0, "importants": 0, "code_diff_hash": "h"});
        if let Some(w) = waiver {
            j["waiver"] = w.into();
        }
        serde_json::from_value(j).unwrap()
    }

    #[test]
    fn check_3_passes_a_pass_and_a_reasoned_waiver_only() {
        assert_eq!(review(&cr("PASS", None)), Ok(None));
        assert_eq!(review(&cr("PASS", Some("stray"))), Ok(None));
        assert_eq!(review(&cr("WAIVED", Some("  owner ruling  "))), Ok(Some("owner ruling".to_string())));
        for (v, w, needle) in [
            ("WAIVED", None, "no reason"),
            ("WAIVED", Some(" \n"), "no reason"),
            ("FAIL", None, "verdict is FAIL"),
            ("FAIL", Some("a reason on a FAIL waives nothing"), "verdict is FAIL"),
            ("CAPPED", Some("unknown verdict"), "verdict is CAPPED"),
            ("WAIVED", Some("--blockers"), "no reason"),
        ] {
            let (what, fix) = review(&cr(v, w)).unwrap_err();
            assert!(what.contains(needle) && !fix.is_empty(), "{v} {w:?}: {what}");
            assert!(!fix.contains("--verdict WAIVED"), "no fix line tells an agent to write a waiver: {fix}");
        }
    }
}
