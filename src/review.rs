//! The review schedule (UC-REV-3): rounds counted from the cr notes on a branch's own first-parent
//! line, the scope each round reviews, which fixes need no new round (a fix-PASS), and the cap.
//! Derived from the notes every time, never typed: a fresh session counts what the branch recorded.
use crate::notes::{self, CrNote, OWNER_WAIVES};
use crate::{git, short};
use std::collections::HashMap;
use std::path::Path;

/// The cap when the target's committed config sets none.
pub(crate) const DEFAULT_MAX_ROUNDS: u32 = 5;
/// Rounds up to this one review the whole range; later rounds only the last fix.
const WHOLE_RANGE_ROUNDS: u32 = 2;
/// From this round on importants are fixed under a fix-PASS, and only a blocker earns another round.
const FIX_IMPORTANTS_FROM: u32 = 4;

/// One cr note on the line: its commit, and the note, or why it cannot be read.
pub(crate) struct Entry {
    pub commit: String,
    pub note: Result<CrNote, String>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Kind {
    /// A review ran: PASS or FAIL, or a waiver that kept a reviewed round of its range.
    Round,
    /// A PASS recorded on a fix without reviewers.
    FixPass,
    /// A waiver with no review under it.
    Other,
}

pub(crate) fn kind(n: &CrNote) -> Kind {
    match n.verdict.as_str() {
        "PASS" if n.fixes().is_some() => Kind::FixPass,
        "PASS" | "FAIL" => Kind::Round,
        "WAIVED" if !n.unreviewed() => Kind::Round,
        _ => Kind::Other,
    }
}

/// The cr notes on the first-parent commits of `base..tip`, newest first, each parsed: one note that
/// git cannot print or that does not parse is an entry of its own, never dropped. The storage read is
/// `notes::cr_line`'s.
pub(crate) fn line(repo: &Path, base: &str, tip: &str) -> Result<Vec<Entry>, String> {
    Ok(notes::cr_line(repo, base, tip)?.into_iter().map(|(commit, body)| Entry { commit, note: body.and_then(|b| parse_note(&b)) }).collect())
}

/// A cr note as the schedule reads it: the schedule's fields are typed on `CrNote`
/// (`notes::ScheduleFields`), so a `fixes` that is not a full commit name makes the note unreadable.
pub(crate) fn parse_note(body: &str) -> Result<CrNote, String> {
    serde_json::from_str(body).map_err(|e| format!("it does not parse as cr/v1 ({e})"))
}

/// `[review] max_rounds` in the target's committed `.keel/config.toml`: the policy a branch is
/// judged by, never one it or the working tree carries. Unset, or a target that holds no config: the
/// default. A target that names no commit, or a config git cannot read, is an error, never the default.
pub(crate) fn max_rounds(repo: &Path, target: &str) -> Result<u32, String> {
    // Resolved first, then peeled: a suffix on `:/<text>` would join the text it searches for.
    let commit = git(repo, &["rev-parse", "--verify", "-q", "--end-of-options", target], &[])
        .and_then(|obj| git(repo, &["rev-parse", "--verify", "-q", &format!("{obj}^{{commit}}")], &[]))
        .map_err(|_| format!("keel: the target {target} names no commit, so its [review] max_rounds cannot be read"))?;
    if git(repo, &["ls-tree", "--name-only", &commit, "--", ".keel/config.toml"], &[])?.is_empty() {
        return Ok(DEFAULT_MAX_ROUNDS);
    }
    max_rounds_in(&git(repo, &["cat-file", "blob", &format!("{commit}:.keel/config.toml")], &[])?)
}

fn max_rounds_in(text: &str) -> Result<u32, String> {
    let v: toml::Value = text.parse().map_err(|e| format!("keel: bad .keel/config.toml on the target: {e}"))?;
    match v.get("review").and_then(|r| r.get("max_rounds")) {
        None => Ok(DEFAULT_MAX_ROUNDS),
        Some(toml::Value::Integer(n)) if *n >= 1 => u32::try_from(*n).map_err(|_| format!("keel: [review] max_rounds = {n} is too large")),
        Some(other) => Err(format!("keel: [review] max_rounds on the target must be a whole number of rounds, 1 or more (it is {other})")),
    }
}

/// The rounds among `earlier` (the line below the tip, newest first), each with its commit. An
/// unreadable note stops the count: nobody can tell what it recorded.
fn rounds(earlier: &[Entry]) -> Result<Vec<(&str, &CrNote)>, (String, String)> {
    let mut out = vec![];
    for e in earlier {
        match &e.note {
            Ok(n) if kind(n) == Kind::Round => out.push((e.commit.as_str(), n)),
            Ok(_) => {}
            Err(why) => {
                return Err((
                    format!("the cr note on {} is unreadable ({why}), so the review rounds on this branch cannot be counted", short(&e.commit)),
                    format!("the owner repairs that note in place (never removes it: rounds are counted from these notes): git notes --ref=cr edit {}", short(&e.commit)),
                ))
            }
        }
    }
    Ok(out)
}

/// The review round about to run on the tip.
#[derive(Debug, PartialEq)]
pub(crate) struct Next {
    pub round: u32,
    pub max: u32,
    /// From round 3: the last reviewed commit; the round reviews only the code changed since.
    pub since: Option<String>,
}

/// A note's `scope` field: `full`, or `since <commit>` from round 3.
fn scope_of(since: Option<&str>) -> String {
    since.map_or("full".to_string(), |s| format!("since {s}"))
}

impl Next {
    /// The note's `scope` field.
    pub(crate) fn scope(&self) -> String {
        scope_of(self.since.as_deref())
    }

    /// Preflight's line: the round and the exact range the reviewers read (full SHAs). From round 3
    /// that is the branch's own changes since the last reviewed commit (`changed`, from
    /// `notes::own_names`), never what a merge brought from its second parent.
    pub(crate) fn announce(&self, base: &str, tip: &str, code_paths: &[String], changed: &[String]) -> String {
        match &self.since {
            None => format!("review round {} of {} - scope: the whole range, {base}..{tip}", self.round, self.max),
            Some(s) => format!(
                "review round {} of {} - scope: the last fix, {s}..{tip}: this branch's own changes, a merge only for what it resolved itself: {}; changed: {} (reviewers may read any file, and report only on these changes)",
                self.round,
                self.max,
                notes::own_diff_command(s, tip, code_paths),
                if changed.is_empty() { "nothing".to_string() } else { changed.join(", ") }
            ),
        }
    }
}

/// What the schedule says follows a recorded round once code changes after it: the one place that
/// decides, so no fix line names a step another rule refuses (code review round 1, B2/B3/I2).
#[derive(Debug, PartialEq)]
pub(crate) enum Then {
    /// Its findings are fixed with no new round: a fix-PASS answering it.
    FixPass,
    /// Fix, then this review round.
    Round(u32),
    /// The last round found a blocker: the cap's WAIVED on that round's own commit.
    Cap,
    /// No round follows and no fix-PASS answers it: only the owner's waiver at sign-off.
    Owner,
}

/// Whether another review round runs after round `round`: within the cap, and from round 5 only after
/// a blocker.
fn round_follows(note: &CrNote, round: u32, max: u32) -> bool {
    round < max && (round < FIX_IMPORTANTS_FROM || note.blockers > 0)
}

/// What follows round `round` (see `Then`). A FAIL with nothing counted (plan open point 4) needs a
/// round like any other failed round, and where none follows, the owner decides.
pub(crate) fn then(note: &CrNote, round: u32, max: u32) -> Then {
    if fixable(note, round, max).is_ok() {
        Then::FixPass
    } else if round_follows(note, round, max) {
        Then::Round(round + 1)
    } else if note.verdict == "FAIL" && note.blockers > 0 && round == max {
        Then::Cap
    } else {
        Then::Owner
    }
}

/// The fix line for a tip past round `round` on `c` when no round follows it: the cap's waiver belongs
/// on that round's own commit, so a later tip has only the owner's.
fn no_round_after(note: &CrNote, round: u32, max: u32, c: &str) -> String {
    match then(note, round, max) {
        Then::Cap => format!(
            "round {round} on {} is the last and found {} blockers: the review stopped. The cap's waiver goes on that commit itself (keel notes write cr --verdict WAIVED, before any later commit); for this tip, {OWNER_WAIVES}",
            short(c),
            note.blockers
        ),
        _ => format!("no round {} follows round {round}; for this tip, {OWNER_WAIVES}", round + 1),
    }
}

/// The round the tip's review would be, from `earlier` (the line below the tip, newest first: the
/// tip's own note never counts, so a re-run on the same commit is the same round). Refused past the
/// cap, and from round 5 unless the round before found a blocker; the fix line is `then`'s.
/// `unchanged(c)`: no code path changed between commit `c` and the tip (fix_pass's carry-over rule,
/// so the two name one way out).
pub(crate) fn next_round(earlier: &[Entry], max: u32, unchanged: &dyn Fn(&str) -> Result<bool, String>) -> Result<Next, (String, String)> {
    let rounds = rounds(earlier)?;
    let Some(&(c, note)) = rounds.first() else { return Ok(Next { round: 1, max, since: None }) };
    let (last, n) = (rounds.len() as u32, rounds.len() as u32 + 1);
    if round_follows(note, last, max) {
        return Ok(Next { round: n, max, since: (n > WHOLE_RANGE_ROUNDS).then(|| c.to_string()) });
    }
    let what = if n > max {
        format!("no review round {n} runs: the cap is {max} rounds, and round {last} ran on {}", short(c))
    } else {
        format!("review round {n} runs only after round {last} found a blocker, and round {last} on {} found none", short(c))
    };
    if then(note, last, max) != Then::FixPass {
        return Err((what, no_round_after(note, last, max, c)));
    }
    let newest_fix = earlier.iter().take_while(|e| e.commit != c).find(|e| e.note.as_ref().is_ok_and(|n| kind(n) == Kind::FixPass));
    let fixed_and_changed = match newest_fix {
        Some(e) if !unchanged(&e.commit).map_err(unknown)? => Some(e),
        _ => None,
    };
    Err(match fixed_and_changed {
        Some(f) => (
            format!("{what}; the fix-PASS on {} already answers it, and code changed since", short(&f.commit)),
            format!("a second fix has no round to go to; for this tip, {OWNER_WAIVES}"),
        ),
        None => (what, format!("fix its findings and record them without a round: keel notes write cr --verdict PASS --fixes {c}   (on the fix commit, with nothing else changed after it)")),
    })
}

/// A tip's schedule, read once: the one place preflight and the cr writer assemble it from.
pub(crate) struct Schedule<'a> {
    /// The cr notes below the tip, newest first (the tip's own note never counts: a re-run on the same
    /// commit is the same round).
    pub earlier: Vec<Entry>,
    /// The target's cap.
    pub max: u32,
    repo: &'a Path,
    tip: &'a str,
    code_paths: &'a [String],
}

impl<'a> Schedule<'a> {
    pub(crate) fn read(repo: &'a Path, base: &str, tip: &'a str, target: &str, code_paths: &'a [String]) -> Result<Self, String> {
        let earlier = line(repo, base, tip)?.into_iter().filter(|e| e.commit != tip).collect();
        Ok(Schedule { earlier, max: max_rounds(repo, target)?, repo, tip, code_paths })
    }

    /// The branch itself changed no code path between commit `c` and the tip (`notes::own_names`: a
    /// back-merge brings no change of this branch's own). One rule for the round and the fix-PASS. An
    /// error is an error, never an answer: the callers refuse on it.
    pub(crate) fn unchanged(&self, c: &str) -> Result<bool, String> {
        Ok(notes::own_names(self.repo, c, self.tip, self.code_paths)?.is_empty())
    }

    /// The round about to run on the tip.
    pub(crate) fn next(&self) -> Result<Next, (String, String)> {
        next_round(&self.earlier, self.max, &|c: &str| self.unchanged(c))
    }
}

/// Whether a round's findings may be fixed with no new round: only nits (a PASS), in any round;
/// importants and no blocker from round 4, or in the last round when the cap comes before round 4.
pub(crate) fn fixable(note: &CrNote, round: u32, max: u32) -> Result<(), String> {
    let from = FIX_IMPORTANTS_FROM.min(max);
    match (note.verdict.as_str(), note.blockers, note.importants) {
        ("PASS", ..) => Ok(()),
        ("FAIL", 0, i) if i > 0 && round >= from => Ok(()),
        ("FAIL", 0, i) if i > 0 => Err(format!("round {round} found {i} importants; before round {from} importants need a new round")),
        ("FAIL", b, _) if b > 0 => Err(format!("round {round} found {b} blockers; a blocker always needs a new round (at the cap, the owner decides)")),
        ("FAIL", ..) => Err(format!("round {round} failed with no finding counted (keel check red?); a fix-PASS answers only counted findings")),
        (v, ..) => Err(format!("round {round} ended {v}; there is nothing a fix-PASS can answer")),
    }
}

/// Refused when keel cannot tell what the branch changed: never read as "changed" or "unchanged".
fn unknown(e: String) -> (String, String) {
    (format!("keel cannot tell what this branch changed since a reviewed commit ({e})"), "repair the repo state (git fsck names what is broken), then run again".to_string())
}

/// What a fix-PASS answers.
#[derive(Debug, PartialEq)]
pub(crate) struct Fix {
    pub round: u32,
    pub importants: u32,
    pub nits: usize,
}

/// A fix-PASS on the tip answering the round on `fixes`: only the latest round below the tip, never
/// over the tip's own note unless that is a fix-PASS (a reviewed note on the tip is the latest round),
/// only findings `fixable` allows, a failed round only after a code fix, and never on top of another
/// fix-PASS when code changed since it (a second fix needs a round). `own`: the tip's own note.
/// `unchanged(c)`: no code path changed between commit `c` and the tip - a later commit with nothing
/// reviewable (the result report) carries the fix-PASS over.
pub(crate) fn fix_pass(own: Option<&CrNote>, earlier: &[Entry], fixes: &str, max: u32, unchanged: impl Fn(&str) -> Result<bool, String>) -> Result<Fix, (String, String)> {
    let rounds = rounds(earlier)?;
    if let Some(n) = own.filter(|n| kind(n) != Kind::FixPass) {
        let what = match kind(n) {
            Kind::Round => format!("the tip {} carries review round {} ({}), the latest round; a fix-PASS never replaces a reviewed note", short(&n.id.tip), rounds.len() + 1, n.verdict),
            _ => format!("the tip {} carries a {} note with no review under it; a fix-PASS never replaces it", short(&n.id.tip), n.verdict),
        };
        return Err((what, "follow the step the writer printed for that note; a fix-PASS goes on a later commit".into()));
    }
    let Some(&(latest, note)) = rounds.first() else {
        return Err(("no review round on this branch below the tip, so a fix-PASS has nothing to answer".into(), "run the review (round 1) on the tip".into()));
    };
    let round = rounds.len() as u32;
    if latest != fixes {
        return Err((
            format!("--fixes names {}, but the latest review round on this branch is round {round} on {}", short(fixes), short(latest)),
            format!("a fix-PASS answers only the latest round: --fixes {latest}"),
        ));
    }
    let newest_fix = earlier.iter().take_while(|e| e.commit != latest).find(|e| e.note.as_ref().is_ok_and(|n| kind(n) == Kind::FixPass));
    let newest_fix = match newest_fix {
        Some(e) if !unchanged(&e.commit).map_err(unknown)? => Some(e),
        _ => None,
    };
    if let Some(e) = newest_fix {
        let what = format!("the fix-PASS on {} already answers round {round}, and code changed since; a second fix needs a new round", short(&e.commit));
        return Err(match then(note, round, max) {
            _ if round_follows(note, round, max) => (what, format!("run review round {} on the tip", round + 1)),
            // From round 4 (or at the cap) no round follows a round without a blocker: nothing is left but the owner.
            Then::FixPass => (format!("{what}, and no round {} follows round {round}", round + 1), format!("write a fix-PASS only on the final fix; for this tip, {OWNER_WAIVES}")),
            _ => (format!("{what}, and no round {} follows round {round}", round + 1), no_round_after(note, round, max, latest)),
        });
    }
    fixable(note, round, max).map_err(|why| match then(note, round, max) {
        Then::Round(n) => (why, format!("fix the findings, then run review round {n} on the fix")),
        _ => (why, no_round_after(note, round, max, latest)),
    })?;
    if note.verdict != "PASS" && unchanged(latest).map_err(unknown)? {
        return Err((
            format!("no code path changed since round {round} on {}, so nothing fixed its findings; a fix-PASS answers a failed round only with a fix", short(latest)),
            format!("fix the findings in code, then on the fix commit: keel notes write cr --verdict PASS --fixes {latest}"),
        ));
    }
    Ok(Fix { round, importants: note.importants, nits: note.extra.get("nits").and_then(|n| n.as_array()).map_or(0, Vec::len) })
}

pub(crate) fn cap_reason(max: u32, blockers: u32) -> String {
    format!("review cap: {max} rounds, {blockers} blockers open")
}

/// The cap's waiver on the tip, with the reason the tool fixes: only over the last round's FAIL with
/// a blocker. `own`: the tip's own note.
pub(crate) fn cap(own: Option<&CrNote>, earlier: &[Entry], max: u32, tip: &str) -> Result<String, (String, String)> {
    let round = rounds(earlier)?.len() as u32 + 1;
    match own {
        Some(n) if kind(n) == Kind::Round && then(n, round, max) == Then::Cap => Ok(cap_reason(max, n.blockers)),
        _ => {
            let on_tip = match own {
                Some(n) if kind(n) == Kind::Round => format!("its review, round {round} of {max}, is {} with {} blockers", n.verdict, n.blockers),
                _ => format!("it carries no review of round {round} of {max}"),
            };
            Err((
                format!("the review cap is not reached on {}: {on_tip}; the cap's waiver stops only a round {max} that found a blocker", short(tip)),
                format!("follow the schedule (keel preflight names the round); a waiver anywhere else is the owner's: {OWNER_WAIVES}"),
            ))
        }
    }
}

/// What the schedule says after a reviewed round is recorded on `tip`: `then`'s step, in words.
pub(crate) fn next_step(note: &CrNote, round: u32, max: u32, tip: &str) -> String {
    let t = short(tip);
    match then(note, round, max) {
        Then::FixPass if note.verdict == "PASS" => format!("done. A later commit that only fixes this round's nits: keel notes write cr --verdict PASS --fixes {t}   (no new round)"),
        Then::FixPass => format!("fix the importants, then on the fix commit: keel notes write cr --verdict PASS --fixes {t}   (no new round)"),
        Then::Round(n) => format!("fix the findings (a correctness bug gets its failing test first), then review round {n} on the fix commit"),
        Then::Cap => format!(
            "review cap reached: round {round} of {max} found {} blockers; the review stops here. On this tip: keel notes write cr --verdict WAIVED   (the tool writes the reason: {}, and keeps this round's findings); the owner decides at sign-off",
            note.blockers,
            cap_reason(max, note.blockers)
        ),
        Then::Owner => format!("no round {} follows; {OWNER_WAIVES}", round + 1),
    }
}

/// The history's tag per entry (newest first, as `line` gives them): `[round n, full]`,
/// `[round n, since <commit>]`, `[fix-PASS of round n on <commit>; no reviewers ran; changed: ...]`,
/// or nothing. Rounds are numbered from the oldest under `rounds`' rule: an unreadable note stops the
/// count, so a round above it shows as not counted. A note that records another round or scope than
/// the line gives it says so. `files` lists what a fix-PASS commit changed.
pub(crate) fn tags(entries: &[Entry], files: impl Fn(&str, &str) -> Result<Vec<String>, String>) -> Vec<String> {
    let mut numbered: HashMap<&str, u32> = HashMap::new();
    let mut previous: Option<&str> = None;
    let mut stopped = false;
    let mut tags: Vec<String> = vec![];
    for e in entries.iter().rev() {
        let tag = match &e.note {
            Err(_) => {
                stopped = true;
                String::new()
            }
            Ok(_) if stopped && e.note.as_ref().is_ok_and(|n| kind(n) == Kind::Round) => " [round not counted: an unreadable note below it]".into(),
            Ok(n) if kind(n) == Kind::Round => {
                let round = numbered.len() as u32 + 1;
                numbered.insert(&e.commit, round);
                let since = previous.filter(|_| round > WHOLE_RANGE_ROUNDS);
                let (scope, recorded_scope) = (scope_of(since.map(short)), scope_of(since));
                previous = Some(&e.commit);
                let (r, s) = (n.schedule.round, n.schedule.scope.as_deref());
                let differs = r.is_some_and(|r| r != round) || s.is_some_and(|s| s != recorded_scope);
                let note_says = match (differs, r, s) {
                    (true, Some(r), Some(s)) => format!("; the note records round {r}, {s}"),
                    (true, Some(r), None) => format!("; the note records round {r}"),
                    (true, None, Some(s)) => format!("; the note records {s}"),
                    _ => String::new(),
                };
                format!(" [round {round}, {scope}{note_says}]")
            }
            Ok(n) if kind(n) == Kind::FixPass => {
                let reviewed = n.fixes().unwrap_or("?");
                let of = match numbered.get(reviewed) {
                    Some(r) => format!("round {r}"),
                    None if stopped => "a round past an unreadable note".to_string(),
                    None => "a round not on this line".to_string(),
                };
                let changed = files(reviewed, &e.commit).map_or_else(|e| format!("unknown ({e})"), |f| f.join(", "));
                format!(" [fix-PASS of {of} on {}; no reviewers ran; changed: {changed}]", short(reviewed))
            }
            _ => String::new(),
        };
        tags.push(tag);
    }
    tags.reverse();
    tags
}

#[cfg(test)]
mod tests {
    use super::{cap, fix_pass, fixable, max_rounds_in, next_round, next_step, parse_note, tags, then, Entry, Fix, Kind, Next, Then};
    use crate::notes::{full_sha, CrNote};
    use serde_json::json;

    fn note(verdict: &str, blockers: u32, importants: u32, extra: serde_json::Value) -> CrNote {
        let mut j = json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c", "timestamp": 0,
            "verdict": verdict, "blockers": blockers, "importants": importants, "code_diff_hash": "h"});
        j.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(j).unwrap()
    }

    fn at(commit: &str, n: CrNote) -> Entry {
        Entry { commit: commit.into(), note: Ok(n) }
    }

    fn fail(commit: &str, blockers: u32, importants: u32) -> Entry {
        at(commit, note("FAIL", blockers, importants, json!({})))
    }

    /// A fix-PASS on `commit` answering `reviewed`. The test names commits briefly, so `fixes` is set
    /// past the parser's full-commit rule (pinned in a_note_whose_fixes_is_no_full_commit_is_unreadable).
    fn fix_of(commit: &str, reviewed: &str) -> Entry {
        let mut n = note("PASS", 0, 0, json!({}));
        n.schedule.fixes = Some(reviewed.into());
        at(commit, n)
    }

    /// `n` FAIL rounds with a blocker, newest first: commits r<n>..r1.
    fn failed_rounds(n: u32) -> Vec<Entry> {
        (1..=n).rev().map(|i| fail(&format!("r{i}"), 1, 0)).collect()
    }

    #[test]
    fn a_round_is_a_review_that_ran_and_nothing_else() {
        assert_eq!(super::kind(&note("PASS", 0, 0, json!({}))), Kind::Round);
        assert_eq!(super::kind(&note("FAIL", 1, 0, json!({}))), Kind::Round);
        assert_eq!(super::kind(&note("WAIVED", 1, 0, json!({"waiver": "cap"}))), Kind::Round, "a waiver kept over its round");
        assert_eq!(super::kind(fix_of("f1", "r1").note.as_ref().unwrap()), Kind::FixPass);
        assert_eq!(super::kind(&note("WAIVED", 0, 0, json!({"waiver": "r", "unreviewed": true}))), Kind::Other);
    }

    #[test]
    fn the_round_counts_the_reviews_below_the_tip_and_rounds_3_on_review_the_last_fix() {
        assert_eq!(next_round(&[], 5, &|_| Ok(false)), Ok(Next { round: 1, max: 5, since: None }));
        let two = vec![fail("r2", 1, 0), fix_of("f1", "r1"), at("w", note("WAIVED", 0, 0, json!({"waiver": "x", "unreviewed": true}))), fail("r1", 0, 1)];
        assert_eq!(next_round(&two, 5, &|_| Ok(false)), Ok(Next { round: 3, max: 5, since: Some("r2".into()) }), "a fix-PASS or an unreviewed waiver is no round");
        assert_eq!(next_round(&two[1..], 5, &|_| Ok(false)).unwrap().since, None, "round 2 reviews the whole range");
        assert_eq!(next_round(&failed_rounds(4), 5, &|_| Ok(false)), Ok(Next { round: 5, max: 5, since: Some("r4".into()) }));
        let n = Next { round: 3, max: 5, since: Some("s".into()) };
        assert_eq!((n.scope(), Next { round: 1, max: 5, since: None }.scope()), ("since s".to_string(), "full".to_string()));
        let paths = ["src/".to_string(), "tests/".to_string()];
        let a = n.announce("b", "t", &paths, &["src/a.rs".into()]);
        assert!(a.starts_with("review round 3 of 5 - scope: the last fix, s..t: this branch's own changes"), "{a}");
        assert!(a.contains("git rev-list --first-parent --end-of-options s..t | git diff-tree --stdin -r -p --cc --no-renames --src-prefix=a/ --dst-prefix=b/ -- src/ tests/; changed: src/a.rs ("), "{a}");
        assert!(n.announce("b", "t", &paths, &[]).contains("changed: nothing"));
        assert_eq!(Next { round: 2, max: 5, since: None }.announce("b", "t", &paths, &[]), "review round 2 of 5 - scope: the whole range, b..t");
    }

    #[test]
    fn round_5_follows_only_a_blocker_and_nothing_follows_the_cap() {
        let mut no_blocker = failed_rounds(4);
        no_blocker[0] = fail("r4", 0, 2);
        let (what, fix) = next_round(&no_blocker, 5, &|_| Ok(false)).unwrap_err();
        assert!(what.contains("round 5 runs only after round 4 found a blocker") && fix.contains("--fixes r4"), "{what} / {fix}");
        let mut fixed = vec![fix_of("f4", "r4")];
        fixed.extend(no_blocker);
        let (what, fix) = next_round(&fixed, 5, &|_| Ok(false)).unwrap_err();
        assert!(what.contains("fix-PASS on f4 already answers it") && fix.contains("--waive-review") && !fix.contains("--fixes"), "a second fix: {what} / {fix}");
        let (_, fix) = next_round(&fixed, 5, &|c| Ok(c == "f4")).unwrap_err();
        assert!(fix.contains("--fixes r4"), "no code since the fix-PASS: it carries over: {fix}");
        let (what, fix) = next_round(&failed_rounds(5), 5, &|_| Ok(false)).unwrap_err();
        assert!(what.contains("no review round 6") && fix.contains("--verdict WAIVED") && fix.contains("--waive-review"), "{what} / {fix}");
        assert_eq!(next_round(&failed_rounds(5), 6, &|_| Ok(false)).unwrap().round, 6, "max_rounds moves the cap");
        let mut six = failed_rounds(5);
        six[0] = fail("r5", 0, 1);
        assert!(next_round(&six, 6, &|_| Ok(false)).is_err(), "from round 4 on, only a blocker earns another round, whatever the cap");
        assert!(next_round(&failed_rounds(2), 2, &|_| Ok(false)).is_err() && next_round(&failed_rounds(1), 2, &|_| Ok(false)).is_ok());
        let unreadable = vec![fail("r2", 1, 0), Entry { commit: "0123456789ab".into(), note: Err("it does not parse".into()) }];
        let (what, fix) = next_round(&unreadable, 5, &|_| Ok(false)).unwrap_err();
        assert!(what.contains("0123456789 is unreadable") && fix.contains("repairs"), "{what}");
    }

    #[test]
    fn a_fix_pass_answers_nits_in_any_round_and_importants_from_round_4() {
        let pass_nits = note("PASS", 0, 0, json!({"nits": [{"finding": "n"}, {"finding": "m"}]}));
        assert_eq!(fixable(&pass_nits, 1, 5), Ok(()));
        for round in 1..=3 {
            assert!(fixable(&note("FAIL", 0, 2, json!({})), round, 5).unwrap_err().contains("importants"), "{round}");
        }
        for round in 4..=5 {
            assert_eq!(fixable(&note("FAIL", 0, 2, json!({})), round, 5), Ok(()), "{round}");
        }
        assert_eq!(fixable(&note("FAIL", 0, 2, json!({})), 3, 3), Ok(()), "the last round when the cap comes before round 4");
        for round in 1..=5 {
            assert!(fixable(&note("FAIL", 1, 0, json!({})), round, 5).unwrap_err().contains("blocker"), "{round}");
        }
        assert!(fixable(&note("FAIL", 0, 0, json!({})), 4, 5).is_err(), "a FAIL with nothing counted needs a round");
        assert!(fixable(&note("WAIVED", 0, 1, json!({"waiver": "w"})), 4, 5).is_err());

        let line = vec![at("r1", pass_nits)];
        let changed = |_: &str| Ok(false);
        assert_eq!(fix_pass(None, &line, "r1", 5, changed), Ok(Fix { round: 1, importants: 0, nits: 2 }));
        let mut four = failed_rounds(4);
        four[0] = fail("r4", 0, 3);
        assert_eq!(fix_pass(None, &four, "r4", 5, changed), Ok(Fix { round: 4, importants: 3, nits: 0 }));
        assert!(fix_pass(None, &four, "r3", 5, changed).unwrap_err().0.contains("latest review round on this branch is round 4 on r4"));
        assert!(fix_pass(None, &[], "r1", 5, changed).is_err(), "nothing to answer");
        let mut stacked = vec![fix_of("f5", "r4"), fix_of("f4", "r4")];
        stacked.extend(four);
        let (what, fix) = fix_pass(None, &stacked, "r4", 5, changed).unwrap_err();
        assert!(what.contains("fix-PASS on f5 already answers round 4, and code changed since") && what.contains("no round 5 follows"), "{what}");
        assert!(fix.contains("--waive-review") && !fix.contains("run review round"), "no round 5 after a round 4 without a blocker: {fix}");
        let two = vec![fix_of("f2", "r2"), fix_of("f1", "r2"), at("r2", note("PASS", 0, 0, json!({}))), fail("r1", 1, 0)];
        let (_, fix) = fix_pass(None, &two, "r2", 5, changed).unwrap_err();
        assert_eq!(fix, "run review round 3 on the tip", "before round 4 a second fix gets its round");
        assert_eq!(fix_pass(None, &stacked, "r4", 5, |c| Ok(c == "f5")), Ok(Fix { round: 4, importants: 3, nits: 0 }), "no code since the newest fix-PASS: it carries over");
        assert!(fix_pass(None, &stacked, "r4", 5, |c| Ok(c == "f4")).is_err(), "only the newest fix-PASS counts: code changed since it");
        // Epic code review round 2, I2: when keel cannot tell what changed, it refuses, never guessing.
        let broken = |_: &str| -> Result<bool, String> { Err("git failed".into()) };
        let (what, _) = fix_pass(None, &stacked, "r4", 5, broken).unwrap_err();
        assert!(what.contains("cannot tell what this branch changed") && what.contains("git failed"), "{what}");
        let one_failed = vec![fail("r4", 0, 2), fail("r3", 1, 0), fail("r2", 1, 0), fail("r1", 1, 0)];
        assert!(fix_pass(None, &one_failed, "r4", 5, broken).unwrap_err().0.contains("cannot tell"), "a failed round is never answered unproven");
    }

    #[test]
    fn what_follows_a_round_is_decided_in_one_place() {
        let (pass, imp, blk, none) = (note("PASS", 0, 0, json!({})), note("FAIL", 0, 2, json!({})), note("FAIL", 1, 0, json!({})), note("FAIL", 0, 0, json!({})));
        let waived = note("WAIVED", 1, 0, json!({"waiver": "owner"}));
        for (n, round, max, want) in [
            (&pass, 1, 5, Then::FixPass),
            (&pass, 5, 5, Then::FixPass),
            (&imp, 3, 5, Then::Round(4)),
            (&imp, 4, 5, Then::FixPass),
            (&imp, 2, 2, Then::FixPass),
            (&blk, 1, 5, Then::Round(2)),
            (&blk, 4, 5, Then::Round(5)),
            (&blk, 5, 5, Then::Cap),
            (&blk, 5, 6, Then::Round(6)),
            (&none, 2, 5, Then::Round(3)),
            (&none, 4, 5, Then::Owner),
            (&none, 5, 5, Then::Owner),
            (&waived, 2, 5, Then::Round(3)),
            (&waived, 5, 5, Then::Owner),
        ] {
            assert_eq!(then(n, round, max), want, "{} {}/{} round {round} of {max}", n.verdict, n.blockers, n.importants);
        }
    }

    #[test]
    fn a_note_whose_fixes_is_no_full_commit_is_unreadable() {
        let body = |fixes: serde_json::Value| {
            json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c", "timestamp": 0,
                "verdict": "PASS", "blockers": 0, "importants": 0, "code_diff_hash": "h", "fixes": fixes})
            .to_string()
        };
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert!(parse_note(&body(json!(sha))).is_ok());
        assert!(parse_note(&body(json!(sha.repeat(2)[..64]))).is_ok(), "a SHA-256 name");
        for bad in [json!("--output=x"), json!("HEAD~1"), json!(" "), json!(&sha[..39]), json!(sha.to_uppercase())] {
            assert!(parse_note(&body(bad.clone())).unwrap_err().contains("not a full commit name"), "{bad}");
        }
        for bad in [json!(1), json!(null), json!([sha])] {
            assert!(parse_note(&body(bad.clone())).is_err(), "a fixes that is no string is unreadable: {bad}");
        }
        // Epic code review round 1, I2: one typed home. A blank fixes is unreadable for every reader
        // (it was "no fix-PASS" to CrNote::fixes and unreadable to the schedule), and a round that is
        // not a whole number is unreadable too, never ignored.
        assert!(serde_json::from_str::<CrNote>(&body(json!(" "))).is_err());
        let mut round = serde_json::from_str::<serde_json::Value>(&body(json!(sha))).unwrap();
        round["round"] = json!("2");
        assert!(parse_note(&round.to_string()).is_err());
        let n = parse_note(&body(json!(sha))).unwrap();
        assert_eq!((n.fixes(), n.extra.get("fixes")), (Some(sha), None), "the schedule's fields are typed, not left in extra");
        assert_eq!(serde_json::to_value(&n).unwrap()["fixes"], json!(sha), "and written back at the top level");
        assert!(full_sha(sha) && !full_sha(&format!("-{}", &sha[1..])));
    }

    #[test]
    fn no_fix_line_names_a_round_that_cannot_run() {
        // Code review round 1, B2: a round-5 blocker at the cap ends in the cap's waiver, never a round 6.
        let (what, fix) = fix_pass(None, &failed_rounds(5), "r5", 5, |_| Ok(false)).unwrap_err();
        assert!(fix.contains("--verdict WAIVED") && fix.contains("--waive-review") && !fix.contains("round 6"), "{what} / {fix}");
        // B3: a round-4 FAIL with nothing counted: no round 5, no fix-PASS; only the owner.
        let mut nothing = failed_rounds(4);
        nothing[0] = fail("r4", 0, 0);
        let (what, fix) = next_round(&nothing, 5, &|_| Ok(false)).unwrap_err();
        assert!(fix.contains("--waive-review") && !fix.contains("--fixes"), "{what} / {fix}");
        let (what, fix) = fix_pass(None, &nothing, "r4", 5, |_| Ok(false)).unwrap_err();
        assert!(fix.contains("--waive-review") && !fix.contains("review round 5"), "{what} / {fix}");
        assert!(next_step(&note("FAIL", 0, 0, json!({})), 4, 5, "t").contains("--waive-review"));
    }

    #[test]
    fn a_second_fix_and_a_lowered_cap_name_the_step_and_round_that_hold() {
        // Code review round 2, I2: a second fix above a round-5 blocker names the cap's way out, never
        // a fix-PASS that fixable refuses.
        let mut stacked = vec![fix_of("f5", "r5")];
        stacked.extend(failed_rounds(5));
        let (what, fix) = fix_pass(None, &stacked, "r5", 5, |_| Ok(false)).unwrap_err();
        assert!(!fix.contains("write a fix-PASS") && fix.contains("--verdict WAIVED") && fix.contains("--waive-review"), "{what} / {fix}");
        // I3: a target that lowers the cap below the rounds recorded names the last round as it is.
        let (what, _) = next_round(&failed_rounds(5), 3, &|_| Ok(false)).unwrap_err();
        assert!(what.contains("round 5 ran on r5") && !what.contains("round 3 ran on r5"), "{what}");
    }

    #[test]
    fn the_cap_waiver_stops_only_the_last_rounds_blocker_and_names_the_reason() {
        let four = failed_rounds(4);
        assert_eq!(cap(Some(&note("FAIL", 2, 1, json!({}))), &four, 5, "tip"), Ok("review cap: 5 rounds, 2 blockers open".into()));
        assert_eq!(cap(Some(&note("FAIL", 1, 0, json!({}))), &failed_rounds(5), 6, "tip"), Ok("review cap: 6 rounds, 1 blockers open".into()));
        for (own, earlier, needle) in [
            (Some(note("FAIL", 1, 0, json!({}))), failed_rounds(3), "round 4 of 5"),
            (Some(note("FAIL", 0, 2, json!({}))), failed_rounds(4), "round 5 of 5, is FAIL with 0 blockers"),
            (Some(note("PASS", 0, 0, json!({}))), failed_rounds(4), "is PASS"),
            (None, failed_rounds(4), "no review of round 5"),
        ] {
            let (what, fix) = cap(own.as_ref(), &earlier, 5, "0123456789ab").unwrap_err();
            assert!(what.contains("cap is not reached on 0123456789") && what.contains(needle), "{what}");
            assert!(fix.contains("--waive-review") && !fix.contains("--verdict WAIVED"), "never points an agent at a waiver: {fix}");
        }
    }

    #[test]
    fn each_recorded_round_names_the_schedules_next_step() {
        let t = "0123456789ab";
        assert!(next_step(&note("PASS", 0, 0, json!({})), 1, 5, t).contains("--fixes 0123456789"));
        assert!(next_step(&note("FAIL", 0, 1, json!({})), 2, 5, t).contains("review round 3"));
        assert!(next_step(&note("FAIL", 0, 1, json!({})), 4, 5, t).contains("--verdict PASS --fixes"));
        assert!(next_step(&note("FAIL", 1, 0, json!({})), 4, 5, t).contains("review round 5"));
        let capped = next_step(&note("FAIL", 2, 0, json!({})), 5, 5, t);
        assert!(capped.contains("review cap reached") && capped.contains("--verdict WAIVED") && capped.contains("review cap: 5 rounds, 2 blockers open"), "{capped}");
        assert!(next_step(&note("FAIL", 0, 0, json!({})), 5, 5, t).contains("--waive-review"));
    }

    #[test]
    fn the_history_tags_number_rounds_from_the_oldest_and_name_each_fix_pass_with_its_files() {
        let line = vec![
            fix_of("f3", "r3"),
            at("r3", note("PASS", 0, 0, json!({}))),
            Entry { commit: "bad".into(), note: Err("x".into()) },
            fail("r2", 0, 1),
            at("w0", note("WAIVED", 0, 0, json!({"waiver": "w", "unreviewed": true}))),
            fail("r1", 1, 0),
        ];
        let got = tags(&line, |from, to| Ok(vec![format!("{from}->{to}")]));
        // Code review round 2, I1: an unreadable note stops the count, as it stops preflight and the writer.
        assert_eq!(
            got,
            [
                " [fix-PASS of a round past an unreadable note on r3; no reviewers ran; changed: r3->f3]",
                " [round not counted: an unreadable note below it]",
                "",
                " [round 2, full]",
                "",
                " [round 1, full]"
            ]
        );
        // A note that records another round or scope than the line gives it says so.
        let recorded = vec![at("r2", note("FAIL", 1, 0, json!({"round": 3, "scope": "since r0"}))), at("r1", note("FAIL", 1, 0, json!({"round": 1, "scope": "full"})))];
        assert_eq!(tags(&recorded, |_, _| Ok(vec![])), [" [round 2, full; the note records round 3, since r0]", " [round 1, full]"]);
        let orphan = tags(&[fix_of("f", "elsewhere")], |_, _| Err("gone".into()));
        assert_eq!(orphan, [" [fix-PASS of a round not on this line on elsewhere; no reviewers ran; changed: unknown (gone)]"]);
    }

    #[test]
    fn the_cap_comes_from_review_max_rounds_and_defaults_to_5() {
        assert_eq!(max_rounds_in("[check]\nlint = \"x\"\n"), Ok(5));
        assert_eq!(max_rounds_in("[review]\nmodules = [\"src/*\"]\n"), Ok(5));
        assert_eq!(max_rounds_in("[review]\nmax_rounds = 6\n"), Ok(6));
        for bad in ["[review]\nmax_rounds = 0\n", "[review]\nmax_rounds = -1\n", "[review]\nmax_rounds = \"5\"\n", "[review]\nmax_rounds = 99999999999\n", "not toml ["] {
            assert!(max_rounds_in(bad).is_err(), "{bad}");
        }
    }
}
