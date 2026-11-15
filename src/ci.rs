//! `keel ci gate`'s evidence page: the Markdown job summary GitHub shows on the gate's run
//! (E-GHFLOW.W1). A view of the notes and the gate's verdict, never a second truth.
use crate::{mr, short};
use serde_json::Value;

/// The notes the page lists, in its order.
pub(crate) const NOTE_KINDS: [&str; 5] = ["test", "doc", "cr", "mr", "dod"];

/// The check the gate on GitHub does not judge: the sign-off (W4 turns it into its own status).
pub(crate) const SIGN_OFF: &str = "8·manual-review";

/// What the page shows. `failures` are (check, what, fix) as recorded, never escaped yet (a fix
/// keeps its own line breaks); `notes` are each kind's stored note as `notes::raw` read it.
pub(crate) struct Evidence<'a> {
    pub level: &'a str,
    pub tip: &'a str,
    pub base: &'a str,
    pub target: &'a str,
    pub exempt: bool,
    pub checks: &'a [&'a str],
    pub failures: &'a [(String, String, String)],
    pub waived: &'a [String],
    pub notes: &'a [(&'a str, Result<Option<String>, String>)],
}

/// The receipt name of a check (`Level::checks`) as the gate's output labels it.
pub(crate) fn label(check: &str) -> String {
    let n = match check {
        "hooks" => 0,
        "notes" => 1,
        "manifest" => 2,
        "review" => 3,
        "review-base" => 4,
        "diff-binding" => 5,
        "doc-note" => 6,
        "walk" => 7,
        "manual-review" => 8,
        _ => return check.to_string(),
    };
    format!("{n}·{check}")
}

/// Whether a failure printed as `failed` is one of `label`'s: the label itself, or a walk seam's
/// failure nested under it (`7·walk·3·review` is check 7's, not check 3's).
pub(crate) fn belongs(failed: &str, label: &str) -> bool {
    failed == label || failed.strip_prefix(label).is_some_and(|rest| rest.starts_with('·'))
}

/// The gate on GitHub is red when any check but the sign-off failed.
pub(crate) fn red_before_sign_off<'a>(failed: impl IntoIterator<Item = &'a str>) -> bool {
    failed.into_iter().any(|c| c != SIGN_OFF)
}

/// Recorded text as Markdown that renders as the same text and nothing else: made inert as the
/// gate's printout makes it (`mr::printable`, the check ids' middle dot kept), then every ASCII
/// punctuation character becomes a numeric entity, so no link, autolink, mention, emphasis, table
/// cell, math or tag can form.
pub(crate) fn md(s: &str) -> String {
    let entities = |part: &str| -> String {
        mr::printable(part).chars().map(|c| if c.is_ascii_punctuation() { format!("&#{};", c as u32) } else { c.to_string() }).collect()
    };
    s.split('·').map(entities).collect::<Vec<_>>().join("·")
}

fn str_of<'v>(v: &'v Value, key: &str) -> Option<&'v str> {
    v.get(key).and_then(Value::as_str)
}

fn num_of(v: &Value, key: &str) -> String {
    v.get(key).map(|n| n.to_string()).unwrap_or_else(|| "?".into())
}

/// One note's key fields, as plain (unescaped) text.
fn fields(kind: &str, v: &Value) -> String {
    match kind {
        "test" => {
            let layers: Vec<String> = ["unit", "integration", "e2e"]
                .iter()
                .filter_map(|layer| {
                    let l = v.get(*layer).filter(|l| l.is_object())?;
                    Some(match str_of(l, "waived") {
                        Some(why) => format!("{layer} waived: {why}"),
                        None => format!("{layer} {} tests ({})", num_of(l, "count"), str_of(l, "covers").unwrap_or("")),
                    })
                })
                .collect();
            if layers.is_empty() {
                "no layer".into()
            } else {
                layers.join("; ")
            }
        }
        "doc" => {
            let changes = v.get("changes").and_then(Value::as_array).cloned().unwrap_or_default();
            let shown: Vec<String> =
                changes.iter().take(10).map(|c| format!("{} ({})", str_of(c, "path").unwrap_or("?"), str_of(c, "tier").unwrap_or("?"))).collect();
            let more = if changes.len() > 10 { format!(", and {} more", changes.len() - 10) } else { String::new() };
            format!("{} docs: {}{more}", changes.len(), shown.join(", "))
        }
        "cr" => {
            let mut s = format!("verdict {}", str_of(v, "verdict").unwrap_or("?"));
            for key in ["round", "scope"] {
                if let Some(x) = v.get(key).filter(|x| !x.is_null()) {
                    s += &format!(", {key} {}", x.as_str().map(String::from).unwrap_or_else(|| x.to_string()));
                }
            }
            s += &format!(", blockers {}, importants {}", num_of(v, "blockers"), num_of(v, "importants"));
            if let Some(w) = str_of(v, "waiver") {
                s += &format!(", waiver: {w}");
            }
            s
        }
        "mr" => {
            let mut s = format!(
                "signer {} (tier {}), level {}",
                str_of(v, "signer").unwrap_or("?"),
                str_of(v, "tier").unwrap_or("?"),
                str_of(v, "level").unwrap_or("?")
            );
            if let Some(r) = str_of(v, "report") {
                s += &format!(", report {r}");
            }
            s
        }
        "dod" => format!("level {}, {} checks", str_of(v, "level").unwrap_or("?"), v.get("checks").and_then(Value::as_array).map_or(0, Vec::len)),
        _ => String::new(),
    }
}

/// One row of the notes table: kind, whether it names this tip, its fields (escaped).
fn note_row(kind: &str, tip: &str, stored: &Result<Option<String>, String>) -> String {
    let (on_tip, what) = match stored {
        Ok(None) => ("-".to_string(), "none".to_string()),
        Err(e) => ("-".to_string(), format!("unreadable: {}", md(e))),
        Ok(Some(raw)) => match serde_json::from_str::<Value>(raw) {
            Err(_) => ("-".to_string(), "not JSON".to_string()),
            Ok(v) => {
                let on_tip = match str_of(&v, "tip") {
                    Some(t) if t == tip => "yes".to_string(),
                    Some(t) => format!("no: names {}", md(short(t))),
                    None => "no: names no tip".to_string(),
                };
                let schema = str_of(&v, "schema").map(md).unwrap_or_else(|| "no schema".into());
                (on_tip, format!("{schema} · {}", md(&fields(kind, &v))))
            }
        },
    };
    format!("| {kind} | {on_tip} | {what} |")
}

/// The job summary for one gate run.
pub(crate) fn render(e: &Evidence) -> String {
    let failed: Vec<&str> = e.failures.iter().map(|(c, _, _)| c.as_str()).collect();
    let red = red_before_sign_off(failed.iter().copied());
    let verdict = match (red, e.exempt, failed.is_empty()) {
        (true, _, _) => "red",
        (false, true, _) => "exempt (docs/infra only), sign-off judged at the merge",
        (false, false, true) => "green on every check",
        (false, false, false) => "green on checks 0–7, the sign-off is judged at the merge",
    };
    let mut out =
        format!("## Keel gate: {} {verdict}\n\nTip `{}` over base `{}` (merge-base with `{}`).\n\n", e.level, short(e.tip), short(e.base), md(e.target));
    out += "| Check | State |\n|---|---|\n";
    for check in std::iter::once("hooks").chain(e.checks.iter().copied()) {
        let label = label(check);
        let mine = failed.iter().filter(|f| belongs(f, &label)).count();
        let state = if mine > 0 && label == SIGN_OFF {
            "waiting: no valid sign-off yet (not judged here)".to_string()
        } else if mine > 0 {
            format!("red ({mine})")
        } else if e.waived.iter().any(|w| w.starts_with(&label)) {
            "waived".to_string()
        } else if e.exempt && check != "hooks" && check != "manual-review" {
            "exempt".to_string()
        } else {
            "green".to_string()
        };
        out += &format!("| {} | {state} |\n", md(&label));
    }
    out += "\n| Note | Names this tip | Fields |\n|---|---|---|\n";
    for (kind, stored) in e.notes {
        out += &note_row(kind, e.tip, stored);
        out.push('\n');
    }
    if !e.waived.is_empty() {
        out += "\n**Waived**\n\n";
        for w in e.waived {
            out += &format!("- {}\n", md(w));
        }
    }
    if !e.failures.is_empty() {
        out += "\n| Failed check | What | Fix |\n|---|---|---|\n";
        for (check, what, fix) in e.failures {
            let fix = fix.split('\n').map(|l| md(l.trim())).collect::<Vec<_>>().join("<br>");
            out += &format!("| {} | {} | {fix} |\n", md(check), md(what));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{belongs, label, md, red_before_sign_off, render, Evidence, NOTE_KINDS};

    const TIP: &str = "1111111111222222222233333333334444444444";
    const BASE: &str = "aaaaaaaaaabbbbbbbbbbccccccccccdddddddddd";
    const WU: [&str; 7] = ["notes", "manifest", "review", "review-base", "diff-binding", "doc-note", "manual-review"];

    fn notes(test: Option<String>) -> Vec<(&'static str, Result<Option<String>, String>)> {
        NOTE_KINDS.iter().map(|k| (*k, Ok(if *k == "test" { test.clone() } else { None }))).collect()
    }

    fn failure(check: &str, what: &str, fix: &str) -> (String, String, String) {
        (check.into(), what.into(), fix.into())
    }

    fn page(failures: &[(String, String, String)], waived: &[String], exempt: bool, notes: &[(&str, Result<Option<String>, String>)]) -> String {
        render(&Evidence { level: "wu", tip: TIP, base: BASE, target: "epic/e", exempt, checks: &WU, failures, waived, notes })
    }

    fn row<'a>(page: &'a str, first: &str) -> &'a str {
        page.lines().find(|l| l.starts_with(&format!("| {first} |"))).unwrap_or_else(|| panic!("no row {first} in:\n{page}"))
    }

    #[test]
    fn md_turns_every_ascii_punctuation_into_an_entity_and_keeps_letters() {
        assert_eq!(md("plain text 42"), "plain text 42");
        assert_eq!(md("café 日本"), "café 日本");
        assert_eq!(md("7·walk·3·review"), "7·walk·3·review", "the check ids' middle dot stays");
        assert_eq!(md("·\u{b7}"), "··", "only the middle dot, as the printout keeps it");
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

    #[test]
    fn labels_carry_the_gates_check_ids_and_a_walk_failure_is_check_7s() {
        assert_eq!(label("hooks"), "0·hooks");
        assert_eq!(label("doc-note"), "6·doc-note");
        assert_eq!(label("manual-review"), "8·manual-review");
        assert!(belongs("7·walk·3·review", "7·walk"));
        assert!(belongs("7·walk", "7·walk"));
        assert!(!belongs("7·walk·3·review", "3·review"));
        assert!(!belongs("3·review-base", "3·review"), "a prefix of another id is not that check");
    }

    #[test]
    fn only_a_failure_outside_the_sign_off_turns_the_gate_red() {
        assert!(!red_before_sign_off([]));
        assert!(!red_before_sign_off(["8·manual-review"]));
        assert!(red_before_sign_off(["8·manual-review", "1·notes"]));
        assert!(red_before_sign_off(["7·walk·8·manual-review"]), "a seam's sign-off is part of the walk");
        assert!(red_before_sign_off(["0·hooks"]));
    }

    #[test]
    fn a_wu_waiting_for_its_sign_off_is_green_on_0_to_7_and_lists_every_note() {
        let test = format!(r#"{{"schema":"test/v1","tip":"{TIP}","integration":{{"count":3,"covers":"a works","files":[],"languages":{{}}}}}}"#);
        let p = page(&[failure("8·manual-review", "tip has no mr/v1 note", "keel mr confirm")], &[], false, &notes(Some(test)));
        assert!(p.starts_with("## Keel gate: wu green on checks 0–7"), "{p}");
        assert!(p.contains("Tip `1111111111` over base `aaaaaaaaaa`"), "{p}");
        assert_eq!(row(&p, "0·hooks"), "| 0·hooks | green |", "the middle dot is not ASCII: kept as is");
        assert_eq!(row(&p, "5·diff&#45;binding"), "| 5·diff&#45;binding | green |");
        assert!(row(&p, "8·manual&#45;review").contains("waiting"), "{p}");
        assert_eq!(row(&p, "test"), "| test | yes | test&#47;v1 · integration 3 tests &#40;a works&#41; |");
        for kind in ["doc", "cr", "mr", "dod"] {
            assert_eq!(row(&p, kind), format!("| {kind} | - | none |"));
        }
        assert!(row(&p, "8·manual&#45;review").starts_with("| 8·"));
        assert!(p.contains("| Failed check | What | Fix |"));
    }

    #[test]
    fn a_red_page_names_each_failure_escaped_and_counts_it_on_its_check() {
        let f = [
            failure("1·notes", "no cr/v1 note on 1111111111", "run the review\n      then | write it"),
            failure("7·walk·3·review", "[wu abc] verdict is FAIL", "fix it"),
        ];
        let p = page(&f, &[], false, &notes(None));
        assert!(p.starts_with("## Keel gate: wu red\n"), "{p}");
        assert_eq!(row(&p, "1·notes"), "| 1·notes | red (1) |");
        assert_eq!(row(&p, "3·review"), "| 3·review | green |", "the seam's review failure is the walk's");
        assert_eq!(row(&p, "1·notes").matches('|').count(), 3);
        let fail = p.lines().find(|l| l.starts_with("| 1·notes | no cr")).unwrap();
        assert_eq!(fail, "| 1·notes | no cr&#47;v1 note on 1111111111 | run the review<br>then &#124; write it |");
        assert!(p.contains("| 7·walk·3·review | &#91;wu abc&#93; verdict is FAIL | fix it |"), "{p}");
    }

    #[test]
    fn an_exempt_range_and_a_waived_review_show_as_such() {
        let p = page(&[failure("8·manual-review", "no sign-off", "sign")], &[], true, &notes(None));
        assert!(p.starts_with("## Keel gate: wu exempt"), "{p}");
        assert_eq!(row(&p, "1·notes"), "| 1·notes | exempt |");
        assert_eq!(row(&p, "0·hooks"), "| 0·hooks | green |", "the hooks guard is never exempt");
        let p = page(&[], &["3·review: owner ruling".into()], false, &notes(None));
        assert!(p.starts_with("## Keel gate: wu green on every check"), "{p}");
        assert_eq!(row(&p, "3·review"), "| 3·review | waived |");
        assert!(p.contains("**Waived**\n\n- 3·review&#58; owner ruling\n"), "{p}");
        assert!(!p.contains("Failed check"));
    }

    #[test]
    fn a_note_that_is_missing_unreadable_not_json_or_names_another_tip_says_so() {
        let mut n = notes(None);
        n[1] = ("doc", Err("bad | blob".into()));
        n[2] = ("cr", Ok(Some("{oops".into())));
        n[3] = ("mr", Ok(Some(r#"{"schema":"mr/v1","tip":"9999999999999","signer":"[x](y)","tier":"username","level":"wu"}"#.into())));
        n[4] = ("dod", Ok(Some(r#"{"schema":"dod/v1","level":"wu","checks":["a","b"]}"#.into())));
        let p = page(&[], &[], false, &n);
        assert_eq!(row(&p, "test"), "| test | - | none |");
        assert_eq!(row(&p, "doc"), "| doc | - | unreadable: bad &#124; blob |");
        assert_eq!(row(&p, "cr"), "| cr | - | not JSON |");
        assert_eq!(row(&p, "mr"), "| mr | no: names 9999999999 | mr&#47;v1 · signer &#91;x&#93;&#40;y&#41; &#40;tier username&#41;&#44; level wu |");
        assert_eq!(row(&p, "dod"), "| dod | no: names no tip | dod&#47;v1 · level wu&#44; 2 checks |");
    }

    #[test]
    fn note_fields_cover_waived_layers_doc_lists_and_review_rounds() {
        let test = format!(r#"{{"schema":"test/v1","tip":"{TIP}","unit":{{"count":2,"covers":"c"}},"e2e":{{"waived":"no harness"}}}}"#);
        let docs: Vec<String> = (0..12).map(|i| format!(r#"{{"path":"docs/{i}.html","tier":"T3"}}"#)).collect();
        let doc = format!(r#"{{"schema":"doc/v1","tip":"{TIP}","changes":[{}]}}"#, docs.join(","));
        let cr = format!(r#"{{"schema":"cr/v1","tip":"{TIP}","verdict":"WAIVED","round":2,"scope":"full","blockers":0,"importants":1,"waiver":"cap"}}"#);
        let mr = format!(r#"{{"schema":"mr/v1","tip":"{TIP}","signer":"vu","tier":"username","level":"wu","report":"docs/r.html"}}"#);
        let n = vec![("test", Ok(Some(test))), ("doc", Ok(Some(doc))), ("cr", Ok(Some(cr))), ("mr", Ok(Some(mr)))];
        let p = page(&[], &[], false, &n);
        assert!(row(&p, "test").ends_with("unit 2 tests &#40;c&#41;&#59; e2e waived&#58; no harness |"), "{p}");
        assert!(row(&p, "doc").contains("12 docs&#58; docs&#47;0&#46;html &#40;T3&#41;"), "{p}");
        assert!(row(&p, "doc").ends_with("&#44; and 2 more |"), "{p}");
        assert!(!row(&p, "doc").contains("docs&#47;10&#46;html"), "{p}");
        assert!(row(&p, "cr").ends_with("verdict WAIVED&#44; round 2&#44; scope full&#44; blockers 0&#44; importants 1&#44; waiver&#58; cap |"), "{p}");
        assert!(row(&p, "mr").ends_with("level wu&#44; report docs&#47;r&#46;html |"), "{p}");
    }
}
