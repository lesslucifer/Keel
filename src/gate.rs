//! `keel verify-dod` + `keel preflight`: the merge gate (UC-GATE-2/3/5, UC-DOC-3).
//! Ported from Trawler's xtask gate: same check ids, same `FAIL / fix ->` blocks, no cargo.
use crate::notes::{self, CrNote, DocNote, DodNote, TestNote};
use crate::{check, ci, git, hooks_problem, mr, review, short, Config, HOOKS_FIX};
use serde::de::DeserializeOwned;
use std::path::Path;

/// Every check id the gate prints, `<n>·<name>`: the one table. A dod receipt names a check by its
/// name alone (`receipt_name`); the CI page labels its rows with these ids.
pub(crate) const HOOKS: &str = "0·hooks";
const TREE: &str = "0·tree";
const REVIEW_CAP: &str = "0·review-cap";
pub(crate) const NOTES: &str = "1·notes";
pub(crate) const MANIFEST: &str = "2·manifest";
pub(crate) const REVIEW: &str = "3·review";
pub(crate) const REVIEW_BASE: &str = "4·review-base";
pub(crate) const DIFF_BINDING: &str = "5·diff-binding";
pub(crate) const DOC_NOTE: &str = "6·doc-note";
pub(crate) const WALK: &str = "7·walk";
pub(crate) const SIGN_OFF: &str = "8·manual-review";

/// A check id's name, as a dod receipt lists it.
pub(crate) fn receipt_name(id: &'static str) -> &'static str {
    id.split_once('·').map_or(id, |(_, name)| name)
}

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
    /// The ids of the checks this level runs.
    fn checks(self) -> &'static [&'static str] {
        match self {
            Level::Commit | Level::Close => &[NOTES, MANIFEST, DOC_NOTE],
            Level::Wu => &[NOTES, MANIFEST, REVIEW, REVIEW_BASE, DIFF_BINDING, DOC_NOTE, SIGN_OFF],
            Level::Epic => &[NOTES, MANIFEST, REVIEW, REVIEW_BASE, DIFF_BINDING, DOC_NOTE, WALK, SIGN_OFF],
        }
    }

    /// The receipt names of the checks this level runs.
    fn receipt(self) -> Vec<&'static str> {
        self.checks().iter().map(|c| receipt_name(c)).collect()
    }
}

/// One failed check as the gate prints it. `id` is the check it counts against - its own, or the
/// walk's for a seam's failure, whose printed `check` nests the seam's (`7·walk·3·review`).
pub(crate) struct Failure {
    pub id: &'static str,
    pub check: String,
    pub what: String,
    pub fix: String,
}

/// One waiver the gate accepted: the check it waives and the line it prints.
pub(crate) struct Waiver {
    pub check: &'static str,
    pub text: String,
}

/// What a check did on this tip, recorded by the gate as the check ran: it ran (`Ran`), or part of
/// it could not run because its input is missing (`Missing`, with why). A check with no record did
/// not run. The CI page renders this record and nothing it infers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Judged {
    Ran,
    Missing(String),
}

struct Report {
    level: Level,
    tip_ref: String,
    tip: String,
    base: String,
    exempt: bool,
    /// The range changes no code path (UC-GATE-5), as `open` found it; `exempt` also needs no failure.
    range_exempt: bool,
    failures: Vec<Failure>,
    waived: Vec<Waiver>,
    /// Each check's record, in the order the gate made it (`Judged`).
    judged: Vec<(&'static str, Judged)>,
    /// The tip's own review was waived: its reason, for the dod receipt.
    review_waiver: Option<String>,
    /// The tip's cr note as checks 3-5 read it, for check 8's waiver binding (read once).
    tip_cr: Option<CrNote>,
    /// Notes already read from the tip (kind, as `notes::raw` gave it): check 1 reads them from here.
    seen: Vec<(&'static str, Result<Option<String>, String>)>,
}

impl Report {
    fn fail(&mut self, id: &'static str, what: String, fix: impl Into<String>) {
        self.failures.push(Failure { id, check: id.into(), what, fix: fix.into() });
    }

    fn ran(&mut self, id: &'static str) {
        self.judged.push((id, Judged::Ran));
    }

    fn missing(&mut self, id: &'static str, why: &str) {
        self.judged.push((id, Judged::Missing(why.into())));
    }

    fn waive(&mut self, check: &'static str, text: String) {
        self.waived.push(Waiver { check, text });
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
    let r = Report {
        level,
        tip_ref: tip_ref.into(),
        tip,
        base,
        exempt,
        range_exempt: exempt,
        failures: vec![],
        waived: vec![],
        judged: vec![],
        review_waiver: None,
        tip_cr: None,
        seen: vec![],
    };
    Ok((r, names))
}

/// Check 1 for one note: present (when required), JSON, declaring `<kind>/v1`. A required note that
/// does not read records its absence on each check it `feeds` (`Judged::Missing`): the one rule for
/// a missing input. A note not required records nothing when absent.
fn read<T: DeserializeOwned>(repo: &Path, r: &mut Report, kind: &str, required: bool, feeds: &[&'static str]) -> Option<T> {
    let note = read_note(repo, r, kind, required);
    if required && note.is_none() {
        for id in feeds {
            r.missing(id, &format!("no readable {kind} note"));
        }
    }
    note
}

fn read_note<T: DeserializeOwned>(repo: &Path, r: &mut Report, kind: &str, required: bool) -> Option<T> {
    let schema = format!("{kind}/v1");
    r.ran(NOTES);
    let seen = r.seen.iter().find(|(k, _)| *k == kind).map(|(_, raw)| raw.clone());
    let stored = match seen.unwrap_or_else(|| notes::raw(repo, kind, &r.tip)) {
        Ok(stored) => stored,
        Err(e) => {
            r.fail(
                NOTES,
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
            r.fail(NOTES, format!("no {schema} note on {}", short(&r.tip)), fix);
        }
        return None;
    };
    match notes::parse::<T>(&raw, kind) {
        Ok(n) => Some(n),
        Err(e) => {
            r.fail(NOTES, format!("{kind} note on {} is not valid {schema}: {e}", short(&r.tip)), format!("rewrite it with keel notes write {kind}"));
            None
        }
    }
}

/// Checks 1-2 (paperwork, all the preflight reads) or 1-6 on the tip itself. `own_review`: false
/// only when the signer is about to waive the tip's review, so its cr note is neither required nor judged.
fn check_tip(repo: &Path, cfg: &Config, r: &mut Report, names: &[String], full: bool, own_review: bool) -> Result<(), String> {
    let review = full && own_review && matches!(r.level, Level::Wu | Level::Epic);
    let docs = notes::doc_paths(repo, cfg, &r.base, &r.tip, names)?;
    let test = read::<TestNote>(repo, r, "test", true, &[MANIFEST]);
    let cr = if review { read::<CrNote>(repo, r, "cr", true, &[REVIEW, REVIEW_BASE, DIFF_BINDING]) } else { None };
    let doc = if full { read::<DocNote>(repo, r, "doc", r.level != Level::Commit || !docs.is_empty(), &[DOC_NOTE]) } else { None };

    // 2·manifest: the level's layer, and every note bound to this very commit.
    r.ran(MANIFEST);
    if let Some(t) = &test {
        let (layer, have) = match r.level {
            Level::Commit => ("unit", &t.unit),
            Level::Wu => ("integration", &t.integration),
            Level::Epic | Level::Close => ("e2e", &t.e2e),
        };
        if have.is_none() {
            r.fail(
                MANIFEST,
                format!("level {} needs the {layer} layer - the test note has none", r.level.name()),
                format!("add {layer} tests and rewrite the note, or record {{\"{layer}\":{{\"waived\":\"<why>\"}}}}"),
            );
        }
    }
    let tips = [test.as_ref().map(|n| &n.id.tip), cr.as_ref().map(|n| &n.id.tip), doc.as_ref().map(|n| &n.id.tip)];
    if tips.iter().flatten().any(|t| **t != r.tip) {
        r.fail(
            MANIFEST,
            format!("a note on {} names a different tip (copied from another commit?)", short(&r.tip)),
            "rewrite the notes on this tip with keel notes write",
        );
    }

    // 3-5: the review verdict, its base, its binding to the code diff.
    if let Some(cr) = &cr {
        for id in [REVIEW, REVIEW_BASE, DIFF_BINDING] {
            r.ran(id);
        }
        match review_verdict(cr) {
            Ok(None) => {}
            Ok(Some(why)) => {
                r.waive(REVIEW, format!("{REVIEW}: {why}"));
                r.review_waiver = Some(why);
            }
            Err((what, fix)) => r.fail(REVIEW, what, fix),
        }
        if cr.id.base != r.base {
            r.fail(
                REVIEW_BASE,
                format!("review base {} != current merge-base {}", short(&cr.id.base), short(&r.base)),
                "the target moved since the review - re-run the review against the current base",
            );
        }
        if cr.code_diff_hash != notes::code_diff_hash(repo, cfg, &r.base, &r.tip)? {
            r.fail(
                DIFF_BINDING,
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
    r.ran(DOC_NOTE);
    for (what, fix) in notes::doc_problems(repo, cfg, &r.tip, docs, &doc.changes) {
        r.fail(DOC_NOTE, what, fix);
    }
}

/// 6·doc-note at an epic tip (UC-DOC-3): nothing tracked under docs.root outside T0-T2.
fn tracked_layout(repo: &Path, cfg: &Config, r: &mut Report) -> Result<(), String> {
    r.ran(DOC_NOTE);
    let root = cfg.docs.root.trim_end_matches('/');
    for p in notes::zpaths(&git(repo, &["ls-tree", "-r", "-z", "--name-only", &r.tip, "--", root], &[])?) {
        match notes::tier_of(cfg, &p) {
            Some("T3") => r.fail(
                DOC_NOTE,
                format!("{p} is a T3 working doc still tracked at the epic tip"),
                format!("distill it, git rm it (keep the file in {root}/.archive/, gitignored), then keel notes write doc --distill {p}=<T1/T2 doc>"),
            ),
            None => r.fail(
                DOC_NOTE,
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
    let (r, names) = open(repo, cfg, level, tip_ref, target)?;
    judge(repo, cfg, r, &names, target, own_review)
}

fn judge(repo: &Path, cfg: &Config, mut r: Report, names: &[String], target: &str, own_review: bool) -> Result<Report, String> {
    let level = r.level;
    // UC-GATE-5 never exempts W8's archive/tier-layout checks (below). W4's 8·manual-review runs in
    // `run()` instead - this fn is re-entered per walk seam and by mr confirm, neither of which may judge it.
    if !r.exempt {
        check_tip(repo, cfg, &mut r, names, true, own_review)?;
    } else {
        // An archive commit is docs-only by nature; exempting it would leave distillation unchecked.
        let docs = notes::doc_paths(repo, cfg, &r.base, &r.tip, names)?;
        if notes::archives_t3(repo, cfg, &r.tip, &docs) {
            if let Some(doc) = read::<DocNote>(repo, &mut r, "doc", true, &[DOC_NOTE]) {
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
    r.ran(WALK);
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
                    for w in &seam.waived {
                        r.waive(WALK, format!("{WALK} [wu {}] {}", short(m), w.text));
                    }
                }
                (*m, seam.failures, "wu")
            }
            [c, _] if *c == r.tip => continue, // the epic's own checks cover its tip
            [c, first] => {
                let seam = verify_plain(repo, cfg, c, first, target, false)?;
                (*c, seam.failures, seam.level.name())
            }
            [m, ..] if p.len() > 3 => {
                let what = format!("{} is an octopus merge - parents beyond the second were never gated", short(m));
                (*m, vec![Failure { id: WALK, check: String::new(), what, fix: "merge one WU per --no-ff merge commit".into() }], "merge")
            }
            _ => continue,
        };
        if failures.is_empty() {
            continue;
        }
        let why = waivers.iter().find(|w| !w.seam.is_empty() && !w.why.trim().is_empty() && (seam.starts_with(&w.seam) || w.seam.starts_with(seam)));
        if let Some(w) = why {
            r.waive(WALK, format!("{WALK} seam {} waived: {}", short(seam), w.why.trim()));
            continue;
        }
        let hint = format!(
            "; or, if this seam is safe, waive it on the record: git notes --ref=cr edit {} and add \"walk_waivers\":[{{\"seam\":\"{}\",\"why\":\"<why safe>\"}}] - never --no-verify, which skips every gate",
            short(&r.tip),
            short(seam)
        );
        for f in failures {
            let check = if f.check.is_empty() { WALK.to_string() } else { format!("{WALK}·{}", f.check) };
            r.failures.push(Failure { id: WALK, check, what: format!("[{level} {}] {}", short(seam), f.what), fix: format!("{}{hint}", f.fix) });
        }
    }
    Ok(())
}

/// Recorded text made inert for a terminal by the sign-off listing's rule (`mr::printable`), keeping
/// the middle dot of the check ids. `lines`: a fix keeps its own line breaks, each line escaped.
pub(crate) fn inert(s: &str, lines: bool) -> String {
    let one = |l: &str| l.split('·').map(mr::printable).collect::<Vec<_>>().join("·");
    if lines {
        s.split('\n').map(one).collect::<Vec<_>>().join("\n")
    } else {
        one(s)
    }
}

fn print(r: &Report, what: &str) {
    for w in &r.waived {
        println!("WAIVED {}", inert(&w.text, false));
    }
    if r.exempt {
        println!("{what} {} exempt on {} - docs/infra-only, test/cr/doc evidence not required", r.level.name(), short(&r.tip));
    } else if r.failures.is_empty() {
        println!("{what} {} green on {} (checks: {})", r.level.name(), short(&r.tip), r.level.receipt().join(", "));
    } else {
        for f in &r.failures {
            eprintln!("FAIL  {:<24} {}\n      fix -> {}", inert(&f.check, false), inert(&f.what, false), inert(&f.fix, true));
        }
        eprintln!("{what} {} red on {} - {} failed", r.level.name(), short(&r.tip), r.failures.len());
    }
}

/// Guard D (UC-GATE-6), first in the report: a gate command run where git is not running Keel's hooks.
fn guard_hooks(repo: &Path, r: &mut Report) {
    r.ran(HOOKS);
    if let Some(what) = hooks_problem(repo) {
        r.failures.insert(0, Failure { id: HOOKS, check: HOOKS.into(), what, fix: HOOKS_FIX.into() });
        r.exempt = false;
    }
}

/// The gate `verify-dod` runs on a tip, judged and printed: checks 1-7 (`verify`), guard D when
/// `guard` (verify-dod wu|epic by hand; the hook, range and commit gates run from Keel's hooks by
/// construction), and 8·manual-review above the commit level - never exempt (UC-GATE-5), and only at
/// the merge boundary: the walk never re-judges a seam's sign-off against the epic's roster (Trawler
/// DEC-9/10; DEC-48 names the gap). Returns the report and the report the sign-off attested. Its
/// callers differ only in what they do with the verdict: `run` writes the receipt, `ci_gate` does not.
fn gated(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str, guard: bool) -> Result<(Report, Option<(String, String)>), String> {
    let mut r = verify(repo, cfg, level, tip_ref, target)?;
    if guard {
        guard_hooks(repo, &mut r);
    }
    let mut attested = None;
    if level != Level::Commit {
        r.ran(SIGN_OFF);
        match mr::manual_review(repo, cfg, level.name(), &r.tip, &r.base, target, r.tip_cr.as_ref()) {
            Ok(a) => attested = a,
            Err((what, fix)) => r.fail(SIGN_OFF, what, fix),
        }
        r.exempt &= r.failures.is_empty();
    }
    print(&r, "verify-dod");
    Ok((r, attested))
}

/// `verify-dod`: the gate, and on a full green wu/epic the dod/v1 receipt.
fn run(repo: &Path, cfg: &Config, level: Level, tip_ref: &str, target: &str, guard: bool) -> Result<i32, String> {
    let (r, attested) = gated(repo, cfg, level, tip_ref, target, guard)?;
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
            checks: level.receipt().iter().map(|c| c.to_string()).collect(),
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

pub(crate) const CI_USAGE: &str = "usage: keel ci gate <wu|epic> <tip> [--target R] [--summary FILE]";

/// `keel ci gate` (E-GHFLOW.W1): the gate `verify-dod wu|epic` runs, printed by the same code, as a
/// signal for CI. Green when every failure is the sign-off (check 8, judged at the merge); writes no
/// dod receipt; appends the evidence page (`ci::render`) to `--summary FILE`.
pub fn ci_gate(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    let (summary, rest) = summary_flag(args)?;
    let [lvl, tip, rest @ ..] = rest.as_slice() else { return Err(CI_USAGE.into()) };
    let level = level_of(lvl).map_err(|_| CI_USAGE.to_string())?;
    let target = target_of(repo, cfg, level, tip, rest)?;
    let (r, _) = gated(repo, cfg, level, tip, &target, true)?;
    let red = ci::red_before_sign_off(r.failures.iter().map(|f| f.id));
    if let Some(path) = summary {
        let notes: Vec<(&str, Result<Option<String>, String>)> = ci::NOTE_KINDS.iter().map(|k| (*k, notes::raw(repo, k, &r.tip))).collect();
        let page = ci::render(&ci::Evidence {
            level: level.name(),
            tip: &r.tip,
            base: &r.base,
            target: &target,
            exempt: r.range_exempt,
            checks: level.checks(),
            judged: &r.judged,
            failures: &r.failures,
            waived: &r.waived,
            notes: &notes,
        });
        use std::io::Write;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(page.as_bytes()))
            .map_err(|e| format!("keel ci gate: cannot write the summary to {path}: {e}"))?;
    }
    if red {
        eprintln!("ci gate red on {} - a check before the sign-off failed", short(&r.tip));
    } else {
        let range = if r.range_exempt { " (a docs/infra-only range)" } else { "" };
        match r.failures.is_empty() {
            true => println!("ci gate green on {} - every check passes{range}", short(&r.tip)),
            false => println!("ci gate green on {} - every check before {SIGN_OFF} passes{range}; {SIGN_OFF} is judged at the merge", short(&r.tip)),
        }
    }
    Ok(red as i32)
}

/// `--summary FILE` taken out of `args`; the rest kept in order.
fn summary_flag<'a>(args: &[&'a str]) -> Result<(Option<&'a str>, Vec<&'a str>), String> {
    let mut summary = None;
    let mut rest = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "--summary" {
            match (summary, it.next()) {
                (None, Some(path)) if !path.is_empty() => summary = Some(*path),
                _ => return Err(CI_USAGE.into()),
            }
        } else {
            rest.push(*a);
        }
    }
    Ok((summary, rest))
}

/// The range a sign-off signs, once checks 1-7 are green on it.
pub(crate) struct Ready {
    pub tip: String,
    pub base: String,
    pub target: String,
    /// The tip's cr note as checks 3-5 read it; `None` when the signer waives the review (not read).
    pub cr: Option<CrNote>,
}

/// mr confirm's sign-last rule (Trawler DEC-27): checks 1-7 must already be green on the tip.
/// Red -> the failures are printed and `None`; green -> the range to sign and the cr note read.
/// `waive`: the signer waives the tip's own review in this sign-off (UC-REV-2), so its review - the
/// cr note and checks 3-5 on it - is the one thing not yet required; everything else still is.
pub(crate) fn sign_ready(repo: &Path, cfg: &Config, epic: bool, tip_ref: &str, rest: &[&str], waive: bool) -> Result<Option<Ready>, String> {
    let level = if epic { Level::Epic } else { Level::Wu };
    let target = target_of(repo, cfg, level, tip_ref, rest)?;
    let r = verify_with(repo, cfg, level, tip_ref, &target, !waive)?;
    if !r.failures.is_empty() {
        print(&r, "mr confirm");
        return Ok(None);
    }
    Ok(Some(Ready { tip: r.tip, base: r.base, target, cr: r.tip_cr }))
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
    let mut list = vec!["rev-list".to_string(), "--first-parent".into(), "--parents".into(), format!("{old}..{new}")];
    // On an epic branch, what is already on main landed through a gated epic merge (B23): not walked.
    if merges == Level::Wu && git(repo, &["rev-parse", "--verify", "-q", &format!("{main}^{{commit}}")], &[]).is_ok() {
        list.push(format!("^{main}"));
    }
    let mut red = false;
    for line in git(repo, &list.iter().map(String::as_str).collect::<Vec<_>>(), &[])?.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let r = match p.as_slice() {
            [c, first] if merges == Level::Epic => verify(repo, cfg, Level::Commit, c, first)?,
            [c, first] => verify_plain(repo, cfg, c, first, main, *c == tip)?,
            // Already landed on the remote branch, or a back-merge of main into an epic: not a seam.
            [_, first, wu] if is_ancestor(repo, wu, first) || is_ancestor(repo, wu, old) || (merges == Level::Wu && is_ancestor(repo, wu, main)) => continue,
            [_, first, wu] => {
                let mut r = verify(repo, cfg, merges, wu, first)?;
                if let Err((what, fix)) = mr::manual_review(repo, cfg, merges.name(), &r.tip, &r.base, first, r.tip_cr.as_ref()) {
                    r.fail(SIGN_OFF, what, fix);
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

/// Judge a plain first-parent commit `c` (parent `parent`) of an epic branch: over the epic (`close`)
/// or alone (`commit`), by `judged_as`. `main`: what the epic merges into. Its test and doc notes are
/// read once, here, and judged from what was read. A note present but unreadable names no base, so the
/// commit gate judges it and reports why.
fn verify_plain(repo: &Path, cfg: &Config, c: &str, parent: &str, main: &str, pushed_tip: bool) -> Result<Report, String> {
    let epic_base = git(repo, &["merge-base", main, c], &[])?;
    let seen: Vec<(&'static str, Result<Option<String>, String>)> = ["test", "doc"].into_iter().map(|kind| (kind, notes::raw(repo, kind, c))).collect();
    let bases: Vec<String> = seen
        .iter()
        .filter_map(|(_, raw)| raw.as_ref().map(Option::as_deref).transpose())
        .map(|raw| {
            let v = raw.ok().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            v.and_then(|v| v["base"].as_str().map(String::from)).unwrap_or_default()
        })
        .collect();
    let range = note_range(&bases.iter().map(String::as_str).collect::<Vec<_>>(), parent, &epic_base);
    let changes_code = range == NoteRange::Epic && !pushed_tip && !notes::names(repo, parent, c, &cfg.code_paths)?.is_empty();
    let level = judged_as(range, pushed_tip, changes_code);
    let target = if level == Level::Close { main } else { parent };
    let (mut r, names) = open(repo, cfg, level, c, target)?;
    r.seen = seen;
    judge(repo, cfg, r, &names, target, true)
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
        r.fail(TREE, format!("the checkout is not on the tip {}", short(&r.tip)), format!("git checkout {tip_ref}, then preflight again"));
    } else if !git(repo, &["status", "--porcelain", "--untracked-files=no"], &[])?.is_empty() {
        r.fail(TREE, "the working tree has uncommitted changes".into(), "commit or stash them, then preflight again");
    } else if check(repo, cfg)? != 0 {
        r.fail(TREE, "keel check is red on the tip".into(), "get lint and tests green before the review runs");
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
        Err((what, fix)) => r.fail(REVIEW_CAP, what, fix),
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

/// Whether `name` is a local branch, so its name alone decides a WU's target.
fn is_local_branch(repo: &Path, name: &str) -> bool {
    git(repo, &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")], &[]).is_ok()
}

/// The local branches whose history holds `sha`, as their names without `refs/heads/`.
/// `%(refname:short)` disambiguates against tags, so a tag named like a holder turns
/// `epic/e` into `heads/epic/e` and the prefix match below finds nothing (E-SEED3 review
/// round 1, I1); a full refname stripped here cannot be renamed by another ref.
fn holders(repo: &Path, sha: &str) -> Result<Vec<String>, String> {
    let args = ["for-each-ref", "--contains", sha, "--format=%(refname)", "refs/heads"];
    Ok(git(repo, &args, &[])?.lines().map(|l| l.strip_prefix("refs/heads/").unwrap_or(l).to_string()).collect())
}

fn unresolved_target(cfg: &Config, sha: &str, why: &str) -> String {
    format!(
        "keel: cannot tell which branch judges {} - {why}\n  fix -> name it: --target <the branch the WU is measured against>, e.g. --target {}<name>",
        short(sha),
        cfg.branches.epic_prefix
    )
}

/// Epic default target is main; a WU's is inferred from its branch (notes::target). A tip that names
/// no local branch - a bare commit id, `HEAD` on a detached checkout - is resolved from the branches
/// that hold it, or refused: main is never a guess (e-router-v2 R7). The grammar itself lives in
/// `notes`, so the writer and this gate read one rule.
fn target_of(repo: &Path, cfg: &Config, level: Level, tip: &str, rest: &[&str]) -> Result<String, String> {
    if level == Level::Epic || notes::target_flag(rest).is_some() || is_local_branch(repo, tip) {
        return Ok(notes::target(cfg, if level == Level::Epic { "" } else { tip }, rest));
    }
    let sha = git(repo, &["rev-parse", &format!("{tip}^{{commit}}")], &[])?;
    let target = notes::target_from_holders(cfg, &holders(repo, &sha)?).map_err(|why| unresolved_target(cfg, &sha, &why))?;
    if !is_local_branch(repo, &target) {
        return Err(unresolved_target(cfg, &sha, &format!("{target} is not a local branch, so the range cannot be measured")));
    }
    Ok(target)
}

pub fn verify_dod(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    match args {
        ["commit", sha] => run(repo, cfg, Level::Commit, sha, &format!("{sha}^"), false),
        ["hook"] => hook(repo, cfg),
        ["range", old, new] => range(repo, cfg, old, new, new),
        [lvl, tip, rest @ ..] => {
            let level = level_of(lvl)?;
            run(repo, cfg, level, tip, &target_of(repo, cfg, level, tip, rest)?, true)
        }
        _ => Err(USAGE.into()),
    }
}

pub fn preflight_cmd(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    match args {
        [lvl, tip, rest @ ..] => {
            let level = level_of(lvl)?;
            preflight(repo, cfg, level, tip, &target_of(repo, cfg, level, tip, rest)?)
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::inert;
    use super::review_verdict as review;
    use super::{judged_as, note_range, Level, NoteRange};
    use super::{receipt_name, summary_flag, verify, Judged, CI_USAGE, DOC_NOTE, HOOKS, MANIFEST, SIGN_OFF};
    use crate::notes::CrNote;
    use crate::Config;

    /// A repo whose HEAD commit changes code and no doc, judged as a standalone commit with no notes.
    #[test]
    fn a_note_the_gate_does_not_require_is_never_recorded_as_a_missing_input() {
        let d = tempfile::TempDir::new().unwrap();
        let p = d.path();
        let git = |a: &[&str]| {
            let o = crate::no_git_env(std::process::Command::new("git"), false)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(a)
                .current_dir(p)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(o.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&o.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(p.join("README"), "x").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "base"]);
        std::fs::create_dir(p.join("src")).unwrap();
        std::fs::write(p.join("src/a.py"), "a").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "code, no doc"]);
        let cfg: Config =
            toml::from_str("code_paths = [\"src/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n")
                .unwrap();
        let r = verify(p, &cfg, Level::Commit, "HEAD", "HEAD^").unwrap();
        assert!(!r.judged.iter().any(|(id, j)| *id == DOC_NOTE && matches!(j, Judged::Missing(_))), "no doc changed, so no doc note is needed: {:?}", r.judged);
        assert!(r.judged.contains(&(MANIFEST, Judged::Missing("no readable test note".into()))), "a required note still records: {:?}", r.judged);
    }

    #[test]
    fn check_ids_name_their_receipt_in_order() {
        assert_eq!(receipt_name(HOOKS), "hooks");
        assert_eq!(receipt_name(SIGN_OFF), "manual-review");
        assert_eq!(Level::Epic.receipt(), ["notes", "manifest", "review", "review-base", "diff-binding", "doc-note", "walk", "manual-review"]);
        assert_eq!(Level::Wu.receipt(), ["notes", "manifest", "review", "review-base", "diff-binding", "doc-note", "manual-review"]);
        assert_eq!(Level::Commit.receipt(), ["notes", "manifest", "doc-note"]);
        for (i, id) in Level::Epic.checks().iter().enumerate() {
            assert!(id.starts_with(&format!("{}·", i + 1)), "ids are numbered in order: {id}");
        }
    }

    #[test]
    fn the_summary_flag_is_taken_out_once_with_its_path_and_the_rest_keeps_its_order() {
        assert_eq!(summary_flag(&["wu", "t", "--target", "epic/e"]), Ok((None, vec!["wu", "t", "--target", "epic/e"])));
        assert_eq!(summary_flag(&["wu", "--summary", "/s", "t"]), Ok((Some("/s"), vec!["wu", "t"])));
        for bad in [&["wu", "t", "--summary"][..], &["--summary", "", "wu", "t"], &["--summary", "a", "--summary", "b"]] {
            assert_eq!(summary_flag(bad), Err(CI_USAGE.to_string()), "{bad:?}");
        }
    }

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

    /// B17: recorded text reaches the terminal inert; the check ids' middle dot and a fix's own
    /// line breaks stay.
    #[test]
    fn the_printout_escapes_recorded_text_and_keeps_its_own_layout() {
        assert_eq!(inert("WAIVED 3·review: cap", false), "WAIVED 3·review: cap");
        assert_eq!(inert("r\u{1b}[2K\u{7}\u{200b}\\", false), r"r\u{1b}[2K\u{7}\u{200b}\\");
        assert_eq!(inert("a\nFAIL  fake", false), r"a\u{a}FAIL  fake", "a recorded line break cannot start a line");
        assert_eq!(inert("one\n      two\u{1b}", true), "one\n      two\\u{1b}", "a fix keeps its lines, each escaped");
        assert_eq!(inert("7·walk·3·review", false), "7·walk·3·review");
        assert_eq!(inert("café 日本", true), "café 日本");
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
