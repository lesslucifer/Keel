//! The review schedule (UC-REV-3): rounds counted from the cr notes on a branch's own first-parent
//! line, the scope each round reviews, which fixes need no new round (a fix-PASS), and the cap.
//! Derived from the notes every time, never typed: a fresh session counts what the branch recorded.
use crate::notes::{CrNote, OWNER_WAIVES};
use crate::{git, git_raw, ok_text};
use std::collections::HashMap;
use std::path::Path;

/// The cap when the target's committed config sets none.
pub(crate) const DEFAULT_MAX_ROUNDS: u32 = 5;
/// Rounds up to this one review the whole range; later rounds only the last fix.
const WHOLE_RANGE_ROUNDS: u32 = 2;
/// From this round on importants are fixed under a fix-PASS, and only a blocker earns another round.
const FIX_IMPORTANTS_FROM: u32 = 4;

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

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

/// The cr notes on the first-parent commits of `base..tip`, newest first, in three git processes
/// whatever the length of the line: `rev-list` names the commits, `notes list` every note's blob, one
/// `cat-file --batch` prints the line's blobs. Plumbing only, so no display config changes a note.
/// A note git cannot print, or that does not parse, is an entry of its own, never dropped.
pub(crate) fn line(repo: &Path, base: &str, tip: &str) -> Result<Vec<Entry>, String> {
    let commits = git(repo, &["rev-list", "--first-parent", &format!("{base}..{tip}")], &[])?;
    let listed = git(repo, &["notes", "--ref=cr", "list"], &[])?;
    let blobs: HashMap<&str, &str> = listed.lines().filter_map(|l| l.split_once(' ')).map(|(blob, commit)| (commit, blob)).collect();
    let on_line: Vec<(&str, &str)> = commits.lines().filter_map(|c| blobs.get(c).map(|b| (c, *b))).collect();
    if on_line.is_empty() {
        return Ok(vec![]);
    }
    let input: String = on_line.iter().map(|(_, b)| format!("{b}\n")).collect();
    let args = ["cat-file", "--batch"];
    let out = git_raw(repo, &args, Some(input.as_bytes()))?;
    if !out.status.success() {
        ok_text(&args, out)?;
        return Err("keel: git cat-file --batch failed".into());
    }
    let bodies = parse_batch(&out.stdout, on_line.len());
    Ok(on_line
        .into_iter()
        .zip(bodies)
        .map(|((c, _), body)| Entry {
            commit: c.to_string(),
            note: body.and_then(|b| serde_json::from_str(&b).map_err(|e| format!("it does not parse as cr/v1 ({e})"))),
        })
        .collect())
}

/// `git cat-file --batch` output, one result per object asked for: its bytes as text, or why not.
fn parse_batch(mut out: &[u8], n: usize) -> Vec<Result<String, String>> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let Some(nl) = out.iter().position(|&b| b == b'\n') else {
            v.push(Err("git printed nothing for it".into()));
            continue;
        };
        let header = String::from_utf8_lossy(&out[..nl]).into_owned();
        out = &out[nl + 1..];
        let fields: Vec<&str> = header.split(' ').collect();
        let size = match fields[..] {
            [_, _, size] => size.parse::<usize>().ok().filter(|s| out.len() > *s),
            _ => None,
        };
        match (&fields[..], size) {
            ([_, "blob", _], Some(s)) => v.push(String::from_utf8(out[..s].to_vec()).map_err(|_| "its blob is not UTF-8".into())),
            ([_, kind, _], Some(_)) => v.push(Err(format!("it names a {kind}, not a blob"))),
            ([_, "missing"], _) => v.push(Err("its blob is missing from the object store".into())),
            _ => {
                v.push(Err(format!("git printed '{header}' for it")));
                out = &[];
                continue;
            }
        }
        if let Some(s) = size {
            out = &out[s + 1..];
        }
    }
    v
}

/// `[review] max_rounds` in the target's committed `.keel/config.toml`: the policy a branch is
/// judged by, never one it or the working tree carries. Unset, or no committed config: the default.
pub(crate) fn max_rounds(repo: &Path, target: &str) -> Result<u32, String> {
    match git(repo, &["cat-file", "blob", &format!("{target}:.keel/config.toml")], &[]) {
        Ok(text) => max_rounds_in(&text),
        Err(_) => Ok(DEFAULT_MAX_ROUNDS),
    }
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

impl Next {
    /// The note's `scope` field.
    pub(crate) fn scope(&self) -> String {
        self.since.as_ref().map_or("full".to_string(), |s| format!("since {s}"))
    }

    /// Preflight's line: the round and the exact range the reviewers read (full SHAs).
    pub(crate) fn announce(&self, base: &str, tip: &str) -> String {
        match &self.since {
            None => format!("review round {} of {} - scope: the whole range, {base}..{tip}", self.round, self.max),
            Some(s) => format!("review round {} of {} - scope: the last fix, {s}..{tip} (the code paths; reviewers may read any file, and report only on these changes)", self.round, self.max),
        }
    }
}

/// The round the tip's review would be, from `earlier` (the line below the tip, newest first: the
/// tip's own note never counts, so a re-run on the same commit is the same round). Refused past the
/// cap, and from round 5 unless the round before found a blocker.
pub(crate) fn next_round(earlier: &[Entry], max: u32) -> Result<Next, (String, String)> {
    let rounds = rounds(earlier)?;
    let n = rounds.len() as u32 + 1;
    if n > max {
        return Err((
            format!("no review round {n} runs: the cap is {max} rounds, and round {max} ran on {}", short(rounds[0].0)),
            format!("the review stopped. A blocker in the last round ends in the cap's waiver on its commit (keel notes write cr --verdict WAIVED); otherwise {OWNER_WAIVES}"),
        ));
    }
    if n > FIX_IMPORTANTS_FROM && rounds[0].1.blockers == 0 {
        let (c, _) = rounds[0];
        return Err((
            format!("review round {n} runs only after round {} found a blocker, and round {} on {} found none", n - 1, n - 1, short(c)),
            format!("fix its importants and record them without a round: keel notes write cr --verdict PASS --fixes {c}   (on the fix commit, with nothing else changed after it)"),
        ));
    }
    let since = (n > WHOLE_RANGE_ROUNDS).then(|| rounds[0].0.to_string());
    Ok(Next { round: n, max, since })
}

/// The tip's schedule for preflight: the line, minus the tip's own note, under the target's cap.
pub(crate) fn schedule(repo: &Path, base: &str, tip: &str, target: &str) -> Result<Next, (String, String)> {
    let again = |e: String| (e, "fix the repo state, then preflight again".to_string());
    let earlier: Vec<Entry> = line(repo, base, tip).map_err(again)?.into_iter().filter(|e| e.commit != tip).collect();
    next_round(&earlier, max_rounds(repo, target).map_err(again)?)
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
        ("FAIL", ..) => Err(format!("round {round} failed with no finding counted (keel check red?); that needs a new round")),
        (v, ..) => Err(format!("round {round} ended {v}; there is nothing a fix-PASS can answer")),
    }
}

/// What a fix-PASS answers.
#[derive(Debug, PartialEq)]
pub(crate) struct Fix {
    pub round: u32,
    pub importants: u32,
    pub nits: usize,
}

/// A fix-PASS on the tip answering the round on `fixes`: only the latest round below the tip, only
/// findings `fixable` allows, and never on top of another fix-PASS (a second fix needs a round).
pub(crate) fn fix_pass(earlier: &[Entry], fixes: &str, max: u32) -> Result<Fix, (String, String)> {
    let rounds = rounds(earlier)?;
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
    if let Some(e) = earlier.iter().take_while(|e| e.commit != latest).find(|e| e.note.as_ref().is_ok_and(|n| kind(n) == Kind::FixPass)) {
        return Err((
            format!("the fix-PASS on {} already answers round {round}; a second fix needs a new round", short(&e.commit)),
            format!("run review round {} on the tip", round + 1),
        ));
    }
    fixable(note, round, max).map_err(|why| (why, format!("fix the findings, then run review round {} on the fix", round + 1)))?;
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
        Some(n) if kind(n) == Kind::Round && n.verdict == "FAIL" && n.blockers > 0 && round == max => Ok(cap_reason(max, n.blockers)),
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

/// What the schedule says after a reviewed round is recorded on `tip`.
pub(crate) fn next_step(note: &CrNote, round: u32, max: u32, tip: &str) -> String {
    let t = short(tip);
    if note.verdict == "PASS" {
        return format!("done. A later commit that only fixes this round's nits: keel notes write cr --verdict PASS --fixes {t}   (no new round)");
    }
    if fixable(note, round, max).is_ok() {
        return format!("fix the importants, then on the fix commit: keel notes write cr --verdict PASS --fixes {t}   (no new round)");
    }
    if round >= max && note.blockers > 0 {
        return format!(
            "review cap reached: round {round} of {max} found {} blockers; the review stops here. Record the round's findings on this note, then: keel notes write cr --verdict WAIVED   (the tool writes the reason: {}); the owner decides at sign-off",
            note.blockers,
            cap_reason(max, note.blockers)
        );
    }
    if round >= max || (round >= FIX_IMPORTANTS_FROM && note.blockers == 0) {
        return format!("no round {} follows; {OWNER_WAIVES}", round + 1);
    }
    format!("fix the findings (a correctness bug gets its failing test first), then review round {} on the fix commit", round + 1)
}

/// The history's tag per entry (newest first, as `line` gives them): `[round n, full]`,
/// `[round n, since <commit>]`, `[fix-PASS of round n on <commit>; no reviewers ran; changed: ...]`,
/// or nothing. Rounds are numbered from the oldest; `files` lists what a fix-PASS commit changed.
pub(crate) fn tags(entries: &[Entry], files: impl Fn(&str, &str) -> Result<Vec<String>, String>) -> Vec<String> {
    let mut numbered: HashMap<&str, u32> = HashMap::new();
    let mut previous: Option<&str> = None;
    let mut tags: Vec<String> = vec![];
    for e in entries.iter().rev() {
        let tag = match &e.note {
            Ok(n) if kind(n) == Kind::Round => {
                let round = numbered.len() as u32 + 1;
                numbered.insert(&e.commit, round);
                let scope = match previous {
                    Some(p) if round > WHOLE_RANGE_ROUNDS => format!("since {}", short(p)),
                    _ => "full".into(),
                };
                previous = Some(&e.commit);
                format!(" [round {round}, {scope}]")
            }
            Ok(n) if kind(n) == Kind::FixPass => {
                let reviewed = n.fixes().unwrap_or("?");
                let of = numbered.get(reviewed).map_or("a round not on this line".to_string(), |r| format!("round {r}"));
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
    use super::{cap, fix_pass, fixable, max_rounds_in, next_round, next_step, parse_batch, tags, Entry, Fix, Kind, Next};
    use crate::notes::CrNote;
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

    fn fix_of(commit: &str, reviewed: &str) -> Entry {
        at(commit, note("PASS", 0, 0, json!({"fixes": reviewed})))
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
        assert_eq!(super::kind(&note("PASS", 0, 0, json!({"fixes": "r1"}))), Kind::FixPass);
        assert_eq!(super::kind(&note("PASS", 0, 0, json!({"fixes": " "}))), Kind::Round, "a blank fixes answers nothing");
        assert_eq!(super::kind(&note("WAIVED", 0, 0, json!({"waiver": "r", "unreviewed": true}))), Kind::Other);
    }

    #[test]
    fn the_round_counts_the_reviews_below_the_tip_and_rounds_3_on_review_the_last_fix() {
        assert_eq!(next_round(&[], 5), Ok(Next { round: 1, max: 5, since: None }));
        let two = vec![fail("r2", 1, 0), fix_of("f1", "r1"), at("w", note("WAIVED", 0, 0, json!({"waiver": "x", "unreviewed": true}))), fail("r1", 0, 1)];
        assert_eq!(next_round(&two, 5), Ok(Next { round: 3, max: 5, since: Some("r2".into()) }), "a fix-PASS or an unreviewed waiver is no round");
        assert_eq!(next_round(&two[1..], 5).unwrap().since, None, "round 2 reviews the whole range");
        assert_eq!(next_round(&failed_rounds(4), 5), Ok(Next { round: 5, max: 5, since: Some("r4".into()) }));
        let n = Next { round: 3, max: 5, since: Some("s".into()) };
        assert_eq!((n.scope(), Next { round: 1, max: 5, since: None }.scope()), ("since s".to_string(), "full".to_string()));
        assert!(n.announce("b", "t").starts_with("review round 3 of 5 - scope: the last fix, s..t"));
        assert_eq!(Next { round: 2, max: 5, since: None }.announce("b", "t"), "review round 2 of 5 - scope: the whole range, b..t");
    }

    #[test]
    fn round_5_follows_only_a_blocker_and_nothing_follows_the_cap() {
        let mut no_blocker = failed_rounds(4);
        no_blocker[0] = fail("r4", 0, 2);
        let (what, fix) = next_round(&no_blocker, 5).unwrap_err();
        assert!(what.contains("round 5 runs only after round 4 found a blocker") && fix.contains("--fixes r4"), "{what} / {fix}");
        let (what, fix) = next_round(&failed_rounds(5), 5).unwrap_err();
        assert!(what.contains("no review round 6") && fix.contains("--verdict WAIVED") && fix.contains("--waive-review"), "{what} / {fix}");
        assert_eq!(next_round(&failed_rounds(5), 6).unwrap().round, 6, "max_rounds moves the cap");
        let mut six = failed_rounds(5);
        six[0] = fail("r5", 0, 1);
        assert!(next_round(&six, 6).is_err(), "from round 4 on, only a blocker earns another round, whatever the cap");
        assert!(next_round(&failed_rounds(2), 2).is_err() && next_round(&failed_rounds(1), 2).is_ok());
        let unreadable = vec![fail("r2", 1, 0), Entry { commit: "0123456789ab".into(), note: Err("it does not parse".into()) }];
        let (what, fix) = next_round(&unreadable, 5).unwrap_err();
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
        assert_eq!(fix_pass(&line, "r1", 5), Ok(Fix { round: 1, importants: 0, nits: 2 }));
        let mut four = failed_rounds(4);
        four[0] = fail("r4", 0, 3);
        assert_eq!(fix_pass(&four, "r4", 5), Ok(Fix { round: 4, importants: 3, nits: 0 }));
        assert!(fix_pass(&four, "r3", 5).unwrap_err().0.contains("latest review round on this branch is round 4 on r4"));
        assert!(fix_pass(&[], "r1", 5).is_err(), "nothing to answer");
        let mut stacked = vec![fix_of("f4", "r4")];
        stacked.extend(four);
        let (what, fix) = fix_pass(&stacked, "r4", 5).unwrap_err();
        assert!(what.contains("fix-PASS on f4 already answers round 4") && fix.contains("round 5"), "{what}");
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
        assert_eq!(got, [" [fix-PASS of round 3 on r3; no reviewers ran; changed: r3->f3]", " [round 3, since r2]", "", " [round 2, full]", "", " [round 1, full]"]);
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

    #[test]
    fn a_batch_read_gives_each_blob_or_why_not_in_order() {
        let out = b"aaa blob 5\nhello\nbbb missing\nccc tree 3\nxyz\nddd blob 2\n\xff\xfe\n";
        let got = parse_batch(out, 4);
        assert_eq!(got[0], Ok("hello".to_string()));
        assert!(got[1].as_ref().unwrap_err().contains("missing"));
        assert!(got[2].as_ref().unwrap_err().contains("tree"));
        assert!(got[3].as_ref().unwrap_err().contains("UTF-8"));
        let short = parse_batch(b"aaa blob 50\nhello\n", 2);
        assert!(short.iter().all(Result::is_err) && short.len() == 2, "a truncated read never invents a note");
        assert_eq!(parse_batch(b"", 1).len(), 1);
    }
}
