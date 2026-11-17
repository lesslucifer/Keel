//! `keel ci gate`'s evidence page: the Markdown job summary GitHub shows on the gate's run
//! (E-GHFLOW.W1). A view, never a second truth: each check's row renders what the gate recorded as
//! it ran (`gate::Judged`, its failures and waivers by check id), and the notes are read through
//! `notes::parse`, the one schema rule.
use crate::gate::{self, Failure, Judged, Waiver, HOOKS, SIGN_OFF};
use crate::notes::{self, CrNote, DocNote, DodNote, Identity, Layer, MrNote, TestNote};
use crate::short;

/// The notes the page lists, in its order.
pub(crate) const NOTE_KINDS: [&str; 5] = ["test", "doc", "cr", "mr", "dod"];

/// The longest field the page shows, in characters as the printout shows them; GitHub keeps at most
/// 1 MiB of one step's summary.
pub(crate) const FIELD_CAP: usize = 500;
/// The most failure rows, and waived lines, the page shows; the log has every one.
pub(crate) const ROW_CAP: usize = 20;
/// The most lines of one fix the page shows.
const FIX_LINES: usize = 6;

/// What the page shows: the gate's own record. `checks` are the level's check ids; `judged`,
/// `failures` and `waived` are the gate's, never escaped yet (a fix keeps its own line breaks);
/// `notes` are each kind's stored note as `notes::raw` read it.
pub(crate) struct Evidence<'a> {
    pub level: &'a str,
    pub tip: &'a str,
    pub base: &'a str,
    pub target: &'a str,
    /// The range changes no code path (UC-GATE-5).
    pub exempt: bool,
    pub checks: &'a [&'a str],
    pub judged: &'a [(&'a str, Judged)],
    pub failures: &'a [Failure],
    pub waived: &'a [Waiver],
    pub notes: &'a [(&'a str, Result<Option<String>, String>)],
}

/// The gate on GitHub is red when any check but the sign-off failed (by the id each failure counts
/// against: a seam's sign-off is the walk's).
pub(crate) fn red_before_sign_off<'a>(failed: impl IntoIterator<Item = &'a str>) -> bool {
    failed.into_iter().any(|c| c != SIGN_OFF)
}

/// What the gate on GitHub says of a tip's review that did not pass (E-GHFLOW2.W7).
pub(crate) const REVIEW_LEFT: &str = "the review did not pass: the owner's approval decides at the sign-off";

/// The gate on GitHub (E-GHFLOW2.W7): red when any check but the sign-off failed, the tip's own
/// review excepted - an approval signs over it, so it is decided with the sign-off, after this run.
pub(crate) fn red_on_github(failures: &[Failure]) -> bool {
    red_before_sign_off(failures.iter().filter(|f| !f.review).map(|f| f.id))
}

/// Recorded text as Markdown that renders as the same text and nothing else: made inert as the
/// gate's printout makes it (`gate::inert`), cut to `FIELD_CAP` of those characters with the count
/// it dropped, then every ASCII punctuation character becomes a numeric entity, so no link,
/// autolink, mention, emphasis, table cell, math or tag can form.
pub(crate) fn md(s: &str) -> String {
    let shown = gate::inert(s, false);
    let mut out = String::new();
    let mut chars = shown.chars();
    for c in chars.by_ref().take(FIELD_CAP) {
        if c.is_ascii_punctuation() {
            out += &format!("&#{};", c as u32);
        } else {
            out.push(c);
        }
    }
    let dropped = chars.count();
    if dropped > 0 {
        out += &format!(" … ({dropped} more characters)");
    }
    out
}

/// A stored note as its type, or why not, worded as check 1 words it.
fn typed<T: serde::de::DeserializeOwned>(raw: &str, kind: &str) -> Result<T, String> {
    notes::parse(raw, kind).map_err(|e| notes::invalid(kind, &e))
}

fn layer(name: &str, l: &Option<Layer>) -> Option<String> {
    Some(match l.as_ref()? {
        Layer::Waived { waived } => format!("{name} waived: {waived}"),
        Layer::Counted { count, covers, .. } => format!("{name} {count} tests ({covers})"),
    })
}

/// One note's identity and key fields, as plain (unescaped) text.
fn fields(kind: &str, raw: &str) -> Result<(Identity, String), String> {
    Ok(match kind {
        "test" => {
            let n: TestNote = typed(raw, kind)?;
            let layers: Vec<String> =
                [("unit", &n.unit), ("integration", &n.integration), ("e2e", &n.e2e)].iter().filter_map(|(name, l)| layer(name, l)).collect();
            (n.id, if layers.is_empty() { "no layer".into() } else { layers.join("; ") })
        }
        "doc" => {
            let n: DocNote = typed(raw, kind)?;
            let shown: Vec<String> = n.changes.iter().take(10).map(|c| format!("{} ({})", c.path, c.tier)).collect();
            let more = if n.changes.len() > 10 { format!(", and {} more", n.changes.len() - 10) } else { String::new() };
            let text = format!("{} docs: {}{more}", n.changes.len(), shown.join(", "));
            (n.id, text)
        }
        "cr" => {
            let n: CrNote = typed(raw, kind)?;
            let mut s = format!("verdict {}", n.verdict);
            if let Some(fixes) = n.fixes() {
                s += &format!(", fixes {}", short(fixes));
            }
            if let Some(round) = n.schedule.round {
                s += &format!(", round {round}");
            }
            if let Some(scope) = &n.schedule.scope {
                s += &format!(", scope {scope}");
            }
            s += &if n.unreviewed() { ", not reviewed".to_string() } else { format!(", blockers {}, importants {}", n.blockers, n.importants) };
            if let Some(w) = n.waiver() {
                s += &format!(", waiver: {w}");
            }
            (n.id, s)
        }
        "mr" => {
            let n: MrNote = typed(raw, kind)?;
            let mut s = format!("signer {} (tier {}), level {}", n.signer, n.tier, n.level);
            if let Some(r) = &n.report {
                s += &format!(", report {r}");
            }
            (n.id, s)
        }
        "dod" => {
            let n: DodNote = typed(raw, kind)?;
            let review = n.review.as_deref().map(|r| format!(", review {r}")).unwrap_or_default();
            let text = format!("level {}, {} checks{review}", n.level, n.checks.len());
            (n.id, text)
        }
        _ => return Err(format!("no reader for {kind} notes")),
    })
}

/// One row of the notes table: kind, whether it names this tip, its fields (escaped).
fn note_row(kind: &str, tip: &str, stored: &Result<Option<String>, String>) -> String {
    let (on_tip, what) = match stored {
        Ok(None) => ("-".to_string(), "none".to_string()),
        Err(e) => ("-".to_string(), format!("unreadable: {}", md(e))),
        Ok(Some(raw)) => match fields(kind, raw) {
            Err(e) => ("-".to_string(), md(&e)),
            Ok((id, text)) => {
                let on_tip = if id.tip == tip { "yes".to_string() } else { format!("no: names {}", md(short(&id.tip))) };
                (on_tip, format!("{kind}&#47;v1 · {}", md(&text)))
            }
        },
    };
    format!("| {kind} | {on_tip} | {what} |")
}

/// One check's row, from the gate's record alone: red when it failed; "not run" or "incomplete"
/// when the gate recorded an input missing; waived when it recorded a waiver; green only when it
/// recorded the check as run and nothing else; "not run" when it recorded nothing.
fn state(e: &Evidence, id: &str) -> String {
    let red = e.failures.iter().filter(|f| f.id == id).count();
    let mut ran = false;
    let mut missing = None;
    for (_, judged) in e.judged.iter().filter(|(c, _)| *c == id) {
        match judged {
            Judged::Ran => ran = true,
            Judged::Missing(why) => missing = missing.or(Some(why)),
        }
    }
    match (red, missing) {
        (1.., _) if id == SIGN_OFF => "waiting: no valid sign-off yet (not judged here)".into(),
        (1.., _) => format!("red ({red})"),
        (0, Some(why)) if ran => format!("incomplete: {}", md(why)),
        (0, Some(why)) => format!("not run: {}", md(why)),
        (0, None) if !ran && e.exempt => "not run: docs/infra-only range".into(),
        (0, None) if !ran => "not run".into(),
        (0, None) if e.waived.iter().any(|w| w.check == id) => "waived".into(),
        (0, None) => "green".into(),
    }
}

/// The job summary for one gate run.
pub(crate) fn render(e: &Evidence) -> String {
    let verdict = if red_on_github(e.failures) {
        "red".to_string()
    } else if e.failures.is_empty() {
        "green on every check".to_string()
    } else if e.failures.iter().any(|f| f.review) {
        "green on every check but the review, which did not pass: the owner's approval decides at the sign-off".to_string()
    } else {
        format!("green on every check before {SIGN_OFF}, which is judged at the merge")
    };
    let range = if e.exempt { " - a docs/infra-only range" } else { "" };
    let mut out =
        format!("## Keel gate: {} {verdict}{range}\n\nTip `{}` over base `{}` (merge-base with {}).\n\n", e.level, short(e.tip), short(e.base), md(e.target));
    out += "| Check | State |\n|---|---|\n";
    let rows: Vec<&str> = std::iter::once(HOOKS).chain(e.checks.iter().copied()).collect();
    for id in &rows {
        out += &format!("| {} | {} |\n", md(id), state(e, id));
    }
    let mut unknown: Vec<&str> = vec![];
    for f in e.failures.iter().filter(|f| !rows.contains(&f.id)) {
        if !unknown.contains(&f.id) {
            unknown.push(f.id);
        }
    }
    for id in unknown {
        out += &format!("| {} | {} |\n", md(id), state(e, id));
    }
    out += "\n| Note | Names this tip | Fields |\n|---|---|---|\n";
    for (kind, stored) in e.notes {
        out += &note_row(kind, e.tip, stored);
        out.push('\n');
    }
    if !e.waived.is_empty() {
        out += "\n**Waived**\n\n";
        for w in e.waived.iter().take(ROW_CAP) {
            out += &format!("- {}\n", md(&w.text));
        }
        if e.waived.len() > ROW_CAP {
            out += &format!("- and {} more waived lines - the log lists them all\n", e.waived.len() - ROW_CAP);
        }
    }
    if !e.failures.is_empty() {
        out += "\n| Failed check | What | Fix |\n|---|---|---|\n";
        for f in e.failures.iter().take(ROW_CAP) {
            let mut lines: Vec<String> = f.fix.split('\n').take(FIX_LINES).map(|l| md(l.trim())).collect();
            if f.fix.split('\n').nth(FIX_LINES).is_some() {
                lines.push("…".into());
            }
            out += &format!("| {} | {} | {} |\n", md(&f.check), md(&f.what), lines.join("<br>"));
        }
        if e.failures.len() > ROW_CAP {
            out += &format!("| … | and {} more failures - the log lists them all | |\n", e.failures.len() - ROW_CAP);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{md, red_before_sign_off, red_on_github, render, Evidence, NOTE_KINDS};
    use crate::gate::{Failure, Judged, Waiver, DIFF_BINDING, DOC_NOTE, HOOKS, MANIFEST, NOTES, REVIEW, REVIEW_BASE, SIGN_OFF, WALK};

    const TIP: &str = "1111111111222222222233333333334444444444";
    const BASE: &str = "aaaaaaaaaabbbbbbbbbbccccccccccdddddddddd";
    const WU: [&str; 7] = [NOTES, MANIFEST, REVIEW, REVIEW_BASE, DIFF_BINDING, DOC_NOTE, SIGN_OFF];
    const EPIC: [&str; 8] = [NOTES, MANIFEST, REVIEW, REVIEW_BASE, DIFF_BINDING, DOC_NOTE, WALK, SIGN_OFF];

    /// A note's identity fields, naming `tip`, for a JSON object body.
    fn id(tip: &str) -> String {
        format!(r#""tip":"{tip}","base":"{BASE}","branch":null,"committer":"c","timestamp":0"#)
    }

    fn notes(test: Option<String>) -> Vec<(&'static str, Result<Option<String>, String>)> {
        NOTE_KINDS.iter().map(|k| (*k, Ok(if *k == "test" { test.clone() } else { None }))).collect()
    }

    fn failure(id: &'static str, what: &str, fix: &str) -> Failure {
        Failure { id, check: id.into(), what: what.into(), fix: fix.into(), review: false }
    }

    fn waiver(check: &'static str, text: &str) -> Waiver {
        Waiver { check, text: text.into() }
    }

    /// The record of a gate that ran every check of `checks` and guard D.
    fn ran_all(checks: &[&'static str]) -> Vec<(&'static str, Judged)> {
        std::iter::once(HOOKS).chain(checks.iter().copied()).map(|id| (id, Judged::Ran)).collect()
    }

    fn page_with(judged: &[(&str, Judged)], failures: &[Failure], waived: &[Waiver], exempt: bool, notes: &[(&str, Result<Option<String>, String>)]) -> String {
        render(&Evidence { level: "wu", tip: TIP, base: BASE, target: "epic/e", exempt, checks: &WU, judged, failures, waived, notes })
    }

    fn page(failures: &[Failure], waived: &[Waiver], notes: &[(&str, Result<Option<String>, String>)]) -> String {
        page_with(&ran_all(&WU), failures, waived, false, notes)
    }

    fn row<'a>(page: &'a str, first: &str) -> &'a str {
        page.lines().find(|l| l.starts_with(&format!("| {first} |"))).unwrap_or_else(|| panic!("no row {first} in:\n{page}"))
    }

    fn cr_note(verdict: &str, extra: &str) -> String {
        format!(r#"{{"schema":"cr/v1",{},"verdict":"{verdict}","blockers":0,"importants":0,"code_diff_hash":"h"{extra}}}"#, id(TIP))
    }

    #[test]
    fn md_turns_every_ascii_punctuation_into_an_entity_and_keeps_letters() {
        assert_eq!(md("plain text 42"), "plain text 42");
        assert_eq!(md("café 日本"), "café 日本");
        assert_eq!(md("7·walk·3·review"), "7·walk·3·review", "the check ids' middle dot stays, as in the printout");
        for c in "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~".chars() {
            assert_eq!(md(&c.to_string()).chars().filter(|x| x.is_ascii_punctuation() && *x != '&' && *x != '#' && *x != ';').count(), 0, "{c}");
            assert!(md(&c.to_string()).starts_with("&#"), "{c} -> {}", md(&c.to_string()));
        }
        assert_eq!(md("|"), "&#124;");
        assert_eq!(md("a\nb"), "a&#92;u&#123;a&#125;b", "a line break is made inert first, then escaped");
        let hostile = "x | y\n| fake | row |\n[link](https://e.vil) @owner <b>$x$</b> www.e.vil";
        let out = md(hostile);
        for bad in ["|", "\n", "[", "](", "@", "<", ">", "$", "://", "www."] {
            assert!(!out.contains(bad), "{bad} survived in {out}");
        }
    }

    /// GFM decodes no entity inside a code span, so the target, made inert by `md`, must stand
    /// outside one to render as its own name (epic code review round 1).
    #[test]
    fn the_target_in_the_header_renders_as_its_branch_name() {
        let target = "epic/E-GHFLOW";
        let page = render(&Evidence {
            level: "wu",
            tip: TIP,
            base: BASE,
            target,
            exempt: false,
            checks: &WU,
            judged: &ran_all(&WU),
            failures: &[],
            waived: &[],
            notes: &[],
        });
        let header = page.lines().find(|l| l.starts_with("Tip ")).unwrap();
        assert_eq!(header, "Tip `1111111111` over base `aaaaaaaaaa` (merge-base with epic&#47;E&#45;GHFLOW).");
        let spans: Vec<&str> = header.split('`').skip(1).step_by(2).collect();
        assert!(spans.iter().all(|s| !s.contains("&#")), "an entity inside a code span shows as its source: {header}");
    }

    #[test]
    fn hostile_sizes_keep_the_page_under_githubs_1_mib_step_limit() {
        let huge = "\u{7}|".repeat(20_000);
        let mut n: Vec<(&str, Result<Option<String>, String>)> = vec![("test", Err(huge.clone()))];
        let note = serde_json::json!({"schema": "mr/v1", "tip": huge, "base": huge, "branch": huge, "committer": huge, "timestamp": 0,
            "signer": huge, "tier": huge, "level": huge, "report": huge, "change_hash": huge})
        .to_string();
        n.extend(["doc", "cr", "mr", "dod"].map(|k| (k, Ok(Some(note.clone())))));
        let fix = format!("{huge}\n").repeat(10);
        let failures: Vec<Failure> =
            (0..60).map(|_| Failure { id: WALK, check: format!("7·walk·{huge}"), what: huge.clone(), fix: fix.clone(), review: false }).collect();
        let waived: Vec<Waiver> = (0..60).map(|_| waiver(REVIEW, &huge)).collect();
        let judged: Vec<(&str, Judged)> = WU.iter().map(|id| (*id, Judged::Missing(huge.clone()))).collect();
        let p = page_with(&judged, &failures, &waived, false, &n);
        assert!(p.len() < 1 << 20, "{} bytes", p.len());
        assert!(p.contains("and 40 more failures - the log lists them all"), "only the first rows are shown");
        assert!(p.contains("and 40 more waived lines - the log lists them all"));
        assert!(p.contains("<br>…"), "a long fix shows its first lines");
        assert_eq!(md(&"a".repeat(600)), format!("{} … (100 more characters)", "a".repeat(500)), "a capped field says how much it dropped");
        assert_eq!(md(&"a".repeat(500)), "a".repeat(500));
        assert_eq!(
            md(&"\u{7}".repeat(200)),
            format!("{} … (500 more characters)", "&#92;u&#123;7&#125;".repeat(100)),
            "the cap counts what the printout shows"
        );
    }

    #[test]
    fn only_a_failure_outside_the_sign_off_turns_the_gate_red() {
        let review = |id| Failure { review: true, ..failure(id, "w", "f") };
        assert!(!red_on_github(&[review(NOTES), review(REVIEW), failure(SIGN_OFF, "w", "f")]), "the review is left to the approval");
        assert!(red_on_github(&[review(REVIEW), failure(NOTES, "w", "f")]), "any other check is not");
        assert!(red_on_github(&[failure(REVIEW, "w", "f")]), "a seam's or an unmarked review failure is not");
        assert!(!red_before_sign_off([]));
        assert!(!red_before_sign_off([SIGN_OFF]));
        assert!(red_before_sign_off([SIGN_OFF, NOTES]));
        assert!(red_before_sign_off([WALK]), "a seam's sign-off counts against the walk");
        assert!(red_before_sign_off([HOOKS]));
    }

    #[test]
    fn a_wu_waiting_for_its_sign_off_is_green_before_it_and_lists_every_note() {
        let test = format!(r#"{{"schema":"test/v1",{},"integration":{{"count":3,"covers":"a works","files":[],"languages":{{}}}}}}"#, id(TIP));
        let p = page(&[failure(SIGN_OFF, "tip has no mr/v1 note", "keel mr confirm")], &[], &notes(Some(test)));
        assert!(p.starts_with("## Keel gate: wu green on every check before 8·manual-review, which is judged at the merge\n"), "{p}");
        assert!(p.contains("Tip `1111111111` over base `aaaaaaaaaa`"), "{p}");
        assert_eq!(row(&p, "0·hooks"), "| 0·hooks | green |", "the middle dot is not ASCII: kept as is");
        assert_eq!(row(&p, "5·diff&#45;binding"), "| 5·diff&#45;binding | green |");
        assert!(row(&p, "8·manual&#45;review").contains("waiting"), "{p}");
        assert_eq!(row(&p, "test"), "| test | yes | test&#47;v1 · integration 3 tests &#40;a works&#41; |");
        for kind in ["doc", "cr", "mr", "dod"] {
            assert_eq!(row(&p, kind), format!("| {kind} | - | none |"));
        }
        assert!(p.contains("| Failed check | What | Fix |"));
    }

    #[test]
    fn a_red_page_names_each_failure_escaped_and_counts_it_against_its_check() {
        let f = [
            failure(NOTES, "no cr/v1 note on 1111111111", "run the review\n      then | write it"),
            Failure { id: WALK, check: "7·walk·3·review".into(), what: "[wu abc] verdict is FAIL".into(), fix: "fix it".into(), review: false },
        ];
        let judged = ran_all(&EPIC);
        let p = render(&Evidence {
            level: "epic",
            tip: TIP,
            base: BASE,
            target: "main",
            exempt: false,
            checks: &EPIC,
            judged: &judged,
            failures: &f,
            waived: &[],
            notes: &notes(None),
        });
        assert!(p.starts_with("## Keel gate: epic red\n"), "{p}");
        assert_eq!(row(&p, "1·notes"), "| 1·notes | red (1) |");
        assert_eq!(row(&p, "3·review"), "| 3·review | green |", "the seam's review failure is the walk's");
        assert_eq!(row(&p, "7·walk"), "| 7·walk | red (1) |");
        let fail = p.lines().find(|l| l.starts_with("| 1·notes | no cr")).unwrap();
        assert_eq!(fail, "| 1·notes | no cr&#47;v1 note on 1111111111 | run the review<br>then &#124; write it |");
        assert!(p.contains("| 7·walk·3·review | &#91;wu abc&#93; verdict is FAIL | fix it |"), "{p}");
    }

    #[test]
    fn an_unknown_failing_check_shows_as_a_red_row() {
        let p = page(&[failure("9·new-check", "x", "y"), failure("9·new-check", "z", "y")], &[], &notes(None));
        assert!(p.starts_with("## Keel gate: wu red\n"), "{p}");
        assert_eq!(row(&p, "9·new&#45;check"), "| 9·new&#45;check | red (2) |", "{p}");
        assert_eq!(p.matches("| 9·new&#45;check | red").count(), 1, "one row per unknown check");
    }

    #[test]
    fn a_row_is_green_only_when_the_gate_recorded_its_check_as_run() {
        let judged = vec![
            (HOOKS, Judged::Ran),
            (NOTES, Judged::Ran),
            (MANIFEST, Judged::Ran),
            (MANIFEST, Judged::Missing("no readable test note".into())),
            (REVIEW, Judged::Missing("no readable cr note".into())),
            (DOC_NOTE, Judged::Ran),
        ];
        let p = page_with(&judged, &[failure(NOTES, "no test/v1 note", "write it")], &[], false, &notes(None));
        assert_eq!(row(&p, "2·manifest"), "| 2·manifest | incomplete: no readable test note |");
        assert_eq!(row(&p, "3·review"), "| 3·review | not run: no readable cr note |");
        assert_eq!(row(&p, "4·review&#45;base"), "| 4·review&#45;base | not run |", "no record is never green");
        assert_eq!(row(&p, "6·doc&#45;note"), "| 6·doc&#45;note | green |");
        assert_eq!(row(&p, "1·notes"), "| 1·notes | red (1) |");
    }

    #[test]
    fn checks_a_docs_only_range_did_not_record_say_not_run_and_the_rest_say_what_they_found() {
        let judged = vec![(HOOKS, Judged::Ran), (SIGN_OFF, Judged::Ran)];
        let p = page_with(&judged, &[failure(SIGN_OFF, "no sign-off", "sign")], &[], true, &notes(None));
        assert!(p.starts_with("## Keel gate: wu green on every check before 8·manual-review, which is judged at the merge - a docs/infra-only range\n"), "{p}");
        assert_eq!(row(&p, "1·notes"), "| 1·notes | not run: docs/infra-only range |");
        assert_eq!(row(&p, "6·doc&#45;note"), "| 6·doc&#45;note | not run: docs/infra-only range |");
        assert_eq!(row(&p, "0·hooks"), "| 0·hooks | green |");
        let p = page_with(&judged, &[], &[], true, &notes(None));
        assert!(p.starts_with("## Keel gate: wu green on every check - a docs/infra-only range\n"), "a judged sign-off is not 'judged at the merge': {p}");
        assert_eq!(row(&p, "8·manual&#45;review"), "| 8·manual&#45;review | green |");
    }

    #[test]
    fn a_waiver_marks_the_check_it_names_whatever_its_line_says() {
        let p = page(&[], &[waiver(REVIEW, "the owner waived it, reworded"), waiver(WALK, "seam abc: safe")], &notes(None));
        assert!(p.starts_with("## Keel gate: wu green on every check\n"), "{p}");
        assert_eq!(row(&p, "3·review"), "| 3·review | waived |");
        assert_eq!(row(&p, "4·review&#45;base"), "| 4·review&#45;base | green |", "3·review's waiver is not 4's");
        assert!(p.contains("**Waived**\n\n- the owner waived it&#44; reworded\n"), "{p}");
        assert!(!p.contains("Failed check"));
    }

    #[test]
    fn a_cr_note_shows_a_waiver_only_when_waived_and_says_not_reviewed_when_no_review_ran() {
        let mut n = notes(None);
        n[2] = ("cr", Ok(Some(cr_note("WAIVED", r#","waiver":"owner ruling","unreviewed":true"#))));
        let p = page(&[], &[], &n);
        let cr = row(&p, "cr");
        assert!(cr.contains("not reviewed") && cr.contains("waiver&#58; owner ruling"), "{cr}");
        assert!(!cr.contains("blockers"), "an unreviewed waiver's counts mean nothing: {cr}");
        n[2] = ("cr", Ok(Some(cr_note("PASS", r#","waiver":"stray","round":2,"scope":"full""#))));
        let p = page(&[], &[], &n);
        assert_eq!(row(&p, "cr"), "| cr | yes | cr&#47;v1 · verdict PASS&#44; round 2&#44; scope full&#44; blockers 0&#44; importants 0 |");
    }

    #[test]
    fn a_note_that_is_missing_unreadable_invalid_or_names_another_tip_says_so_as_check_1_does() {
        let mut n = notes(None);
        n[1] = ("doc", Err("bad | blob".into()));
        n[2] = ("cr", Ok(Some("{oops".into())));
        n[3] = ("mr", Ok(Some(format!(r#"{{"schema":"mr/v1",{},"signer":"[x](y)","tier":"username","level":"wu","change_hash":"h"}}"#, id("9999999999999")))));
        n[4] = ("dod", Ok(Some(r#"{"schema":"dod/v1","level":"wu","checks":["a","b"]}"#.into())));
        let p = page(&[], &[], &n);
        assert_eq!(row(&p, "test"), "| test | - | none |");
        assert_eq!(row(&p, "doc"), "| doc | - | unreadable: bad &#124; blob |");
        assert!(row(&p, "cr").starts_with("| cr | - | not valid cr&#47;v1&#58; key must be a string"), "{p}");
        assert_eq!(row(&p, "mr"), "| mr | no: names 9999999999 | mr&#47;v1 · signer &#91;x&#93;&#40;y&#41; &#40;tier username&#41;&#44; level wu |");
        assert!(row(&p, "dod").starts_with("| dod | - | not valid dod&#47;v1&#58; missing field "), "{p}");
        n[4] = ("dod", Ok(Some(r#"{"schema":"dod/v2"}"#.into())));
        assert_eq!(row(&page(&[], &[], &n), "dod"), "| dod | - | not valid dod&#47;v1&#58; declares schema dod&#47;v2 |", "the expected schema is named once");
    }

    #[test]
    fn note_fields_cover_waived_layers_doc_lists_reports_and_receipts() {
        let test =
            format!(r#"{{"schema":"test/v1",{},"unit":{{"count":2,"covers":"c","files":[],"languages":{{}}}},"e2e":{{"waived":"no harness"}}}}"#, id(TIP));
        let docs: Vec<String> = (0..12).map(|i| format!(r#"{{"path":"docs/{i}.html","tier":"T3"}}"#)).collect();
        let doc = format!(r#"{{"schema":"doc/v1",{},"changes":[{}]}}"#, id(TIP), docs.join(","));
        let mr = format!(r#"{{"schema":"mr/v1",{},"signer":"vu","tier":"username","level":"wu","report":"docs/r.html","change_hash":"h"}}"#, id(TIP));
        let dod = format!(r#"{{"schema":"dod/v1",{},"level":"wu","checks":["notes","manifest"],"diff_hash":"h","review":"WAIVED"}}"#, id(TIP));
        let n = vec![("test", Ok(Some(test))), ("doc", Ok(Some(doc))), ("mr", Ok(Some(mr))), ("dod", Ok(Some(dod)))];
        let p = page(&[], &[], &n);
        assert!(row(&p, "test").ends_with("unit 2 tests &#40;c&#41;&#59; e2e waived&#58; no harness |"), "{p}");
        assert!(row(&p, "doc").contains("12 docs&#58; docs&#47;0&#46;html &#40;T3&#41;"), "{p}");
        assert!(row(&p, "doc").ends_with("&#44; and 2 more |"), "{p}");
        assert!(!row(&p, "doc").contains("docs&#47;10&#46;html"), "{p}");
        assert!(row(&p, "mr").ends_with("level wu&#44; report docs&#47;r&#46;html |"), "{p}");
        assert_eq!(row(&p, "dod"), "| dod | yes | dod&#47;v1 · level wu&#44; 2 checks&#44; review WAIVED |");
    }
}
