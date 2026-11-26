//! The review schedule (UC-REV-3): rounds counted from the cr notes on a branch's own first-parent
//! line, the scope each round reviews, which fixes need no new round (a fix-PASS), and the cap.
//! Derived from the notes every time, never typed: a fresh session counts what the branch recorded.
//! Its shape is the target's committed `[review]` settings (`Policy`, DEC-50).
use crate::notes::{self, CrNote, OWNER_WAIVES};
use crate::{git, short};
use std::collections::HashMap;
use std::path::Path;

/// The schedule's shape at one level: the target's committed `[review]` settings, its level table's
/// over them, each defaulting to `DEFAULT`'s.
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
    /// `fix_blockers_from` (E-CRLEVEL), unset unless written: from this round on every finding of a
    /// round, blockers and importants, is fixed under a fix-PASS, no round follows it and the cap's
    /// waiver is refused for it. Unset: a blocker earns another round, and at the cap the cap's waiver.
    pub fix_blockers_from: Option<u32>,
}

impl Policy {
    /// The default values (owner ruling 2026-11-24): a key neither the level's table nor `[review]`
    /// sets takes its value here, at both levels. The only place they are written; nothing else in
    /// keel depends on them.
    pub(crate) const DEFAULT: Policy = Policy { max_rounds: 3, whole_range_rounds: 1, fix_importants_from: 2, fix_blockers_from: None };

    /// Round `round` is from `fix_blockers_from` on: a fix-PASS answers all its findings, no round follows.
    fn answers_all(&self, round: u32) -> bool {
        self.fix_blockers_from.is_some_and(|f| round >= f)
    }

    /// The round from which importants are fixed under a fix-PASS: `fix_importants_from`, or
    /// `fix_blockers_from` when that comes first (it answers every finding from its round on).
    fn importants_from(&self) -> u32 {
        self.fix_blockers_from.map_or(self.fix_importants_from, |f| f.min(self.fix_importants_from))
    }
}

/// The level a review is judged at (E-CRLEVEL): its target's, by `level`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Level {
    /// A WU's review: `[review.wu]` over `[review]`.
    Wu,
    /// An epic's review: `[review.epic]` over `[review]`.
    Epic,
}

impl Level {
    /// The level's table under `[review]`.
    fn table(self) -> &'static str {
        match self {
            Level::Wu => "wu",
            Level::Epic => "epic",
        }
    }
}

/// The one rule for a review's level (owner ruling 2026-11-26): a review whose target is the main
/// branch (`[branches] main`, by name or as `refs/heads/<main>`) is an epic review, any other a WU
/// review. Preflight, the cr writer and the sign-off history each have the target, so they agree.
pub(crate) fn level(target: &str, main: &str) -> Level {
    if target == main || target.strip_prefix("refs/heads/") == Some(main) {
        Level::Epic
    } else {
        Level::Wu
    }
}

/// The schedule keys: what `[review]` and each level table may set.
const SCHEDULE_KEYS: [&str; 4] = ["max_rounds", "whole_range_rounds", "fix_importants_from", "fix_blockers_from"];

/// The other `[review]` keys keel knows: the two the keel-code-review skill reads.
const SKILL_KEYS: [&str; 2] = ["modules", "rubric"];

/// The level tables.
const LEVELS: [Level; 2] = [Level::Wu, Level::Epic];

/// How to get past a schedule `policy_in` refuses.
const POLICY_FIX: &str = "correct [review] (and [review.wu], [review.epic]) in .keel/config.toml on the target branch (each key a whole number of rounds, 1 or more, none above its level's max_rounds; a level table takes only the schedule keys; a key left out takes [review]'s, else its default), then run again";

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

/// The schedule in the target's committed `.keel/config.toml`, at the level `level(target, main)`
/// gives the review: the policy a branch is judged by, never one it or the working tree carries. A
/// target that holds no config, or a key left out: the default. A target that names no commit, a
/// config git cannot read, or a schedule `policy_in` refuses (at either level) is an error with its
/// fix line, never the default.
pub(crate) fn policy(repo: &Path, target: &str, main: &str) -> Result<Policy, (String, String)> {
    // Resolved first, then peeled: a suffix on `:/<text>` would join the text it searches for.
    let commit = git(repo, &["rev-parse", "--verify", "-q", "--end-of-options", target], &[])
        .and_then(|obj| git(repo, &["rev-parse", "--verify", "-q", &format!("{obj}^{{commit}}")], &[]))
        .ok();
    policy_at(target, level(target, main), commit.as_deref(), |commit| {
        if git(repo, &["ls-tree", "--name-only", commit, "--", ".keel/config.toml"], &[])?.is_empty() {
            return Ok(None);
        }
        git(repo, &["cat-file", "blob", &format!("{commit}:.keel/config.toml")], &[]).map(Some)
    })
}

/// `policy`'s rule on what git resolved: `commit`, the target's commit (`None`: it names none, an
/// error), and `config`, the `.keel/config.toml` that commit holds (`None`: none, the default).
fn policy_at(
    target: &str,
    level: Level,
    commit: Option<&str>,
    config: impl FnOnce(&str) -> Result<Option<String>, String>,
) -> Result<Policy, (String, String)> {
    let commit = commit.ok_or_else(|| {
        (
            format!("keel: the target {target} names no commit, so its [review] settings cannot be read"),
            "give --target a branch or commit that exists".to_string(),
        )
    })?;
    match config(commit).map_err(|e| (e, "fix the repo state (git fsck names what is broken), then run again".to_string()))? {
        None => Ok(Policy::DEFAULT),
        Some(text) => policy_in(&text)
            .map(|p| p.at(level))
            .map_err(|why| (format!("the review schedule on the target {target} is refused: {why}"), POLICY_FIX.to_string())),
    }
}

/// The schedule of each level a `.keel/config.toml` sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Policies {
    pub wu: Policy,
    pub epic: Policy,
}

impl Policies {
    pub(crate) fn at(&self, level: Level) -> Policy {
        match level {
            Level::Wu => self.wu,
            Level::Epic => self.epic,
        }
    }
}

/// The schedule keys one table writes: `None` where it writes none.
#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    max_rounds: Option<u32>,
    whole_range_rounds: Option<u32>,
    fix_importants_from: Option<u32>,
    fix_blockers_from: Option<u32>,
}

/// The schedule keys table `name` writes, each a whole number of rounds, 1 or more, that fits u32.
fn keys(t: &toml::Table, name: &str) -> Result<Keys, String> {
    let key = |k: &str| match t.get(k) {
        None => Ok(None),
        Some(toml::Value::Integer(n)) if *n >= 1 => u32::try_from(*n).map(Some).map_err(|_| format!("{name} {k} = {n} is too large")),
        Some(other) => Err(format!("{name} {k} = {other} must be a whole number of rounds, 1 or more")),
    };
    Ok(Keys {
        max_rounds: key("max_rounds")?,
        whole_range_rounds: key("whole_range_rounds")?,
        fix_importants_from: key("fix_importants_from")?,
        fix_blockers_from: key("fix_blockers_from")?,
    })
}

/// The cap a table is judged by when it sets no `max_rounds` and `[review]` sets none either.
const DEFAULT_CAP: &str = ", the default cap (max_rounds is not set)";

/// One table's schedule: per key, what table `name` writes (`own`) wins, then what it inherits
/// (`under`). A key `own` writes above the resulting `max_rounds` is refused (`cap_is` says where
/// that cap comes from when `own` does not write it); an inherited one is lowered to it.
fn resolve(name: &str, own: Keys, under: Policy, cap_is: &str) -> Result<Policy, String> {
    let max = own.max_rounds.unwrap_or(under.max_rounds);
    let cap_is = if own.max_rounds.is_some() { "" } else { cap_is };
    let under_cap = |k: &str, written: Option<u32>, inherited: Option<u32>| match written {
        Some(n) if n > max => Err(format!("{name} {k} = {n} is above max_rounds = {max}{cap_is}: no round runs past the cap")),
        Some(n) => Ok(Some(n)),
        None => Ok(inherited.map(|n| n.min(max))),
    };
    let set = |k: &str, written: Option<u32>, inherited: u32| under_cap(k, written, Some(inherited)).map(|n| n.unwrap_or(inherited));
    Ok(Policy {
        max_rounds: max,
        whole_range_rounds: set("whole_range_rounds", own.whole_range_rounds, under.whole_range_rounds)?,
        fix_importants_from: set("fix_importants_from", own.fix_importants_from, under.fix_importants_from)?,
        fix_blockers_from: under_cap("fix_blockers_from", own.fix_blockers_from, under.fix_blockers_from)?,
    })
}

/// The schedule of each level a `.keel/config.toml` sets, or why it is refused (the reason only). One
/// rule for the target's committed file and a staged one (`main::staged_config_refusal`), and for both
/// levels: `[review]` holds the shared schedule keys, `[review.wu]` and `[review.epic]` only schedule
/// keys; per key, the level's value wins, then `[review]`'s, then `Policy::DEFAULT`'s. Each key, when
/// written, a whole number of rounds, 1 or more, that fits u32, not above its table's `max_rounds`
/// (`[review]`'s judged on its own, as before the levels); an inherited value above a level's
/// `max_rounds` is lowered to it, so a cap set alone below a default still works. No `[review]` key or
/// table keel does not know. A file that writes no level table gives both levels `[review]`'s schedule.
pub(crate) fn policy_in(text: &str) -> Result<Policies, String> {
    let v: toml::Value = text.parse().map_err(|e| format!(".keel/config.toml does not parse: {e}"))?;
    let Some(review) = v.get("review") else { return Ok(Policies { wu: Policy::DEFAULT, epic: Policy::DEFAULT }) };
    let t = review.as_table().ok_or_else(|| format!("review = {review} must be a table, [review]"))?;
    for (k, value) in t {
        if SCHEDULE_KEYS.contains(&k.as_str()) || SKILL_KEYS.contains(&k.as_str()) {
            continue;
        }
        if LEVELS.iter().any(|l| l.table() == k) {
            if !value.is_table() {
                return Err(format!("[review] {k} = {value} must be a table, [review.{k}], holding that level's schedule keys"));
            }
            continue;
        }
        return Err(if value.is_table() {
            format!("[review.{k}] is not a table keel knows (known: [review.wu], [review.epic])")
        } else {
            format!("[review] {k} is not a key keel knows (known: {}, {}; tables [review.wu], [review.epic])", SCHEDULE_KEYS.join(", "), SKILL_KEYS.join(", "))
        });
    }
    let shared = keys(t, "[review]")?;
    resolve("[review]", shared, Policy::DEFAULT, DEFAULT_CAP)?;
    // What a level inherits: [review]'s written value, else the default; lowered to the level's cap.
    let d = Policy::DEFAULT;
    let under = Policy {
        max_rounds: shared.max_rounds.unwrap_or(d.max_rounds),
        whole_range_rounds: shared.whole_range_rounds.unwrap_or(d.whole_range_rounds),
        fix_importants_from: shared.fix_importants_from.unwrap_or(d.fix_importants_from),
        fix_blockers_from: shared.fix_blockers_from.or(d.fix_blockers_from),
    };
    let at = |l: Level| -> Result<Policy, String> {
        let name = format!("[review.{}]", l.table());
        let own = match t.get(l.table()).and_then(toml::Value::as_table) {
            None => Keys::default(),
            Some(lt) => {
                if let Some(k) = lt.keys().find(|k| !SCHEDULE_KEYS.contains(&k.as_str())) {
                    return Err(format!("{name} {k} is not a schedule key (a level table takes only {})", SCHEDULE_KEYS.join(", ")));
                }
                keys(lt, &name)?
            }
        };
        let cap_is = if shared.max_rounds.is_some() { format!(", [review]'s ({name} does not set max_rounds)") } else { DEFAULT_CAP.to_string() };
        resolve(&name, own, under, &cap_is)
    };
    Ok(Policies { wu: at(Level::Wu)?, epic: at(Level::Epic)? })
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

/// Whether another review round runs after round `round`: within the cap, never from
/// `fix_blockers_from` on, and after a round from `fix_importants_from` on only when it found a blocker.
fn round_follows(note: &CrNote, round: u32, p: &Policy) -> bool {
    round < p.max_rounds && !p.answers_all(round) && (round < p.fix_importants_from || note.blockers > 0)
}

/// What follows round `round` (see `Then`). A FAIL with nothing counted needs a round like any other
/// failed round, and where none follows, the owner decides.
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
/// cap, after a round from `fix_blockers_from` on, and after a round from `fix_importants_from` on
/// unless it found a blocker; the fix line is `then`'s.
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
    } else if let Some(f) = p.fix_blockers_from.filter(|_| p.answers_all(last)) {
        format!(
            "no review round {n} runs: from round {f} on (fix_blockers_from) a round's findings, blockers too, are fixed under a fix-PASS, and round {last} on {} is one",
            short(c)
        )
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
    /// The target's schedule, at the review's level.
    pub policy: Policy,
    repo: &'a Path,
    tip: &'a str,
    code_paths: &'a [String],
}

impl<'a> Schedule<'a> {
    /// Refused, with its fix line, when the line cannot be read or the target's schedule is refused.
    /// `main`: the main branch's name, which makes `target` an epic review (`level`).
    pub(crate) fn read(repo: &'a Path, base: &str, tip: &'a str, target: &str, main: &str, code_paths: &'a [String]) -> Result<Self, (String, String)> {
        let line = line(repo, base, tip, None).map_err(|e| (e, "fix the repo state, then run again".to_string()))?;
        let (own, earlier) = split_own(line, tip);
        Ok(Schedule { own, earlier, policy: policy(repo, target, main)?, repo, tip, code_paths })
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
/// importants and no blocker from `fix_importants_from`; every counted finding, blockers too, from
/// `fix_blockers_from` (neither above the cap: `policy_in` refuses it).
pub(crate) fn fixable(note: &CrNote, round: u32, p: &Policy) -> Result<(), String> {
    let from = p.importants_from();
    debug_assert!(from <= p.max_rounds, "policy_in keeps fix_importants_from within max_rounds: {p:?}");
    match (note.verdict.as_str(), note.blockers, note.importants) {
        ("PASS", ..) => Ok(()),
        ("FAIL", b, i) if b + i > 0 && p.answers_all(round) => Ok(()),
        ("FAIL", 0, i) if i > 0 && round >= from => Ok(()),
        ("FAIL", 0, i) if i > 0 => Err(format!("round {round} found {i} importants; before round {from} importants need a new round")),
        ("FAIL", b, _) if b > 0 => Err(match p.fix_blockers_from {
            Some(f) => {
                format!("round {round} found {b} blockers; before round {f} (fix_blockers_from) a blocker needs a new round (at the cap, the owner decides)")
            }
            None => format!("round {round} found {b} blockers; a blocker always needs a new round (at the cap, the owner decides)"),
        }),
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

/// What a fix-PASS answers. `blockers` is said, never recorded: the round's own note holds them
/// (cr/v1 unchanged).
#[derive(Debug, PartialEq)]
pub(crate) struct Fix {
    pub round: u32,
    pub blockers: u32,
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
        return Err((
            "no review round on this branch below the tip, so a fix-PASS has nothing to answer".into(),
            "run the review (its first round) on the tip".into(),
        ));
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
    Ok(Fix { round, blockers: note.blockers, importants: note.importants, nits: note.extra.get("nits").and_then(|n| n.as_array()).map_or(0, Vec::len) })
}

pub(crate) fn cap_reason(max: u32, blockers: u32) -> String {
    format!("review cap: {max} rounds, {blockers} blockers open")
}

/// The cap's waiver on the tip, with the reason the tool fixes: only over the last round's FAIL with
/// a blocker, and never from `fix_blockers_from` on, where a fix-PASS answers it. `own`: the tip's own note.
pub(crate) fn cap(own: Option<&CrNote>, earlier: &[Entry], p: &Policy, tip: &str) -> Result<String, (String, String)> {
    let round = rounds(earlier)?.len() as u32 + 1;
    let max = p.max_rounds;
    match own {
        Some(n) if kind(n) == Kind::Round && then(n, round, p) == Then::Cap => Ok(cap_reason(max, n.blockers)),
        Some(n) if kind(n) == Kind::Round && n.verdict == "FAIL" && n.blockers > 0 && p.answers_all(round) => Err((
            format!(
                "the cap's waiver is refused on {}: its review, round {round} of {max}, found {} blockers, and from round {} on (fix_blockers_from) a blocker is fixed under a fix-PASS, never waived by the cap",
                short(tip),
                n.blockers,
                p.fix_blockers_from.unwrap_or(round)
            ),
            format!("fix the findings, then on the fix commit: keel notes write cr --verdict PASS --fixes {}   (no new round)", short(tip)),
        )),
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
        Then::FixPass if note.blockers > 0 => format!(
            "fix the blockers and importants, then on the fix commit: keel notes write cr --verdict PASS --fixes {t}   (no new round and no cap waiver: from round {} on, fix_blockers_from, a blocker is fixed under a fix-PASS; the owner reads the fix at sign-off)",
            p.fix_blockers_from.unwrap_or(round)
        ),
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
/// `[round n, since <commit>]`, `[fix-PASS of round n on <commit>; no reviewers ran; changed: ...]`
/// (with `answers b blockers: ...` when the round it answers found blockers, read from that round's
/// own note, `blockers_of`), or nothing. Rounds are numbered from the oldest under `rounds`' rule: an unreadable note stops the
/// count, so a round above it shows as not counted; a round's scope is `p`'s (the target's
/// `whole_range_rounds`). A note that records another round or scope than the line gives it says
/// so. `files` lists what a fix-PASS commit changed, as `notes::diff_names` does;
/// `notes::changed_text` renders it, as for preflight.
pub(crate) fn tags(entries: &[Entry], p: &Policy, files: impl Fn(&str, &str) -> Result<Vec<Vec<u8>>, String>) -> Vec<String> {
    let mut numbered: HashMap<&str, (u32, &CrNote)> = HashMap::new();
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
                numbered.insert(&e.commit, (round, n));
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
                let answered = numbered.get(reviewed);
                let round = answered.map(|(r, _)| r);
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
                let blockers = answered.map(|(_, r)| blockers_of(r)).unwrap_or_default();
                format!(" [fix-PASS of {of} on {}{note_says}{blockers}; no reviewers ran; changed: {changed}]", short(reviewed))
            }
            _ => String::new(),
        };
        tags.push(tag);
    }
    tags.reverse();
    tags
}

/// What a fix-PASS of round `r` answers beyond importants and nits: the blockers that round's own
/// note counts, each named by its open finding (cr/v1 records none on the fix-PASS itself), or nothing.
fn blockers_of(r: &CrNote) -> String {
    if r.verdict != "FAIL" || r.blockers == 0 {
        return String::new();
    }
    let named: Vec<String> = r
        .open_findings()
        .into_iter()
        .filter(|f| f["severity"].as_str() == Some("blocker"))
        .map(|f| match (f["location"].as_str(), f["claim"].as_str()) {
            (Some(l), Some(c)) => format!("{l}: {c}"),
            (None, Some(c)) => c.to_string(),
            _ => f.to_string(),
        })
        .collect();
    let named = if named.is_empty() { "its note names none".to_string() } else { named.join("; ") };
    format!("; answers {} blockers ({named})", r.blockers)
}

#[cfg(test)]
mod tests {
    use super::{
        cap, fix_pass, fixable, level, next_round, next_step, parse_note, policy_at, policy_in, split_own, tags, then, with_own, Entry, Fix, Kind, Level, Next,
        Policies, Policy, Then,
    };
    use crate::notes::{full_sha, CrNote};
    use serde_json::json;

    /// Every schedule the tests run with fix_blockers_from unset, the default values one among them:
    /// each expectation below is computed from the schedule's settings, never typed for one of them.
    const POLICIES: [Policy; 6] = [
        Policy::DEFAULT,
        Policy { max_rounds: 5, whole_range_rounds: 2, fix_importants_from: 4, fix_blockers_from: None },
        Policy { max_rounds: 4, whole_range_rounds: 2, fix_importants_from: 3, fix_blockers_from: None },
        Policy { max_rounds: 2, whole_range_rounds: 1, fix_importants_from: 2, fix_blockers_from: None },
        Policy { max_rounds: 4, whole_range_rounds: 3, fix_importants_from: 3, fix_blockers_from: None },
        Policy { max_rounds: 1, whole_range_rounds: 1, fix_importants_from: 1, fix_blockers_from: None },
    ];

    /// Schedules that set fix_blockers_from (E-CRLEVEL): at round 1 with one round, below, at and
    /// above fix_importants_from, and at the cap.
    const FB_POLICIES: [Policy; 6] = [
        Policy { max_rounds: 1, whole_range_rounds: 1, fix_importants_from: 1, fix_blockers_from: Some(1) },
        Policy { max_rounds: 3, whole_range_rounds: 2, fix_importants_from: 3, fix_blockers_from: Some(2) },
        Policy { max_rounds: 4, whole_range_rounds: 1, fix_importants_from: 4, fix_blockers_from: Some(2) },
        Policy { max_rounds: 4, whole_range_rounds: 2, fix_importants_from: 2, fix_blockers_from: Some(2) },
        Policy { max_rounds: 5, whole_range_rounds: 2, fix_importants_from: 2, fix_blockers_from: Some(4) },
        Policy { max_rounds: 3, whole_range_rounds: 1, fix_importants_from: 2, fix_blockers_from: Some(3) },
    ];

    /// Every schedule the tests run, set or not.
    fn all_policies() -> impl Iterator<Item = Policy> {
        POLICIES.into_iter().chain(FB_POLICIES)
    }

    /// Both levels on one schedule: what a file writing no level table gives.
    fn both(p: Policy) -> Policies {
        Policies { wu: p, epic: p }
    }

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

    /// Round `k` failed with these counts, on top of `k - 1` rounds with a blocker.
    fn rounds_to(k: u32, blockers: u32, importants: u32) -> Vec<Entry> {
        let mut line = failed_rounds(k);
        line[0] = fail(&format!("r{k}"), blockers, importants);
        line
    }

    /// The scope the settings give round `k` (commits named r<n>): the whole range up to
    /// `whole_range_rounds`, else the change since round `k - 1`.
    fn scope(p: &Policy, k: u32) -> String {
        if k <= p.whole_range_rounds {
            "full".into()
        } else {
            format!("since r{}", k - 1)
        }
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
    fn each_schedule_numbers_its_rounds_and_scopes_them_by_whole_range_rounds() {
        for p in POLICIES {
            let max = p.max_rounds;
            assert_eq!(next_round(&[], &p, &never), Ok(Next { round: 1, max, since: None }), "{p:?}");
            for k in 1..max {
                let want = Next { round: k + 1, max, since: (k + 1 > p.whole_range_rounds).then(|| format!("r{k}")) };
                assert_eq!(next_round(&failed_rounds(k), &p, &never), Ok(want), "{p:?} after round {k}");
            }
            if max > 1 {
                let mut padded = vec![fix_of("f1", "r1"), at("w", note("WAIVED", 0, 0, json!({"waiver": "x", "unreviewed": true})))];
                padded.extend(failed_rounds(1));
                assert_eq!(next_round(&padded, &p, &never).unwrap().round, 2, "a fix-PASS or an unreviewed waiver is no round: {p:?}");
            }
            let first = next_round(&[], &p, &never).unwrap();
            assert_eq!(
                (first.scope(), first.announce("b", "t", "", "")),
                ("full".to_string(), format!("review round 1 of {max} - scope: the whole range, b..t"))
            );
            let narrow = Next { round: p.whole_range_rounds + 1, max, since: Some("s".into()) };
            let a = narrow.announce("b", "t", "git diff-tree X", "src/a.rs");
            assert!(
                a.starts_with(&format!("review round {} of {max} - scope: the last fix, s..t: this branch's own changes", p.whole_range_rounds + 1)),
                "{a}"
            );
            assert!(a.contains("its parents: git diff-tree X; changed: src/a.rs (") && narrow.scope() == "since s", "{a}");
            let unreadable = vec![fail("r2", 1, 0), Entry { commit: "0123456789ab".into(), note: Err("it does not parse".into()) }];
            let (what, fix) = next_round(&unreadable, &p, &never).unwrap_err();
            assert!(what.contains("0123456789 is unreadable") && fix.contains("repairs"), "{what}");
        }
    }

    #[test]
    fn no_round_runs_past_max_rounds_and_a_blocker_there_ends_in_the_caps_waiver() {
        for p in POLICIES {
            let max = p.max_rounds;
            let (what, fix) = next_round(&failed_rounds(max), &p, &never).unwrap_err();
            assert!(what.contains(&format!("no review round {} runs: the cap is {max} rounds, and round {max} ran on r{max}", max + 1)), "{p:?}: {what}");
            assert!(fix.contains("--verdict WAIVED") && fix.contains("--waive-review"), "{p:?}: {fix}");
            assert_eq!(next_round(&failed_rounds(max), &cap_of(max + 1, p), &never).unwrap().round, max + 1, "raising max_rounds moves the cap");
            // A target that lowers the cap below the rounds recorded names the last round as it is.
            let (what, _) = next_round(&failed_rounds(max + 2), &p, &never).unwrap_err();
            assert!(what.contains(&format!("round {} ran on r{}", max + 2, max + 2)), "{what}");
        }
    }

    #[test]
    fn importants_earn_a_round_only_before_fix_importants_from_and_a_blocker_in_every_round_below_the_cap() {
        for p in POLICIES {
            let (max, from) = (p.max_rounds, p.fix_importants_from);
            for k in 1..=max {
                if k < max {
                    assert_eq!(next_round(&rounds_to(k, 1, 0), &p, &never).unwrap().round, k + 1, "{p:?}: a round-{k} blocker");
                }
                let got = next_round(&rounds_to(k, 0, 2), &p, &never);
                if k < from {
                    assert_eq!(got.unwrap().round, k + 1, "{p:?}: round-{k} importants earn a round");
                    continue;
                }
                let (what, fix) = got.unwrap_err();
                let want = if k == max {
                    format!("no review round {} runs: the cap is {max} rounds", k + 1)
                } else {
                    format!("review round {} runs only after round {k} found a blocker, and round {k} on r{k} found none", k + 1)
                };
                assert!(what.contains(&want) && fix.contains(&format!("--fixes r{k}")), "{p:?} round {k}: {what} / {fix}");
                let mut fixed = vec![fix_of("f", &format!("r{k}"))];
                fixed.extend(rounds_to(k, 0, 2));
                let (what, fix) = next_round(&fixed, &p, &never).unwrap_err();
                assert!(
                    what.contains("fix-PASS on f already answers it") && fix.contains("--waive-review") && !fix.contains("--fixes"),
                    "a second fix: {what} / {fix}"
                );
                let (_, fix) = next_round(&fixed, &p, &|c| Ok(c == "f")).unwrap_err();
                assert!(fix.contains(&format!("--fixes r{k}")), "no code since the fix-PASS: it carries over: {fix}");
            }
        }
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
                    assert_eq!(p, both(Policy { max_rounds: m, whole_range_rounds: w, fix_importants_from: f, fix_blockers_from: None }));
                    let p = p.wu;
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
        let imp = note("FAIL", 0, 2, json!({}));
        for p in POLICIES {
            let (max, from) = (p.max_rounds, p.fix_importants_from);
            for k in 1..=max {
                let rk = format!("r{k}");
                assert_eq!(fixable(&pass_nits, k, &p), Ok(()));
                match fixable(&imp, k, &p) {
                    Ok(()) => assert!(k >= from, "{p:?} round {k}"),
                    Err(e) => assert!(k < from && e.contains(&format!("before round {from} importants need a new round")), "{p:?} round {k}: {e}"),
                }
                assert!(fixable(&note("FAIL", 1, 0, json!({})), k, &p).unwrap_err().contains("blocker"));
                assert!(fixable(&note("FAIL", 0, 0, json!({})), k, &p).is_err(), "a FAIL with nothing counted needs a round");
                assert!(fixable(&note("WAIVED", 0, 1, json!({"waiver": "w"})), k, &p).is_err());

                let got = fix_pass(None, &rounds_to(k, 0, 3), &rk, &p, never);
                if k >= from {
                    assert_eq!(got, Ok(Fix { round: k, blockers: 0, importants: 3, nits: 0 }), "{p:?} round {k}");
                } else {
                    assert_eq!(got.unwrap_err().1, format!("fix the findings, then run review round {} on the fix", k + 1), "{p:?} round {k}");
                }
                if k > 1 {
                    let (what, _) = fix_pass(None, &rounds_to(k, 0, 3), "r1", &p, never).unwrap_err();
                    assert!(what.contains(&format!("latest review round on this branch is round {k} on {rk}")), "{what}");
                }
                // A second code fix over a fix-PASS needs the next round where one follows, else the owner.
                let mut stacked = vec![fix_of("f2", &rk), fix_of("f1", &rk)];
                stacked.extend(rounds_to(k, 0, 3));
                if k >= from {
                    let (what, fix) = fix_pass(None, &stacked, &rk, &p, never).unwrap_err();
                    assert!(what.contains(&format!("fix-PASS on f2 already answers round {k}, and code changed since")), "{what}");
                    assert!(what.contains(&format!("no round {} follows round {k}", k + 1)), "{what}");
                    assert!(fix.contains("--waive-review") && !fix.contains("run review round"), "{fix}");
                    assert_eq!(
                        fix_pass(None, &stacked, &rk, &p, |c| Ok(c == "f2")),
                        Ok(Fix { round: k, blockers: 0, importants: 3, nits: 0 }),
                        "it carries over"
                    );
                    assert!(fix_pass(None, &stacked, &rk, &p, |c| Ok(c == "f1")).is_err(), "only the newest fix-PASS counts");
                }
                let mut over_a_pass = vec![fix_of("f2", &rk), fix_of("f1", &rk), at(&rk, note("PASS", 0, 0, json!({})))];
                over_a_pass.extend(failed_rounds(k - 1));
                let (_, fix) = fix_pass(None, &over_a_pass, &rk, &p, never).unwrap_err();
                if k < max && k < from {
                    assert_eq!(fix, format!("run review round {} on the tip", k + 1), "{p:?} round {k}");
                } else {
                    assert!(fix.contains("--waive-review"), "{p:?} round {k}: {fix}");
                }
            }
        }
        let line = vec![at("r1", pass_nits)];
        assert_eq!(fix_pass(None, &line, "r1", &Policy::DEFAULT, never), Ok(Fix { round: 1, blockers: 0, importants: 0, nits: 2 }));
        assert!(fix_pass(None, &[], "r1", &Policy::DEFAULT, never).is_err(), "nothing to answer");
        // Epic code review round 2, I2: when keel cannot tell what changed, it refuses, never guessing.
        let broken = |_: &str| -> Result<bool, String> { Err("git failed".into()) };
        for p in POLICIES {
            let k = p.fix_importants_from;
            let (what, _) = fix_pass(None, &rounds_to(k, 0, 2), &format!("r{k}"), &p, broken).unwrap_err();
            assert!(what.contains("cannot tell what this branch changed") && what.contains("git failed"), "a failed round is never answered unproven: {what}");
        }
    }

    #[test]
    fn what_follows_a_round_is_decided_in_one_place() {
        let (pass, imp, blk, none) =
            (note("PASS", 0, 0, json!({})), note("FAIL", 0, 2, json!({})), note("FAIL", 1, 0, json!({})), note("FAIL", 0, 0, json!({})));
        let waived = note("WAIVED", 1, 0, json!({"waiver": "owner"}));
        for p in POLICIES {
            let (max, from) = (p.max_rounds, p.fix_importants_from);
            for k in 1..=max {
                let next = Then::Round(k + 1);
                let want = [
                    (&pass, Then::FixPass),
                    (&imp, if k >= from { Then::FixPass } else { next }),
                    (&blk, if k < max { Then::Round(k + 1) } else { Then::Cap }),
                    (&none, if k < from { Then::Round(k + 1) } else { Then::Owner }),
                    (&waived, if k < max { Then::Round(k + 1) } else { Then::Owner }),
                ];
                for (n, want) in want {
                    assert_eq!(then(n, k, &p), want, "{} {}/{} round {k} on {p:?}", n.verdict, n.blockers, n.importants);
                }
            }
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
        for p in POLICIES {
            let (max, from) = (p.max_rounds, p.fix_importants_from);
            let rmax = format!("r{max}");
            // Code review round 1, B2: a blocker in the last round ends in the cap's waiver, never a round past the cap.
            let (what, fix) = fix_pass(None, &failed_rounds(max), &rmax, &p, never).unwrap_err();
            assert!(fix.contains("--verdict WAIVED") && fix.contains("--waive-review") && !fix.contains(&format!("round {}", max + 1)), "{what} / {fix}");
            // Code review round 2, I2: a second fix above it names the cap's way out, never a fix-PASS.
            let mut stacked = vec![fix_of("f", &rmax)];
            stacked.extend(failed_rounds(max));
            let (what, fix) = fix_pass(None, &stacked, &rmax, &p, never).unwrap_err();
            assert!(!fix.contains("write a fix-PASS") && fix.contains("--verdict WAIVED") && fix.contains("--waive-review"), "{what} / {fix}");
            // B3: a FAIL with nothing counted from fix_importants_from on: no round, no fix-PASS; only the owner.
            let nothing = rounds_to(from, 0, 0);
            let (what, fix) = next_round(&nothing, &p, &never).unwrap_err();
            assert!(fix.contains("--waive-review") && !fix.contains("--fixes"), "{p:?}: {what} / {fix}");
            let (what, fix) = fix_pass(None, &nothing, &format!("r{from}"), &p, never).unwrap_err();
            assert!(fix.contains("--waive-review") && !fix.contains(&format!("review round {}", from + 1)), "{p:?}: {what} / {fix}");
            assert!(next_step(&note("FAIL", 0, 0, json!({})), from, &p, "t").contains("--waive-review"));
        }
    }

    #[test]
    fn the_cap_waiver_stops_only_the_last_rounds_blocker_and_names_the_reason() {
        for p in POLICIES {
            let max = p.max_rounds;
            let below = failed_rounds(max - 1);
            assert_eq!(cap(Some(&note("FAIL", 2, 1, json!({}))), &below, &p, "tip"), Ok(format!("review cap: {max} rounds, 2 blockers open")), "{p:?}");
            let mut refused = vec![
                (Some(note("FAIL", 0, 2, json!({}))), failed_rounds(max - 1), format!("round {max} of {max}, is FAIL with 0 blockers")),
                (Some(note("PASS", 0, 0, json!({}))), failed_rounds(max - 1), "is PASS".to_string()),
                (None, failed_rounds(max - 1), format!("no review of round {max}")),
            ];
            if max > 1 {
                refused.push((Some(note("FAIL", 1, 0, json!({}))), failed_rounds(max - 2), format!("round {} of {max}", max - 1)));
            }
            for (own, earlier, needle) in refused {
                let (what, fix) = cap(own.as_ref(), &earlier, &p, "0123456789ab").unwrap_err();
                assert!(what.contains("cap is not reached on 0123456789") && what.contains(&needle), "{p:?}: {what}");
                assert!(fix.contains("--waive-review") && !fix.contains("--verdict WAIVED"), "never points an agent at a waiver: {fix}");
            }
        }
    }

    #[test]
    fn each_recorded_round_names_the_schedules_next_step() {
        let t = "0123456789ab";
        for p in POLICIES {
            let (max, from) = (p.max_rounds, p.fix_importants_from);
            for k in 1..=max {
                let next = format!("review round {}", k + 1);
                assert!(next_step(&note("PASS", 0, 0, json!({})), k, &p, t).contains("--fixes 0123456789"));
                let imp = next_step(&note("FAIL", 0, 1, json!({})), k, &p, t);
                assert!(if k >= from { imp.contains("--verdict PASS --fixes") } else { imp.contains(&next) }, "{p:?} round {k}: {imp}");
                let blk = next_step(&note("FAIL", 2, 0, json!({})), k, &p, t);
                if k < max {
                    assert!(blk.contains(&next), "{p:?} round {k}: {blk}");
                } else {
                    let reason = format!("review cap: {max} rounds, 2 blockers open");
                    assert!(blk.contains("review cap reached") && blk.contains("--verdict WAIVED") && blk.contains(&reason), "{blk}");
                }
                let none = next_step(&note("FAIL", 0, 0, json!({})), k, &p, t);
                assert!(if k >= from { none.contains("--waive-review") } else { none.contains(&next) }, "{p:?} round {k}: {none}");
            }
        }
    }

    #[test]
    fn the_history_tags_number_rounds_from_the_oldest_scope_them_by_the_settings_and_name_each_fix_pass() {
        for p in POLICIES {
            let line = failed_rounds(p.max_rounds + 1);
            let want: Vec<String> = (1..=p.max_rounds + 1).rev().map(|k| format!(" [round {k}, {}]", scope(&p, k))).collect();
            assert_eq!(tags(&line, &p, |_, _| Ok(vec![])), want, "{p:?}");
            // A round the note records otherwise (an older schedule's scope, say) says so.
            let old = vec![at("r2", note("FAIL", 1, 0, json!({"round": 2, "scope": "full"}))), fail("r1", 1, 0)];
            let says = if scope(&p, 2) == "full" { String::new() } else { "; the note records round 2, full".to_string() };
            assert_eq!(tags(&old, &p, |_, _| Ok(vec![]))[0], format!(" [round 2, {}{says}]", scope(&p, 2)), "{p:?}");
            let recorded = vec![at("r2", note("FAIL", 1, 0, json!({"round": 3, "scope": "since r0"}))), fail("r1", 1, 0)];
            assert_eq!(tags(&recorded, &p, |_, _| Ok(vec![]))[0], format!(" [round 2, {}; the note records round 3, since r0]", scope(&p, 2)));
        }
        let p = Policy::DEFAULT;
        let line = vec![
            fix_of("f3", "r3"),
            at("r3", note("PASS", 0, 0, json!({}))),
            Entry { commit: "bad".into(), note: Err("x".into()) },
            fail("r2", 0, 1),
            at("w0", note("WAIVED", 0, 0, json!({"waiver": "w", "unreviewed": true}))),
            fail("r1", 1, 0),
        ];
        // Code review round 2, I1: an unreadable note stops the count, as it stops preflight and the writer.
        let got = tags(&line, &p, |from, to| Ok(vec![format!("{from}->{to}").into_bytes()]));
        let (round2, round1) = (format!(" [round 2, {}]", scope(&p, 2)), format!(" [round 1, {}]", scope(&p, 1)));
        assert_eq!(
            got,
            [
                " [fix-PASS of a round past an unreadable note on r3; no reviewers ran; changed: r3->f3]",
                " [round not counted: an unreadable note below it]",
                "",
                round2.as_str(),
                "",
                round1.as_str(),
            ]
        );
        // Epic code review round 5, I1: a fix-PASS that changed no file says so, as preflight does.
        let empty = tags(&[fix_of("f1", "r1"), at("r1", note("PASS", 0, 0, json!({})))], &p, |_, _| Ok(vec![]));
        assert_eq!(empty[0], " [fix-PASS of round 1 on r1; no reviewers ran; changed: nothing]");
        let orphan = tags(&[fix_of("f", "elsewhere")], &p, |_, _| Err("gone".into()));
        assert_eq!(orphan, [" [fix-PASS of a round not on this line on elsewhere; no reviewers ran; changed: unknown (gone)]"]);
        // B16: a fix-PASS whose note records another round than the line gives it says so too.
        let mut off = fix_of("f2", "r2");
        off.note.as_mut().unwrap().schedule.round = Some(4);
        let mut right = fix_of("f1", "r1");
        right.note.as_mut().unwrap().schedule.round = Some(1);
        let line = [off, fail("r2", 0, 1), right, fail("r1", 0, 1)];
        let got = tags(&line, &p, |_, _| Ok(vec![]));
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

    /// B16: a target that names no commit is an error, never the default values.
    #[test]
    fn the_policy_of_a_target_naming_no_commit_is_an_error() {
        let (what, fix) = policy_at("no-such-ref", Level::Wu, None, |_| panic!("no config is read for a target naming no commit")).unwrap_err();
        assert!(what.contains("no-such-ref") && what.contains("names no commit") && fix.contains("--target"), "{what} / {fix}");
        assert_eq!(policy_at("t", Level::Wu, Some("c"), |_| Ok(None)), Ok(Policy::DEFAULT), "a target holding no config: the default values");
        assert_eq!(policy_at("t", Level::Epic, Some("c"), |c| Ok(Some(format!("[review]\nmax_rounds = {}\n", c.len() + 6)))).map(|p| p.max_rounds), Ok(7));
        assert!(policy_at("t", Level::Wu, Some("c"), |_| Err("git failed".into())).is_err(), "a config git cannot read is an error");
        let (what, fix) = policy_at("epic/e", Level::Wu, Some("c"), |_| Ok(Some("[review]\nmax_rounds = 0\n".into()))).unwrap_err();
        assert!(what.contains("the target epic/e is refused") && what.contains("max_rounds = 0") && fix.contains("[review]"), "{what} / {fix}");
    }

    /// B16: a fix-PASS over the owner's waiver says what that note is and where the fix goes.
    #[test]
    fn a_fix_pass_over_the_owners_waiver_names_it_and_the_way_on() {
        let owner = note("WAIVED", 0, 0, json!({"waiver": "owner ruling", "unreviewed": true}));
        for p in POLICIES {
            let (what, fix) = fix_pass(Some(&owner), &failed_rounds(1), "r1", &p, never).unwrap_err();
            assert!(what.contains("owner's waiver") && what.contains("no review under it"), "{what}");
            assert!(fix.contains("later commit") && fix.contains("keel preflight") && fix.contains("--waive-review"), "{fix}");
            assert!(!fix.contains("the step the writer printed"), "the writer printed no step for the owner's waiver: {fix}");
        }
    }

    #[test]
    fn the_schedule_comes_from_review_and_each_key_takes_its_default_or_is_refused_by_name() {
        let d = Policy::DEFAULT;
        assert_eq!(policy_in("[check]\nlint = \"x\"\n"), Ok(both(d)));
        assert_eq!(policy_in("[review]\nmodules = [\"src/*\"]\nrubric = \"r.md\"\n"), Ok(both(d)), "the skill's keys are known");
        for p in POLICIES {
            let all = format!(
                "[review]\nmax_rounds = {}\nwhole_range_rounds = {}\nfix_importants_from = {}\n",
                p.max_rounds, p.whole_range_rounds, p.fix_importants_from
            );
            assert_eq!(policy_in(&all), Ok(both(p)), "{all}");
            // A key left out takes its default; a default above the set cap is lowered to it.
            let cap = format!("[review]\nmax_rounds = {}\n", p.max_rounds);
            let lowered = Policy {
                max_rounds: p.max_rounds,
                whole_range_rounds: d.whole_range_rounds.min(p.max_rounds),
                fix_importants_from: d.fix_importants_from.min(p.max_rounds),
                fix_blockers_from: d.fix_blockers_from.map(|f| f.min(p.max_rounds)),
            };
            assert_eq!(policy_in(&cap), Ok(both(lowered)), "{cap}");
            let above = p.max_rounds + 1;
            for key in ["whole_range_rounds", "fix_importants_from", "fix_blockers_from"] {
                let text = format!("[review]\nmax_rounds = {}\n{key} = {above}\n", p.max_rounds);
                let e = policy_in(&text).unwrap_err();
                assert!(e.contains(&format!("[review] {key} = {above} is above max_rounds = {}", p.max_rounds)), "{text}: {e}");
            }
        }
        let e = policy_in(&format!("[review]\nwhole_range_rounds = {}\n", d.max_rounds + 1)).unwrap_err();
        let says = format!("is above max_rounds = {}, the default cap (max_rounds is not set)", d.max_rounds);
        assert!(e.contains(&says), "an explicit key is judged against the default cap when none is set, and says so: {e}");
        for (bad, says) in [
            ("[review]\nmax_rounds = 0\n", "[review] max_rounds = 0 must be a whole number of rounds, 1 or more"),
            ("[review]\nmax_rounds = -1\n", "max_rounds = -1 must be"),
            ("[review]\nwhole_range_rounds = \"3\"\n", "whole_range_rounds = \"3\" must be"),
            ("[review]\nfix_importants_from = 2.5\n", "fix_importants_from = 2.5 must be"),
            ("[review]\nmax_rounds = true\n", "max_rounds = true must be"),
            ("[review]\nmax_rounds = 99999999999\n", "max_rounds = 99999999999 is too large"),
            (
                "[review]\nmax_round = 3\n",
                "[review] max_round is not a key keel knows (known: max_rounds, whole_range_rounds, fix_importants_from, fix_blockers_from, modules, rubric; tables [review.wu], [review.epic])",
            ),
            ("review = 3\n", "review = 3 must be a table"),
            ("not toml [", "does not parse"),
        ] {
            let e = policy_in(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    /// E-CRLEVEL: the level is read from the target by one rule, the main branch's name.
    #[test]
    fn a_review_whose_target_is_main_is_an_epic_review_and_any_other_a_wu_review() {
        for main in ["main", "trunk"] {
            assert_eq!(level(main, main), Level::Epic);
            assert_eq!(level(&format!("refs/heads/{main}"), main), Level::Epic);
            for other in ["epic/e", "wu/e/a", "refs/heads/epic/e", "origin/main", "refs/remotes/origin/main", "0123456789abcdef", ":/msg", "mainline", ""] {
                if other != main {
                    assert_eq!(level(other, main), Level::Wu, "{other} under main = {main}");
                }
            }
        }
        assert_eq!(level("main", "trunk"), Level::Wu, "only the configured name is main");
    }

    /// The settings a table's lines write: every key of `p`, fix_blockers_from only when set.
    fn table(name: &str, p: &Policy) -> String {
        let fb = p.fix_blockers_from.map(|f| format!("fix_blockers_from = {f}\n")).unwrap_or_default();
        format!("[{name}]\nmax_rounds = {}\nwhole_range_rounds = {}\nfix_importants_from = {}\n{fb}", p.max_rounds, p.whole_range_rounds, p.fix_importants_from)
    }

    /// E-CRLEVEL: per key the level's table wins, then [review], then the default; an inherited value
    /// above a level's max_rounds is lowered to it, a written one is refused naming its table.
    #[test]
    fn each_level_takes_its_table_over_review_over_the_default_per_key() {
        let d = Policy::DEFAULT;
        let shared = Policy {
            max_rounds: d.max_rounds + 2,
            whole_range_rounds: d.whole_range_rounds + 1,
            fix_importants_from: d.fix_importants_from + 1,
            fix_blockers_from: Some(d.max_rounds + 2),
        };
        let wu = Policy {
            max_rounds: d.max_rounds + 4,
            whole_range_rounds: d.whole_range_rounds + 2,
            fix_importants_from: d.fix_importants_from + 3,
            fix_blockers_from: Some(d.fix_importants_from + 2),
        };
        let epic = Policy { max_rounds: 1, whole_range_rounds: 1, fix_importants_from: 1, fix_blockers_from: None };
        // A level table written whole wins over [review]; the other level keeps [review]'s.
        let text = format!("{}{}", table("review", &shared), table("review.wu", &wu));
        assert_eq!(policy_in(&text), Ok(Policies { wu, epic: shared }), "{text}");
        // A level cannot unset an inherited fix_blockers_from (TOML writes no "unset"): it is lowered to the level's cap.
        let text = format!("{}{}", table("review", &shared), table("review.epic", &epic));
        let inherits = Policy { fix_blockers_from: shared.fix_blockers_from.map(|f| f.min(epic.max_rounds)), ..epic };
        assert_eq!(policy_in(&text), Ok(Policies { wu: shared, epic: inherits }), "{text}");
        let text = format!("{}{}", table("review.wu", &wu), table("review.epic", &epic));
        assert_eq!(policy_in(&text), Ok(Policies { wu, epic }), "with no [review] table: {text}");
        // Per key: a level writing one key inherits the others from [review], then the default.
        for p in all_policies() {
            let text = format!("{}[review.wu]\nwhole_range_rounds = {}\n", table("review", &p), p.max_rounds);
            assert_eq!(policy_in(&text).unwrap(), Policies { wu: Policy { whole_range_rounds: p.max_rounds, ..p }, epic: p }, "{text}");
            let text = format!("[review.epic]\nmax_rounds = {}\n", p.max_rounds);
            let lowered = Policy {
                max_rounds: p.max_rounds,
                whole_range_rounds: d.whole_range_rounds.min(p.max_rounds),
                fix_importants_from: d.fix_importants_from.min(p.max_rounds),
                fix_blockers_from: d.fix_blockers_from.map(|f| f.min(p.max_rounds)),
            };
            assert_eq!(policy_in(&text).unwrap(), Policies { wu: d, epic: lowered }, "a default above the level's cap is lowered: {text}");
            // [review]'s values above a level's own max_rounds are lowered to it.
            let text = format!("{}[review.wu]\nmax_rounds = 1\n", table("review", &p));
            let one = Policy { max_rounds: 1, whole_range_rounds: 1, fix_importants_from: 1, fix_blockers_from: p.fix_blockers_from.map(|_| 1) };
            assert_eq!(policy_in(&text).unwrap(), Policies { wu: one, epic: p }, "{text}");
            // A level raising the cap keeps [review]'s values as written.
            let text = format!("{}[review.epic]\nmax_rounds = {}\n", table("review", &p), p.max_rounds + 3);
            assert_eq!(policy_in(&text).unwrap().epic, Policy { max_rounds: p.max_rounds + 3, ..p }, "{text}");
            // fix_blockers_from in [review] alone reaches both levels; in a level table, only that level.
            let f = p.max_rounds;
            let text = format!("[review]\nmax_rounds = {f}\nfix_blockers_from = {f}\n");
            assert!(policy_in(&text).is_ok_and(|q| q.wu.fix_blockers_from == Some(f) && q.epic.fix_blockers_from == Some(f)), "{text}");
            let text = format!("[review]\nmax_rounds = {f}\n[review.wu]\nfix_blockers_from = {f}\n");
            assert!(policy_in(&text).is_ok_and(|q| q.wu.fix_blockers_from == Some(f) && q.epic.fix_blockers_from.is_none()), "{text}");
            // A key a level table writes above that level's max_rounds is refused, naming the table and where the cap comes from.
            for level in ["wu", "epic"] {
                for key in ["whole_range_rounds", "fix_importants_from", "fix_blockers_from"] {
                    let above = p.max_rounds + 1;
                    let own = format!("[review.{level}]\nmax_rounds = {}\n{key} = {above}\n", p.max_rounds);
                    let e = policy_in(&own).unwrap_err();
                    assert!(e.contains(&format!("[review.{level}] {key} = {above} is above max_rounds = {}: no round", p.max_rounds)), "{own}: {e}");
                    let inherited = format!("[review]\nmax_rounds = {}\n[review.{level}]\n{key} = {above}\n", p.max_rounds);
                    let e = policy_in(&inherited).unwrap_err();
                    assert!(
                        e.contains(&format!("is above max_rounds = {}, [review]'s ([review.{level}] does not set max_rounds)", p.max_rounds)),
                        "{inherited}: {e}"
                    );
                    let none = format!("[review.{level}]\n{key} = {}\n", d.max_rounds + 1);
                    let e = policy_in(&none).unwrap_err();
                    assert!(
                        e.contains(&format!("[review.{level}] {key} = {} is above max_rounds = {}, the default cap", d.max_rounds + 1, d.max_rounds)),
                        "{none}: {e}"
                    );
                }
            }
        }
        for (bad, says) in [
            ("[review.wu]\nmax_rounds = 0\n", "[review.wu] max_rounds = 0 must be a whole number of rounds, 1 or more"),
            ("[review.epic]\nmax_rounds = -1\n", "[review.epic] max_rounds = -1 must be"),
            ("[review.wu]\nwhole_range_rounds = \"3\"\n", "[review.wu] whole_range_rounds = \"3\" must be"),
            ("[review.epic]\nfix_importants_from = 2.5\n", "[review.epic] fix_importants_from = 2.5 must be"),
            ("[review.wu]\nfix_blockers_from = 0\n", "[review.wu] fix_blockers_from = 0 must be"),
            ("[review]\nfix_blockers_from = true\n", "[review] fix_blockers_from = true must be"),
            ("[review.wu]\nfix_blockers_from = 99999999999\n", "[review.wu] fix_blockers_from = 99999999999 is too large"),
            ("[review.wu]\nmodules = [\"src/*\"]\n", "[review.wu] modules is not a schedule key"),
            ("[review.epic]\nrubric = \"r.md\"\n", "[review.epic] rubric is not a schedule key"),
            ("[review.wu]\nmax_round = 1\n", "[review.wu] max_round is not a schedule key"),
            ("[review.close]\nmax_rounds = 1\n", "[review.close] is not a table keel knows (known: [review.wu], [review.epic])"),
            ("[review]\nwu = 1\n", "[review] wu = 1 must be a table, [review.wu]"),
            ("[review]\nepic = \"x\"\n", "[review] epic = \"x\" must be a table, [review.epic]"),
            ("[review.wu.deep]\nmax_rounds = 1\n", "[review.wu] deep is not a schedule key"),
        ] {
            let e = policy_in(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
        // A bad value at one level refuses the file at both: one rule, whichever level reads it.
        let e = policy_in("[review.epic]\nmax_rounds = 0\n").unwrap_err();
        assert!(policy_at("epic/e", Level::Wu, Some("c"), |_| Ok(Some("[review.epic]\nmax_rounds = 0\n".into()))).unwrap_err().0.contains(&e));
    }

    /// E-CRLEVEL: from round fix_blockers_from on every counted finding is fixed under a fix-PASS, no
    /// round follows and the cap's waiver is refused; below it, and with it unset, today's rules.
    #[test]
    fn from_fix_blockers_from_on_every_finding_is_fixed_under_a_fix_pass_and_no_round_follows() {
        let (blk, imp, both_, none) =
            (note("FAIL", 2, 0, json!({})), note("FAIL", 0, 2, json!({})), note("FAIL", 1, 3, json!({})), note("FAIL", 0, 0, json!({})));
        for p in all_policies() {
            let (max, from) = (p.max_rounds, p.fix_importants_from);
            for k in 1..=max {
                let all = p.fix_blockers_from.is_some_and(|f| k >= f);
                let rk = format!("r{k}");
                // What follows each round, from the settings.
                assert_eq!(
                    then(&blk, k, &p),
                    if all {
                        Then::FixPass
                    } else if k < max {
                        Then::Round(k + 1)
                    } else {
                        Then::Cap
                    },
                    "{p:?} round {k}"
                );
                assert_eq!(then(&imp, k, &p), if all || k >= from { Then::FixPass } else { Then::Round(k + 1) }, "{p:?} round {k}");
                assert_eq!(then(&both_, k, &p), then(&blk, k, &p), "a blocker decides: {p:?} round {k}");
                assert_eq!(then(&none, k, &p), if !all && k < from { Then::Round(k + 1) } else { Then::Owner }, "{p:?} round {k}");
                assert_eq!(fixable(&blk, k, &p).is_ok(), all, "{p:?} round {k}");
                if let (Some(f), false) = (p.fix_blockers_from, all) {
                    let e = fixable(&blk, k, &p).unwrap_err();
                    assert!(e.contains(&format!("before round {f} (fix_blockers_from) a blocker needs a new round")), "{e}");
                }
                // The round after it: refused from fix_blockers_from on, naming the fix-PASS.
                let line = rounds_to(k, 1, 3);
                match next_round(&line, &p, &never) {
                    Ok(n) => assert!(!all && k < max && n.round == k + 1, "{p:?} round {k}"),
                    Err((what, fix)) => {
                        assert!(all || k == max, "{p:?} round {k}: {what}");
                        if all {
                            assert!(fix.contains(&format!("--fixes {rk}")) && !fix.contains("WAIVED"), "{p:?} round {k}: {fix}");
                            if k < max {
                                let f = p.fix_blockers_from.unwrap();
                                assert!(what.contains(&format!("from round {f} on (fix_blockers_from)")) && !what.contains("found none"), "{what}");
                            }
                        }
                    }
                }
                // The fix-PASS answers the blockers and importants; the round's note keeps them.
                let got = fix_pass(None, &line, &rk, &p, never);
                if all {
                    assert_eq!(got, Ok(Fix { round: k, blockers: 1, importants: 3, nits: 0 }), "{p:?} round {k}");
                    // A second code fix over it has no round to go to: only the owner.
                    let mut stacked = vec![fix_of("f2", &rk), fix_of("f1", &rk)];
                    stacked.extend(rounds_to(k, 1, 3));
                    let (what, fix) = fix_pass(None, &stacked, &rk, &p, never).unwrap_err();
                    assert!(what.contains("second fix") && fix.contains("--waive-review") && !fix.contains("WAIVED "), "{what} / {fix}");
                    let (what, fix) = next_round(&stacked, &p, &never).unwrap_err();
                    assert!(what.contains("already answers it") && fix.contains("--waive-review"), "{what} / {fix}");
                } else {
                    let (_, fix) = got.unwrap_err();
                    assert!(fix.contains(&format!("round {}", k + 1)) || fix.contains("--verdict WAIVED"), "{p:?} round {k}: {fix}");
                }
                // The cap's waiver: only on the last round's blocker, and never from fix_blockers_from on.
                let got = cap(Some(&blk), &failed_rounds(k - 1), &p, "0123456789ab");
                match (k == max, all) {
                    (true, false) => assert_eq!(got, Ok(format!("review cap: {max} rounds, 2 blockers open"))),
                    (_, true) => {
                        let (what, fix) = got.unwrap_err();
                        assert!(what.contains("waiver is refused on 0123456789") && what.contains("fix_blockers_from"), "{what}");
                        assert!(fix.contains("--verdict PASS --fixes 0123456789") && !fix.contains("WAIVED"), "{fix}");
                    }
                    (false, false) => assert!(got.unwrap_err().0.contains("cap is not reached"), "{p:?} round {k}"),
                }
                let step = next_step(&blk, k, &p, "0123456789ab");
                if all {
                    assert!(
                        step.contains("--verdict PASS --fixes 0123456789") && step.contains("no cap waiver") && !step.contains("--verdict WAIVED"),
                        "{step}"
                    );
                } else if k == max {
                    assert!(step.contains("review cap reached"), "{step}");
                }
            }
        }
    }

    /// Invariant over every schedule with fix_blockers_from: no round follows a round from it on,
    /// whatever that round found, and a fix-PASS answers every counted finding there.
    #[test]
    fn no_round_follows_a_round_from_fix_blockers_from_on_whatever_it_found() {
        for m in 1..=5u32 {
            for f in 1..=m + 1 {
                for i in 1..=m {
                    let text = format!("[review]\nmax_rounds = {m}\nfix_importants_from = {i}\nfix_blockers_from = {f}\n");
                    let Ok(p) = policy_in(&text) else {
                        assert!(f > m, "{text}");
                        continue;
                    };
                    let p = p.wu;
                    for done in 1..=m {
                        for (b, imp) in [(0, 1), (1, 0), (2, 2)] {
                            let line = rounds_to(done, b, imp);
                            let follows = next_round(&line, &p, &never).is_ok();
                            assert_eq!(follows, done < m && done < f && (done < i || b > 0), "{text} after round {done} ({b}, {imp})");
                            assert_eq!(
                                fix_pass(None, &line, &format!("r{done}"), &p, never).is_ok(),
                                done >= f || (b == 0 && done >= i),
                                "{text} round {done}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// E-CRLEVEL: the sign-off history names the blockers a fix-PASS answers, from the round's own note.
    #[test]
    fn the_history_tag_of_a_fix_pass_names_the_blockers_it_answers() {
        let findings = json!({"findings": [
            {"location": "src/a.rs:2", "claim": "a() is wrong", "severity": "blocker"},
            {"location": "src/a.rs:9", "claim": "b() leaks", "severity": "blocker", "waived": "owner: B7"},
            {"location": "src/a.rs:4", "claim": "slow", "severity": "important"}]});
        for p in all_policies() {
            let line = vec![fix_of("f1", "r1"), at("r1", note("FAIL", 1, 1, findings.clone()))];
            let got = tags(&line, &p, |_, _| Ok(vec![b"src/a.rs".to_vec()]));
            assert_eq!(got[0], " [fix-PASS of round 1 on r1; answers 1 blockers (src/a.rs:2: a() is wrong); no reviewers ran; changed: src/a.rs]", "{p:?}");
            let bare = vec![fix_of("f1", "r1"), fail("r1", 2, 0)];
            assert!(tags(&bare, &p, |_, _| Ok(vec![]))[0].contains("; answers 2 blockers (its note names none);"), "{p:?}");
            let imp = vec![fix_of("f1", "r1"), fail("r1", 0, 2)];
            assert!(!tags(&imp, &p, |_, _| Ok(vec![]))[0].contains("blockers"), "a fix-PASS of importants names no blocker");
        }
    }

    /// E-CRLEVEL.W1 review round 1, B1: a round's counts sum past u32 never
    /// wrap: from fix_blockers_from on a fix-PASS answers them, then() never reads Cap and the cap's
    /// waiver is refused, whatever the counts the writer took.
    #[test]
    fn counts_at_the_edge_of_u32_never_wrap_into_the_caps_waiver() {
        for p in FB_POLICIES {
            let f = p.fix_blockers_from.unwrap();
            for k in f..=p.max_rounds {
                for (b, i) in [(u32::MAX, 1), (u32::MAX, u32::MAX), (1, u32::MAX), (u32::MAX, 0)] {
                    let n = note("FAIL", b, i, json!({}));
                    assert_eq!(fixable(&n, k, &p), Ok(()), "{p:?} round {k} ({b}, {i})");
                    assert_eq!(then(&n, k, &p), Then::FixPass, "{p:?} round {k} ({b}, {i})");
                    let (what, _) = cap(Some(&n), &failed_rounds(k - 1), &p, "0123456789ab").unwrap_err();
                    assert!(what.contains("waiver is refused"), "{p:?} round {k} ({b}, {i}): {what}");
                }
            }
        }
    }

    /// E-CRLEVEL.W1 review round 1, N1: a round from fix_blockers_from on that failed with nothing
    /// counted gets no fix-PASS, and the refusal of the next round says so, naming only the owner.
    #[test]
    fn a_round_from_fix_blockers_from_with_nothing_counted_is_never_said_to_be_fixed_under_a_fix_pass() {
        for p in FB_POLICIES {
            let f = p.fix_blockers_from.unwrap();
            for k in f..p.max_rounds {
                let (what, fix) = next_round(&rounds_to(k, 0, 0), &p, &never).unwrap_err();
                assert!(what.contains(&format!("round {k} on r{k} counted no finding a fix-PASS could answer")), "{p:?} round {k}: {what}");
                assert!(!what.contains("are fixed under a fix-PASS") && fix.contains("--waive-review") && !fix.contains("--fixes"), "{what} / {fix}");
            }
        }
    }
}
