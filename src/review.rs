//! The review schedule (UC-REV-3): rounds counted from the cr notes on a branch's own first-parent
//! line, the scope each round reviews, which fixes need no new round (a fix-PASS), and the cap.
//! Derived from the notes every time, never typed: a fresh session counts what the branch recorded.
//! Its shape is the target's committed `[review]` settings (`Policy`, DEC-50).
use crate::notes::{self, CrNote, OWNER_WAIVES};
use crate::{git, short};
use std::collections::HashMap;
use std::path::Path;

/// The schedule's shape: the target's committed `[review]` settings, each defaulting to `DEFAULT`'s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Policy {
    /// `max_rounds`, the cap: no round runs past it, and a blocker open in its last round ends in the
    /// cap's waiver.
    pub max_rounds: u32,
    /// `whole_range_rounds`: rounds up to this one review the whole range; later rounds only the
    /// branch's own changes since the last reviewed commit.
    pub whole_range_rounds: u32,
    /// `fix_importants_from`: from this round on importants with no blocker are fixed under a fix-PASS,
    /// and only a blocker earns another round; before it, importants earn the next round.
    pub fix_importants_from: u32,
}

impl Policy {
    /// The owner's schedule (ruling 2026-11-24): round 1 the whole range, round 2 the last fix, round
    /// 3 only after a round-2 blocker, cap 3. E-CRCAP's was 5, 2 and 4.
    pub(crate) const DEFAULT: Policy = Policy { max_rounds: 3, whole_range_rounds: 1, fix_importants_from: 2 };
}

/// The `[review]` keys keel knows: the schedule's three, and the two the keel-code-review skill reads.
const KNOWN_KEYS: [&str; 5] = ["max_rounds", "whole_range_rounds", "fix_importants_from", "modules", "rubric"];

/// How to get past a schedule `policy_in` refuses.
const POLICY_FIX: &str = "correct [review] in .keel/config.toml on the target branch (each key a whole number of rounds, 1 or more, none above max_rounds; a key left out takes its default), then run again";

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
/// `own`: the tip's note when the caller already read it, which stands in for a second read (B16).
pub(crate) fn line(repo: &Path, base: &str, tip: &str, own: Option<CrNote>) -> Result<Vec<Entry>, String> {
    let skip = own.is_some().then_some(tip);
    let below: Vec<Entry> =
        notes::cr_line(repo, base, tip, skip)?.into_iter().map(|(commit, body)| Entry { commit, note: body.and_then(|b| parse_note(&b)) }).collect();
    Ok(match own {
        Some(n) => with_own(below, tip, n),
        None => below,
    })
}

/// The line with the tip's note, already read, first.
fn with_own(mut below: Vec<Entry>, tip: &str, own: CrNote) -> Vec<Entry> {
    below.insert(0, Entry { commit: tip.into(), note: Ok(own) });
    below
}

/// The tip's own entry (its note, or why it cannot be read), apart from the entries below it.
fn split_own(entries: Vec<Entry>, tip: &str) -> (Option<Result<CrNote, String>>, Vec<Entry>) {
    let (own, earlier): (Vec<Entry>, Vec<Entry>) = entries.into_iter().partition(|e| e.commit == tip);
    (own.into_iter().next().map(|e| e.note), earlier)
}

/// A cr note as the schedule reads it: the schedule's fields are typed on `CrNote`
/// (`notes::ScheduleFields`), so a `fixes` that is not a full commit name makes the note unreadable.
pub(crate) fn parse_note(body: &str) -> Result<CrNote, String> {
    serde_json::from_str(body).map_err(|e| format!("it does not parse as cr/v1 ({e})"))
}

/// The schedule in the target's committed `.keel/config.toml` `[review]`: the policy a branch is
/// judged by, never one it or the working tree carries. A target that holds no config, or a key left
/// out: the default. A target that names no commit, a config git cannot read, or a schedule
/// `policy_in` refuses is an error with its fix line, never the default.
pub(crate) fn policy(repo: &Path, target: &str) -> Result<Policy, (String, String)> {
    // Resolved first, then peeled: a suffix on `:/<text>` would join the text it searches for.
    let commit = git(repo, &["rev-parse", "--verify", "-q", "--end-of-options", target], &[])
        .and_then(|obj| git(repo, &["rev-parse", "--verify", "-q", &format!("{obj}^{{commit}}")], &[]))
        .ok();
    policy_at(target, commit.as_deref(), |commit| {
        if git(repo, &["ls-tree", "--name-only", commit, "--", ".keel/config.toml"], &[])?.is_empty() {
            return Ok(None);
        }
        git(repo, &["cat-file", "blob", &format!("{commit}:.keel/config.toml")], &[]).map(Some)
    })
}

/// `policy`'s rule on what git resolved: `commit`, the target's commit (`None`: it names none, an
/// error), and `config`, the `.keel/config.toml` that commit holds (`None`: none, the default).
fn policy_at(target: &str, commit: Option<&str>, config: impl FnOnce(&str) -> Result<Option<String>, String>) -> Result<Policy, (String, String)> {
    let commit = commit.ok_or_else(|| {
        (
            format!("keel: the target {target} names no commit, so its [review] settings cannot be read"),
            "give --target a branch or commit that exists".to_string(),
        )
    })?;
    match config(commit).map_err(|e| (e, "fix the repo state (git fsck names what is broken), then run again".to_string()))? {
        None => Ok(Policy::DEFAULT),
        Some(text) => policy_in(&text).map_err(|why| (format!("the review schedule on the target {target} is refused: {why}"), POLICY_FIX.to_string())),
    }
}

/// The schedule a `.keel/config.toml` sets, or why it is refused (the reason only). One rule for the
/// target's committed file and a staged one (`main::staged_config_refusal`): each schedule key, when
/// set, a whole number of rounds, 1 or more, that fits u32; an explicit `whole_range_rounds` or
/// `fix_importants_from` not above `max_rounds`; no `[review]` key keel does not know. A default above
/// a lowered `max_rounds` is lowered to it, so a cap set alone below a default still works.
pub(crate) fn policy_in(text: &str) -> Result<Policy, String> {
    let v: toml::Value = text.parse().map_err(|e| format!(".keel/config.toml does not parse: {e}"))?;
    let Some(review) = v.get("review") else { return Ok(Policy::DEFAULT) };
    let t = review.as_table().ok_or_else(|| format!("review = {review} must be a table, [review]"))?;
    if let Some(k) = t.keys().find(|k| !KNOWN_KEYS.contains(&k.as_str())) {
        return Err(format!("[review] {k} is not a key keel knows (known: {})", KNOWN_KEYS.join(", ")));
    }
    let key = |k: &str| match t.get(k) {
        None => Ok(None),
        Some(toml::Value::Integer(n)) if *n >= 1 => u32::try_from(*n).map(Some).map_err(|_| format!("[review] {k} = {n} is too large")),
        Some(other) => Err(format!("[review] {k} = {other} must be a whole number of rounds, 1 or more")),
    };
    let max = key("max_rounds")?.unwrap_or(Policy::DEFAULT.max_rounds);
    let under_cap = |k: &str, default: u32| match key(k)? {
        Some(n) if n > max => Err(format!("[review] {k} = {n} is above max_rounds = {max}: no round runs past the cap")),
        Some(n) => Ok(n),
        None => Ok(default.min(max)),
    };
    Ok(Policy {
        max_rounds: max,
        whole_range_rounds: under_cap("whole_range_rounds", Policy::DEFAULT.whole_range_rounds)?,
        fix_importants_from: under_cap("fix_importants_from", Policy::DEFAULT.fix_importants_from)?,
    })
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
                    format!(
                        "the owner repairs that note in place (never removes it: rounds are counted from these notes): git notes --ref=cr edit {}",
                        short(&e.commit)
                    ),
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
    /// Past `whole_range_rounds`: the last reviewed commit; the round reviews only the code changed since.
    pub since: Option<String>,
}

/// A note's `scope` field: `full`, or `since <commit>` past `whole_range_rounds`.
fn scope_of(since: Option<&str>) -> String {
    since.map_or("full".to_string(), |s| format!("since {s}"))
}

impl Next {
    /// The note's `scope` field.
    pub(crate) fn scope(&self) -> String {
        scope_of(self.since.as_deref())
    }

    /// Preflight's line: the round and the exact range the reviewers read (full SHAs). Past
    /// `whole_range_rounds` that is the branch's own changes since the last reviewed commit: `commands` prints them
    /// (`notes::own_diff_commands`), and `changed` is the ready text of their files
    /// (`notes::changed_text` over `notes::diff_names`, one argv for both).
    pub(crate) fn announce(&self, base: &str, tip: &str, commands: &str, changed: &str) -> String {
        match &self.since {
            None => format!("review round {} of {} - scope: the whole range, {base}..{tip}", self.round, self.max),
            Some(s) => format!(
                "review round {} of {} - scope: the last fix, {s}..{tip}: this branch's own changes, each commit against its first parent and a merge against git's automatic merge of its parents: {commands}; changed: {changed} (reviewers may read any file, and report only on these changes)",
                self.round,
                self.max,
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

/// Whether another review round runs after round `round`: within the cap, and after a round from
/// `fix_importants_from` on only when it found a blocker.
fn round_follows(note: &CrNote, round: u32, p: &Policy) -> bool {
    round < p.max_rounds && (round < p.fix_importants_from || note.blockers > 0)
}

/// What follows round `round` (see `Then`). A FAIL with nothing counted (plan open point 4) needs a
/// round like any other failed round, and where none follows, the owner decides.
pub(crate) fn then(note: &CrNote, round: u32, p: &Policy) -> Then {
    if fixable(note, round, p).is_ok() {
        Then::FixPass
    } else if round_follows(note, round, p) {
        Then::Round(round + 1)
    } else if note.verdict == "FAIL" && note.blockers > 0 && round == p.max_rounds {
        Then::Cap
    } else {
        Then::Owner
    }
}

/// The fix line for a tip past round `round` on `c` when no round follows it: the cap's waiver belongs
/// on that round's own commit, so a later tip has only the owner's.
fn no_round_after(note: &CrNote, round: u32, p: &Policy, c: &str) -> String {
    match then(note, round, p) {
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
/// cap, and after a round from `fix_importants_from` on unless it found a blocker; the fix line is
/// `then`'s.
/// `unchanged(c)`: no code path changed between commit `c` and the tip (fix_pass's carry-over rule,
/// so the two name one way out).
pub(crate) fn next_round(earlier: &[Entry], p: &Policy, unchanged: &dyn Fn(&str) -> Result<bool, String>) -> Result<Next, (String, String)> {
    let rounds = rounds(earlier)?;
    let max = p.max_rounds;
    let Some(&(c, note)) = rounds.first() else { return Ok(Next { round: 1, max, since: None }) };
    let (last, n) = (rounds.len() as u32, rounds.len() as u32 + 1);
    if round_follows(note, last, p) {
        return Ok(Next { round: n, max, since: (n > p.whole_range_rounds).then(|| c.to_string()) });
    }
    let what = if n > max {
        format!("no review round {n} runs: the cap is {max} rounds, and round {last} ran on {}", short(c))
    } else {
        format!("review round {n} runs only after round {last} found a blocker, and round {last} on {} found none", short(c))
    };
    if then(note, last, p) != Then::FixPass {
        return Err((what, no_round_after(note, last, p, c)));
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
    /// The tip's own note, read with the line (B16): `None` when there is none, `Err` when unreadable.
    pub own: Option<Result<CrNote, String>>,
    /// The cr notes below the tip, newest first (the tip's own note never counts: a re-run on the same
    /// commit is the same round).
    pub earlier: Vec<Entry>,
    /// The target's schedule.
    pub policy: Policy,
    repo: &'a Path,
    tip: &'a str,
    code_paths: &'a [String],
}

impl<'a> Schedule<'a> {
    /// Refused, with its fix line, when the line cannot be read or the target's schedule is refused.
    pub(crate) fn read(repo: &'a Path, base: &str, tip: &'a str, target: &str, code_paths: &'a [String]) -> Result<Self, (String, String)> {
        let line = line(repo, base, tip, None).map_err(|e| (e, "fix the repo state, then run again".to_string()))?;
        let (own, earlier) = split_own(line, tip);
        Ok(Schedule { own, earlier, policy: policy(repo, target)?, repo, tip, code_paths })
    }

    /// The branch itself changed no code path between commit `c` and the tip (`notes::own_names`: a
    /// back-merge brings no change of this branch's own). One rule for the round and the fix-PASS. An
    /// error is an error, never an answer: the callers refuse on it.
    pub(crate) fn unchanged(&self, c: &str) -> Result<bool, String> {
        Ok(notes::own_names(self.repo, c, self.tip, self.code_paths)?.is_empty())
    }

    /// The round about to run on the tip.
    pub(crate) fn next(&self) -> Result<Next, (String, String)> {
        next_round(&self.earlier, &self.policy, &|c: &str| self.unchanged(c))
    }
}

/// Whether a round's findings may be fixed with no new round: only nits (a PASS), in any round;
/// importants and no blocker from `fix_importants_from` (never later than the cap's round).
pub(crate) fn fixable(note: &CrNote, round: u32, p: &Policy) -> Result<(), String> {
    let from = p.fix_importants_from.min(p.max_rounds);
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
    (
        format!("keel cannot tell what this branch changed since a reviewed commit ({e})"),
        "repair the repo state (git fsck names what is broken), then run again".to_string(),
    )
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
pub(crate) fn fix_pass(
    own: Option<&CrNote>,
    earlier: &[Entry],
    fixes: &str,
    p: &Policy,
    unchanged: impl Fn(&str) -> Result<bool, String>,
) -> Result<Fix, (String, String)> {
    let rounds = rounds(earlier)?;
    if let Some(n) = own.filter(|n| kind(n) != Kind::FixPass) {
        let what = match kind(n) {
            Kind::Round => format!(
                "the tip {} carries review round {} ({}), the latest round; a fix-PASS never replaces a reviewed note",
                short(&n.id.tip),
                rounds.len() + 1,
                n.verdict
            ),
            _ if n.waiver().is_some() => {
                return Err((
                    format!(
                        "the tip {} carries the owner's waiver, with no review under it; a fix-PASS answers a reviewed round and never replaces a waiver",
                        short(&n.id.tip)
                    ),
                    format!("put the fix on a later commit and review it as keel preflight names; or {OWNER_WAIVES}"),
                ))
            }
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
        return Err(match then(note, round, p) {
            _ if round_follows(note, round, p) => (what, format!("run review round {} on the tip", round + 1)),
            // From fix_importants_from (or at the cap) no round follows a round without a blocker: nothing is left but the owner.
            Then::FixPass => (
                format!("{what}, and no round {} follows round {round}", round + 1),
                format!("write a fix-PASS only on the final fix; for this tip, {OWNER_WAIVES}"),
            ),
            _ => (format!("{what}, and no round {} follows round {round}", round + 1), no_round_after(note, round, p, latest)),
        });
    }
    fixable(note, round, p).map_err(|why| match then(note, round, p) {
        Then::Round(n) => (why, format!("fix the findings, then run review round {n} on the fix")),
        _ => (why, no_round_after(note, round, p, latest)),
    })?;
    if note.verdict != "PASS" && unchanged(latest).map_err(unknown)? {
        return Err((
            format!(
                "no code path changed since round {round} on {}, so nothing fixed its findings; a fix-PASS answers a failed round only with a fix",
                short(latest)
            ),
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
pub(crate) fn cap(own: Option<&CrNote>, earlier: &[Entry], p: &Policy, tip: &str) -> Result<String, (String, String)> {
    let round = rounds(earlier)?.len() as u32 + 1;
    let max = p.max_rounds;
    match own {
        Some(n) if kind(n) == Kind::Round && then(n, round, p) == Then::Cap => Ok(cap_reason(max, n.blockers)),
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
pub(crate) fn next_step(note: &CrNote, round: u32, p: &Policy, tip: &str) -> String {
    let t = short(tip);
    let max = p.max_rounds;
    match then(note, round, p) {
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
/// count, so a round above it shows as not counted; a round's scope is `p`'s (the target's
/// `whole_range_rounds`). A note that records another round or scope than the line gives it says so. `files` lists what a fix-PASS commit changed, as `notes::diff_names` does;
/// `notes::changed_text` renders it, as for preflight.
pub(crate) fn tags(entries: &[Entry], p: &Policy, files: impl Fn(&str, &str) -> Result<Vec<Vec<u8>>, String>) -> Vec<String> {
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
                let since = previous.filter(|_| round > p.whole_range_rounds);
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
                let round = numbered.get(reviewed);
                let of = match round {
                    Some(r) => format!("round {r}"),
                    None if stopped => "a round past an unreadable note".to_string(),
                    None => "a round not on this line".to_string(),
                };
                let note_says = match (n.schedule.round, round) {
                    (Some(r), Some(line)) if r != *line => format!("; the note records round {r}"),
                    _ => String::new(),
                };
                let changed = notes::changed_text(files(reviewed, &e.commit), &format!("{}..{}", short(reviewed), short(&e.commit)));
                format!(" [fix-PASS of {of} on {}{note_says}; no reviewers ran; changed: {changed}]", short(reviewed))
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
    use super::{
        cap, fix_pass, fixable, next_round, next_step, parse_note, policy_at, policy_in, split_own, tags, then, with_own, Entry, Fix, Kind, Next, Policy, Then,
    };
    use crate::notes::{full_sha, CrNote};
    use serde_json::json;

    /// The defaults: cap 3, round 1 the whole range, importants fixed under a fix-PASS from round 2.
    const D: Policy = Policy::DEFAULT;
    /// E-CRCAP's schedule, as the three settings restore it.
    const OLD: Policy = Policy { max_rounds: 5, whole_range_rounds: 2, fix_importants_from: 4 };
    /// A configured schedule: cap 4, two whole-range rounds, importants earn a round up to round 3.
    const SET: Policy = Policy { max_rounds: 4, whole_range_rounds: 2, fix_importants_from: 3 };

    fn cap_of(max_rounds: u32, p: Policy) -> Policy {
        Policy { max_rounds, ..p }
    }

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

    fn never(_: &str) -> Result<bool, String> {
        Ok(false)
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
    fn the_round_counts_the_reviews_below_the_tip_and_rounds_past_whole_range_rounds_review_the_last_fix() {
        assert_eq!(next_round(&[], &D, &never), Ok(Next { round: 1, max: 3, since: None }));
        let one = vec![fix_of("f1", "r1"), at("w", note("WAIVED", 0, 0, json!({"waiver": "x", "unreviewed": true}))), fail("r1", 0, 1)];
        assert_eq!(
            next_round(&one, &D, &never),
            Ok(Next { round: 2, max: 3, since: Some("r1".into()) }),
            "a fix-PASS or an unreviewed waiver is no round; round 2 reviews the last fix"
        );
        assert_eq!(next_round(&one, &SET, &never).unwrap().since, None, "under whole_range_rounds = 2, round 2 reviews the whole range");
        let two = vec![fail("r2", 1, 0), fix_of("f1", "r1"), fail("r1", 0, 1)];
        assert_eq!(next_round(&two, &D, &never), Ok(Next { round: 3, max: 3, since: Some("r2".into()) }), "a round-2 blocker earns round 3");
        assert_eq!(next_round(&two, &OLD, &never), Ok(Next { round: 3, max: 5, since: Some("r2".into()) }));
        assert_eq!(next_round(&failed_rounds(4), &OLD, &never), Ok(Next { round: 5, max: 5, since: Some("r4".into()) }));
        let n = Next { round: 2, max: 3, since: Some("s".into()) };
        assert_eq!((n.scope(), Next { round: 1, max: 3, since: None }.scope()), ("since s".to_string(), "full".to_string()));
        let a = n.announce("b", "t", "git diff-tree X", "src/a.rs");
        assert!(a.starts_with("review round 2 of 3 - scope: the last fix, s..t: this branch's own changes"), "{a}");
        assert!(a.contains("its parents: git diff-tree X; changed: src/a.rs ("), "{a}");
        assert_eq!(Next { round: 1, max: 3, since: None }.announce("b", "t", "", ""), "review round 1 of 3 - scope: the whole range, b..t");
    }

    #[test]
    fn round_3_follows_only_a_round_2_blocker_and_nothing_follows_the_cap() {
        let no_blocker = vec![fail("r2", 0, 2), fail("r1", 1, 0)];
        let (what, fix) = next_round(&no_blocker, &D, &never).unwrap_err();
        assert!(
            what.contains("review round 3 runs only after round 2 found a blocker, and round 2 on r2 found none") && fix.contains("--fixes r2"),
            "{what} / {fix}"
        );
        let mut fixed = vec![fix_of("f2", "r2")];
        fixed.extend(no_blocker);
        let (what, fix) = next_round(&fixed, &D, &never).unwrap_err();
        assert!(
            what.contains("fix-PASS on f2 already answers it") && fix.contains("--waive-review") && !fix.contains("--fixes"),
            "a second fix: {what} / {fix}"
        );
        let (_, fix) = next_round(&fixed, &D, &|c| Ok(c == "f2")).unwrap_err();
        assert!(fix.contains("--fixes r2"), "no code since the fix-PASS: it carries over: {fix}");
        let (what, fix) = next_round(&failed_rounds(3), &D, &never).unwrap_err();
        assert!(
            what.contains("no review round 4 runs: the cap is 3 rounds") && fix.contains("--verdict WAIVED") && fix.contains("--waive-review"),
            "{what} / {fix}"
        );
        assert_eq!(next_round(&failed_rounds(3), &cap_of(4, D), &never).unwrap().round, 4, "max_rounds moves the cap");
        let mut four = failed_rounds(3);
        four[0] = fail("r3", 0, 1);
        assert!(next_round(&four, &cap_of(4, D), &never).is_err(), "from fix_importants_from on, only a blocker earns another round, whatever the cap");
        assert!(next_round(&failed_rounds(2), &cap_of(2, D), &never).is_err() && next_round(&failed_rounds(1), &cap_of(2, D), &never).is_ok());
        let unreadable = vec![fail("r2", 1, 0), Entry { commit: "0123456789ab".into(), note: Err("it does not parse".into()) }];
        let (what, fix) = next_round(&unreadable, &D, &never).unwrap_err();
        assert!(what.contains("0123456789 is unreadable") && fix.contains("repairs"), "{what}");
    }

    #[test]
    fn a_configured_schedule_and_the_old_one_follow_their_own_settings() {
        // SET: round-2 importants earn round 3; round-3 importants earn no round 4, a blocker does.
        let r2 = vec![fail("r2", 0, 1), fail("r1", 0, 1)];
        assert_eq!(next_round(&r2, &SET, &never), Ok(Next { round: 3, max: 4, since: Some("r2".into()) }));
        assert!(next_round(&r2, &D, &never).is_err(), "on the defaults the same round 2 earns no round 3");
        let (what, fix) = next_round(&[fail("r3", 0, 1), fail("r2", 0, 1), fail("r1", 0, 1)], &SET, &never).unwrap_err();
        assert!(what.contains("review round 4 runs only after round 3 found a blocker") && fix.contains("--fixes r3"), "{what} / {fix}");
        assert_eq!(next_round(&[fail("r3", 1, 0), fail("r2", 0, 1), fail("r1", 0, 1)], &SET, &never).unwrap().round, 4);
        // OLD: rounds 1-2 the whole range, importants earn rounds up to round 4, round 5 only after a blocker.
        assert_eq!(next_round(&[fail("r1", 0, 1)], &OLD, &never).unwrap().since, None);
        let mut four = failed_rounds(4);
        four[0] = fail("r4", 0, 2);
        let (what, fix) = next_round(&four, &OLD, &never).unwrap_err();
        assert!(what.contains("round 5 runs only after round 4 found a blocker") && fix.contains("--fixes r4"), "{what} / {fix}");
        assert_eq!(next_round(&failed_rounds(5), &cap_of(6, OLD), &never).unwrap().round, 6);
        let mut six = failed_rounds(5);
        six[0] = fail("r5", 0, 1);
        assert!(next_round(&six, &cap_of(6, OLD), &never).is_err());
    }

    /// Invariant: every schedule whose rounds fit its cap is accepted as written, every other one is
    /// refused, and none lets a round run past its cap, read the whole range past whole_range_rounds,
    /// or follow a round from fix_importants_from on that found no blocker.
    #[test]
    fn every_schedule_is_accepted_exactly_when_its_rounds_fit_the_cap_and_no_round_breaks_it() {
        for m in 1..=6u32 {
            for w in 1..=7u32 {
                for f in 1..=7u32 {
                    let text = format!("[review]\nmax_rounds = {m}\nwhole_range_rounds = {w}\nfix_importants_from = {f}\n");
                    let Ok(p) = policy_in(&text) else {
                        assert!(w > m || f > m, "{text}");
                        continue;
                    };
                    assert!(w <= m && f <= m, "{text}");
                    assert_eq!(p, Policy { max_rounds: m, whole_range_rounds: w, fix_importants_from: f });
                    for done in 0..=m + 1 {
                        for (blockers, importants) in [(0, 0), (0, 1), (1, 0)] {
                            let mut line = failed_rounds(done);
                            if let Some(top) = line.first_mut() {
                                *top = fail(&format!("r{done}"), blockers, importants);
                            }
                            if let Ok(n) = next_round(&line, &p, &never) {
                                assert!(n.round == done + 1 && n.round <= m, "{text} after {done}");
                                assert_eq!(n.since.is_some(), n.round > w, "{text} round {}", n.round);
                                assert!(done == 0 || done < f || blockers > 0, "{text} after {done}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_fix_pass_answers_nits_in_any_round_and_importants_from_fix_importants_from() {
        let pass_nits = note("PASS", 0, 0, json!({"nits": [{"finding": "n"}, {"finding": "m"}]}));
        assert_eq!(fixable(&pass_nits, 1, &D), Ok(()));
        let imp = note("FAIL", 0, 2, json!({}));
        assert!(fixable(&imp, 1, &D).unwrap_err().contains("before round 2 importants need a new round"));
        for round in 2..=3 {
            assert_eq!(fixable(&imp, round, &D), Ok(()), "{round}");
        }
        for round in 1..=3 {
            assert!(fixable(&imp, round, &OLD).unwrap_err().contains("importants"), "{round}");
        }
        for round in 4..=5 {
            assert_eq!(fixable(&imp, round, &OLD), Ok(()), "{round}");
        }
        assert_eq!(fixable(&imp, 3, &cap_of(3, OLD)), Ok(()), "the last round when the cap comes before fix_importants_from");
        for round in 1..=3 {
            assert!(fixable(&note("FAIL", 1, 0, json!({})), round, &D).unwrap_err().contains("blocker"), "{round}");
        }
        assert!(fixable(&note("FAIL", 0, 0, json!({})), 2, &D).is_err(), "a FAIL with nothing counted needs a round");
        assert!(fixable(&note("WAIVED", 0, 1, json!({"waiver": "w"})), 2, &D).is_err());

        let line = vec![at("r1", pass_nits)];
        assert_eq!(fix_pass(None, &line, "r1", &D, never), Ok(Fix { round: 1, importants: 0, nits: 2 }));
        let two = || vec![fail("r2", 0, 3), fail("r1", 1, 0)];
        assert_eq!(fix_pass(None, &two(), "r2", &D, never), Ok(Fix { round: 2, importants: 3, nits: 0 }));
        assert!(fix_pass(None, &two(), "r1", &D, never).unwrap_err().0.contains("latest review round on this branch is round 2 on r2"));
        assert!(fix_pass(None, &[], "r1", &D, never).is_err(), "nothing to answer");
        let mut stacked = vec![fix_of("f3", "r2"), fix_of("f2", "r2")];
        stacked.extend(two());
        let (what, fix) = fix_pass(None, &stacked, "r2", &D, never).unwrap_err();
        assert!(what.contains("fix-PASS on f3 already answers round 2, and code changed since") && what.contains("no round 3 follows"), "{what}");
        assert!(fix.contains("--waive-review") && !fix.contains("run review round"), "no round 3 after a round 2 without a blocker: {fix}");
        let early = vec![fix_of("f2", "r1"), fix_of("f1", "r1"), at("r1", note("PASS", 0, 0, json!({})))];
        let (_, fix) = fix_pass(None, &early, "r1", &D, never).unwrap_err();
        assert_eq!(fix, "run review round 2 on the tip", "before fix_importants_from a second fix gets its round");
        assert_eq!(
            fix_pass(None, &stacked, "r2", &D, |c| Ok(c == "f3")),
            Ok(Fix { round: 2, importants: 3, nits: 0 }),
            "no code since the newest fix-PASS: it carries over"
        );
        assert!(fix_pass(None, &stacked, "r2", &D, |c| Ok(c == "f2")).is_err(), "only the newest fix-PASS counts: code changed since it");
        // Epic code review round 2, I2: when keel cannot tell what changed, it refuses, never guessing.
        let broken = |_: &str| -> Result<bool, String> { Err("git failed".into()) };
        let (what, _) = fix_pass(None, &stacked, "r2", &D, broken).unwrap_err();
        assert!(what.contains("cannot tell what this branch changed") && what.contains("git failed"), "{what}");
        assert!(fix_pass(None, &two(), "r2", &D, broken).unwrap_err().0.contains("cannot tell"), "a failed round is never answered unproven");
        // OLD: importants are fixed under a fix-PASS only from round 4.
        let mut four = failed_rounds(4);
        four[0] = fail("r4", 0, 3);
        assert_eq!(fix_pass(None, &four, "r4", &OLD, never), Ok(Fix { round: 4, importants: 3, nits: 0 }));
        assert!(fix_pass(None, &[fail("r3", 0, 1), fail("r2", 1, 0), fail("r1", 1, 0)], "r3", &OLD, never).unwrap_err().0.contains("importants"));
    }

    #[test]
    fn what_follows_a_round_is_decided_in_one_place() {
        let (pass, imp, blk, none) =
            (note("PASS", 0, 0, json!({})), note("FAIL", 0, 2, json!({})), note("FAIL", 1, 0, json!({})), note("FAIL", 0, 0, json!({})));
        let waived = note("WAIVED", 1, 0, json!({"waiver": "owner"}));
        for (n, round, p, want) in [
            (&pass, 1, D, Then::FixPass),
            (&pass, 3, D, Then::FixPass),
            (&imp, 1, D, Then::Round(2)),
            (&imp, 2, D, Then::FixPass),
            (&imp, 3, D, Then::FixPass),
            (&blk, 1, D, Then::Round(2)),
            (&blk, 2, D, Then::Round(3)),
            (&blk, 3, D, Then::Cap),
            (&blk, 3, cap_of(4, D), Then::Round(4)),
            (&none, 1, D, Then::Round(2)),
            (&none, 2, D, Then::Owner),
            (&none, 3, D, Then::Owner),
            (&waived, 1, D, Then::Round(2)),
            (&waived, 3, D, Then::Owner),
            (&imp, 2, SET, Then::Round(3)),
            (&imp, 3, SET, Then::FixPass),
            (&blk, 3, SET, Then::Round(4)),
            (&blk, 4, SET, Then::Cap),
            (&imp, 3, OLD, Then::Round(4)),
            (&imp, 4, OLD, Then::FixPass),
            (&imp, 2, cap_of(2, OLD), Then::FixPass),
            (&blk, 4, OLD, Then::Round(5)),
            (&blk, 5, OLD, Then::Cap),
            (&none, 4, OLD, Then::Owner),
        ] {
            assert_eq!(then(n, round, &p), want, "{} {}/{} round {round} on {p:?}", n.verdict, n.blockers, n.importants);
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
        // Code review round 1, B2: a blocker in the last round ends in the cap's waiver, never a round past the cap.
        let (what, fix) = fix_pass(None, &failed_rounds(3), "r3", &D, never).unwrap_err();
        assert!(fix.contains("--verdict WAIVED") && fix.contains("--waive-review") && !fix.contains("round 4"), "{what} / {fix}");
        // B3: a round-2 FAIL with nothing counted: no round 3, no fix-PASS; only the owner.
        let nothing = vec![fail("r2", 0, 0), fail("r1", 1, 0)];
        let (what, fix) = next_round(&nothing, &D, &never).unwrap_err();
        assert!(fix.contains("--waive-review") && !fix.contains("--fixes"), "{what} / {fix}");
        let (what, fix) = fix_pass(None, &nothing, "r2", &D, never).unwrap_err();
        assert!(fix.contains("--waive-review") && !fix.contains("review round 3"), "{what} / {fix}");
        assert!(next_step(&note("FAIL", 0, 0, json!({})), 2, &D, "t").contains("--waive-review"));
    }

    #[test]
    fn a_second_fix_and_a_lowered_cap_name_the_step_and_round_that_hold() {
        // Code review round 2, I2: a second fix above a blocker at the cap names the cap's way out,
        // never a fix-PASS that fixable refuses.
        let mut stacked = vec![fix_of("f3", "r3")];
        stacked.extend(failed_rounds(3));
        let (what, fix) = fix_pass(None, &stacked, "r3", &D, never).unwrap_err();
        assert!(!fix.contains("write a fix-PASS") && fix.contains("--verdict WAIVED") && fix.contains("--waive-review"), "{what} / {fix}");
        // I3: a target that lowers the cap below the rounds recorded names the last round as it is.
        let (what, _) = next_round(&failed_rounds(5), &D, &never).unwrap_err();
        assert!(what.contains("round 5 ran on r5") && !what.contains("round 3 ran on r5"), "{what}");
    }

    #[test]
    fn the_cap_waiver_stops_only_the_last_rounds_blocker_and_names_the_reason() {
        assert_eq!(cap(Some(&note("FAIL", 2, 1, json!({}))), &failed_rounds(2), &D, "tip"), Ok("review cap: 3 rounds, 2 blockers open".into()));
        assert_eq!(cap(Some(&note("FAIL", 1, 0, json!({}))), &failed_rounds(4), &OLD, "tip"), Ok("review cap: 5 rounds, 1 blockers open".into()));
        assert_eq!(cap(Some(&note("FAIL", 1, 0, json!({}))), &failed_rounds(5), &cap_of(6, OLD), "tip"), Ok("review cap: 6 rounds, 1 blockers open".into()));
        for (own, earlier, needle) in [
            (Some(note("FAIL", 1, 0, json!({}))), failed_rounds(1), "round 2 of 3"),
            (Some(note("FAIL", 0, 2, json!({}))), failed_rounds(2), "round 3 of 3, is FAIL with 0 blockers"),
            (Some(note("PASS", 0, 0, json!({}))), failed_rounds(2), "is PASS"),
            (None, failed_rounds(2), "no review of round 3"),
        ] {
            let (what, fix) = cap(own.as_ref(), &earlier, &D, "0123456789ab").unwrap_err();
            assert!(what.contains("cap is not reached on 0123456789") && what.contains(needle), "{what}");
            assert!(fix.contains("--waive-review") && !fix.contains("--verdict WAIVED"), "never points an agent at a waiver: {fix}");
        }
    }

    #[test]
    fn each_recorded_round_names_the_schedules_next_step() {
        let t = "0123456789ab";
        assert!(next_step(&note("PASS", 0, 0, json!({})), 1, &D, t).contains("--fixes 0123456789"));
        assert!(next_step(&note("FAIL", 0, 1, json!({})), 1, &D, t).contains("review round 2"));
        assert!(next_step(&note("FAIL", 0, 1, json!({})), 2, &D, t).contains("--verdict PASS --fixes"));
        assert!(next_step(&note("FAIL", 0, 1, json!({})), 2, &SET, t).contains("review round 3"));
        assert!(next_step(&note("FAIL", 1, 0, json!({})), 2, &D, t).contains("review round 3"));
        let capped = next_step(&note("FAIL", 2, 0, json!({})), 3, &D, t);
        assert!(
            capped.contains("review cap reached") && capped.contains("--verdict WAIVED") && capped.contains("review cap: 3 rounds, 2 blockers open"),
            "{capped}"
        );
        assert!(next_step(&note("FAIL", 0, 0, json!({})), 3, &D, t).contains("--waive-review"));
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
        let got = tags(&line, &D, |from, to| Ok(vec![format!("{from}->{to}").into_bytes()]));
        // Code review round 2, I1: an unreadable note stops the count, as it stops preflight and the writer.
        assert_eq!(
            got,
            [
                " [fix-PASS of a round past an unreadable note on r3; no reviewers ran; changed: r3->f3]",
                " [round not counted: an unreadable note below it]",
                "",
                " [round 2, since r1]",
                "",
                " [round 1, full]"
            ]
        );
        // The scope follows the target's whole_range_rounds.
        let three = [fail("r3", 0, 1), fail("r2", 0, 1), fail("r1", 0, 1)];
        assert_eq!(tags(&three, &SET, |_, _| Ok(vec![])), [" [round 3, since r2]", " [round 2, full]", " [round 1, full]"]);
        assert_eq!(tags(&three, &D, |_, _| Ok(vec![])), [" [round 3, since r2]", " [round 2, since r1]", " [round 1, full]"]);
        // A note that records another round or scope than the line gives it says so.
        let recorded =
            vec![at("r2", note("FAIL", 1, 0, json!({"round": 3, "scope": "since r0"}))), at("r1", note("FAIL", 1, 0, json!({"round": 1, "scope": "full"})))];
        assert_eq!(tags(&recorded, &D, |_, _| Ok(vec![])), [" [round 2, since r1; the note records round 3, since r0]", " [round 1, full]"]);
        let old = vec![at("r2", note("FAIL", 1, 0, json!({"round": 2, "scope": "full"}))), fail("r1", 1, 0)];
        assert_eq!(tags(&old, &D, |_, _| Ok(vec![]))[0], " [round 2, since r1; the note records round 2, full]", "a round recorded under the old schedule");
        // Epic code review round 5, I1: a fix-PASS that changed no file says so, as preflight does.
        let empty = tags(&[fix_of("f1", "r1"), at("r1", note("PASS", 0, 0, json!({})))], &D, |_, _| Ok(vec![]));
        assert_eq!(empty[0], " [fix-PASS of round 1 on r1; no reviewers ran; changed: nothing]");
        let orphan = tags(&[fix_of("f", "elsewhere")], &D, |_, _| Err("gone".into()));
        assert_eq!(orphan, [" [fix-PASS of a round not on this line on elsewhere; no reviewers ran; changed: unknown (gone)]"]);
        // B16: a fix-PASS whose note records another round than the line gives it says so too.
        let mut off = fix_of("f2", "r2");
        off.note.as_mut().unwrap().schedule.round = Some(4);
        let mut right = fix_of("f1", "r1");
        right.note.as_mut().unwrap().schedule.round = Some(1);
        let line = [off, fail("r2", 0, 1), right, fail("r1", 0, 1)];
        let got = tags(&line, &D, |_, _| Ok(vec![]));
        assert_eq!(got[0], " [fix-PASS of round 2 on r2; the note records round 4; no reviewers ran; changed: nothing]");
        assert_eq!(got[2], " [fix-PASS of round 1 on r1; no reviewers ran; changed: nothing]");
    }

    /// B16: the tip's own note is read once, with the line: the schedule keeps it apart from the
    /// rounds below it, and a note the caller already read stands in for the stored one.
    #[test]
    fn the_tips_own_note_is_read_with_the_line_and_kept_apart() {
        let line = || vec![fail("t", 1, 0), fail("r2", 0, 1), Entry { commit: "bad".into(), note: Err("x".into()) }, fail("r1", 1, 0)];
        let (own, earlier) = split_own(line(), "t");
        assert_eq!(own.unwrap().unwrap().blockers, 1);
        assert_eq!(earlier.iter().map(|e| e.commit.as_str()).collect::<Vec<_>>(), ["r2", "bad", "r1"]);
        let (own, earlier) = split_own(line().split_off(1), "t");
        assert!(own.is_none() && earlier.len() == 3, "no note on the tip");
        let (own, _) = split_own(vec![Entry { commit: "t".into(), note: Err("unreadable".into()) }], "t");
        assert_eq!(own.unwrap().unwrap_err(), "unreadable", "an unreadable own note stays an error");
        let below = line().split_off(1);
        let read = with_own(below, "t", note("WAIVED", 2, 0, json!({"waiver": "cap"})));
        assert_eq!(read.iter().map(|e| e.commit.as_str()).collect::<Vec<_>>(), ["t", "r2", "bad", "r1"]);
        assert_eq!(read[0].note.as_ref().unwrap().blockers, 2, "the caller's note, not a second read");
    }

    /// B16: a target that names no commit is an error, never the default schedule.
    #[test]
    fn the_policy_of_a_target_naming_no_commit_is_an_error() {
        let (what, fix) = policy_at("no-such-ref", None, |_| panic!("no config is read for a target naming no commit")).unwrap_err();
        assert!(what.contains("no-such-ref") && what.contains("names no commit") && fix.contains("--target"), "{what} / {fix}");
        assert_eq!(policy_at("t", Some("c"), |_| Ok(None)), Ok(D), "a target holding no config: the defaults");
        assert_eq!(policy_at("t", Some("c"), |c| Ok(Some(format!("[review]\nmax_rounds = {}\n", c.len() + 6)))).map(|p| p.max_rounds), Ok(7));
        assert!(policy_at("t", Some("c"), |_| Err("git failed".into())).is_err(), "a config git cannot read is an error");
        let (what, fix) = policy_at("epic/e", Some("c"), |_| Ok(Some("[review]\nmax_rounds = 0\n".into()))).unwrap_err();
        assert!(what.contains("the target epic/e is refused") && what.contains("max_rounds = 0") && fix.contains("[review]"), "{what} / {fix}");
    }

    /// B16: a fix-PASS over the owner's waiver says what that note is and where the fix goes.
    #[test]
    fn a_fix_pass_over_the_owners_waiver_names_it_and_the_way_on() {
        let owner = note("WAIVED", 0, 0, json!({"waiver": "owner ruling", "unreviewed": true}));
        let (what, fix) = fix_pass(Some(&owner), &failed_rounds(1), "r1", &D, never).unwrap_err();
        assert!(what.contains("owner's waiver") && what.contains("no review under it"), "{what}");
        assert!(fix.contains("later commit") && fix.contains("keel preflight") && fix.contains("--waive-review"), "{fix}");
        assert!(!fix.contains("the step the writer printed"), "the writer printed no step for the owner's waiver: {fix}");
    }

    #[test]
    fn the_schedule_comes_from_review_and_each_key_defaults_or_is_refused_by_name() {
        assert_eq!(policy_in("[check]\nlint = \"x\"\n"), Ok(D));
        assert_eq!(policy_in("[review]\nmodules = [\"src/*\"]\nrubric = \"r.md\"\n"), Ok(D), "the skill's keys are known");
        assert_eq!(policy_in("[review]\nmax_rounds = 5\nwhole_range_rounds = 2\nfix_importants_from = 4\n"), Ok(OLD), "the old schedule returns");
        assert_eq!(policy_in("[review]\nmax_rounds = 6\n"), Ok(cap_of(6, D)));
        assert_eq!(policy_in("[review]\nwhole_range_rounds = 3\n"), Ok(Policy { whole_range_rounds: 3, ..D }), "up to the cap");
        // A default above a lowered cap is lowered to it; only a value the project writes is refused.
        assert_eq!(policy_in("[review]\nmax_rounds = 1\n"), Ok(Policy { max_rounds: 1, whole_range_rounds: 1, fix_importants_from: 1 }));
        assert_eq!(policy_in("[review]\nmax_rounds = 2\n"), Ok(Policy { max_rounds: 2, whole_range_rounds: 1, fix_importants_from: 2 }));
        for (bad, says) in [
            ("[review]\nmax_rounds = 0\n", "[review] max_rounds = 0 must be a whole number of rounds, 1 or more"),
            ("[review]\nmax_rounds = -1\n", "max_rounds = -1 must be"),
            ("[review]\nwhole_range_rounds = \"3\"\n", "whole_range_rounds = \"3\" must be"),
            ("[review]\nfix_importants_from = 2.5\n", "fix_importants_from = 2.5 must be"),
            ("[review]\nmax_rounds = true\n", "max_rounds = true must be"),
            ("[review]\nmax_rounds = 99999999999\n", "max_rounds = 99999999999 is too large"),
            ("[review]\nwhole_range_rounds = 4\n", "whole_range_rounds = 4 is above max_rounds = 3"),
            ("[review]\nmax_rounds = 3\nfix_importants_from = 5\n", "fix_importants_from = 5 is above max_rounds = 3"),
            ("[review]\nmax_rounds = 1\nfix_importants_from = 2\n", "fix_importants_from = 2 is above max_rounds = 1"),
            (
                "[review]\nmax_round = 3\n",
                "[review] max_round is not a key keel knows (known: max_rounds, whole_range_rounds, fix_importants_from, modules, rubric)",
            ),
            ("review = 3\n", "review = 3 must be a table"),
            ("not toml [", "does not parse"),
        ] {
            let e = policy_in(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
