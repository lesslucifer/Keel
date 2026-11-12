//! `keel verify-doc`: htmldoc checklist, relative links, quoted diagram labels (UC-DOC-1/2/4).
//! ponytail: hand-rolled tag scanner, not an HTML parser; fine for htmldoc's
//! well-formed output, swap in a real parser if docs start using odd markup.
use std::path::Path;

const EPIC_PLAN: &[&str] = &["intent", "owner-changes", "use-cases", "expected", "non-goals", "wu-summaries", "e2e-plan", "distillation-plan", "open-questions"];
const WU_PLAN: &[&str] = &["requirement", "maps-to", "expected", "test-plan"];
const WU_RESULT: &[&str] = &["outcome", "deviations", "evidence", "distillation", "follow-ups"];
const EPIC_RESULT: &[&str] = &["outcome", "wu-rollup", "deviations", "evidence", "distillation", "follow-ups"];
const WU_SUB: &[&str] = &["wu-summary", "wu-maps-to", "wu-expected", "wu-deps"];
const WU_RESULT_ROLE: &str = "wu-result";
/// Words allowed in an epic plan's top: its main content above the first WU.
const TOP_CAP: usize = 300;
const EPIC_RESULT_ROLE: &str = "epic-result";

struct Tag<'a> {
    name: &'a str,
    attrs: &'a str,
    start: usize,
    end: usize,
    closing: bool,
}

fn tags(html: &str) -> Vec<Tag<'_>> {
    let mut out = vec![];
    let mut i = 0;
    while let Some(p) = html[i..].find('<') {
        let start = i + p;
        let Some(q) = html[start..].find('>') else { break };
        let end = start + q + 1;
        let body = &html[start + 1..end - 1];
        i = end;
        if body.starts_with('!') || body.starts_with('?') {
            continue;
        }
        let (closing, body) = body.strip_prefix('/').map_or((false, body), |r| (true, r));
        let n = body.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(body.len());
        if n > 0 {
            out.push(Tag { name: &body[..n], attrs: &body[n..], start, end, closing });
        }
    }
    out
}

fn attr<'a>(t: &Tag<'a>, key: &str) -> Option<&'a str> {
    let pat = format!(" {key}=\"");
    let s = t.attrs.find(&pat)? + pat.len();
    Some(&t.attrs[s..s + t.attrs[s..].find('"')?])
}

/// (start, end) byte range of the inner html of the element opened by ts[i].
fn span(ts: &[Tag], i: usize) -> Option<(usize, usize)> {
    let mut depth = 0;
    for t in &ts[i..] {
        if t.name != ts[i].name {
            continue;
        }
        depth += if t.closing { -1 } else { 1 };
        if depth == 0 {
            return Some((ts[i].end, t.start));
        }
    }
    None
}

fn filled(html: &str, ts: &[Tag], i: usize) -> bool {
    let Some((a, b)) = span(ts, i) else { return false };
    let mut in_tag = false;
    html[a..b].chars().any(|c| match c {
        '<' => { in_tag = true; false }
        '>' => { in_tag = false; false }
        _ => !in_tag && !c.is_whitespace(),
    })
}

fn strip_blocks(html: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut s = html.to_string();
    while let Some(a) = s.find(&open) {
        let b = s[a..].find(&close).map_or(s.len(), |b| a + b + close.len());
        s.replace_range(a..b, "");
    }
    s
}

fn scannable(raw: &str) -> String {
    strip_blocks(&strip_blocks(raw, "script"), "style")
}

fn root<'a, 'b>(ts: &'b [Tag<'a>]) -> Option<&'b Tag<'a>> {
    ts.iter().find(|t| t.name == "html" && !t.closing)
}

/// The root `<html>`'s `data-artifact` (its governed role(s)); `None` for an ungoverned doc.
pub fn artifact(raw: &str) -> Option<String> {
    let html = scannable(raw);
    let ts = tags(&html);
    attr(root(&ts)?, "data-artifact").map(String::from)
}

/// The result role a sign-off at this level attests.
pub fn result_role(epic: bool) -> &'static str {
    if epic {
        EPIC_RESULT_ROLE
    } else {
        WU_RESULT_ROLE
    }
}

/// Does the doc's root `data-artifact` carry `role`? A collapsed doc carries each of its roles.
pub fn has_role(raw: &str, role: &str) -> bool {
    artifact(raw).is_some_and(|a| a.split_whitespace().any(|r| r == role))
}

/// Problems with one doc; empty = green. Drafts are fully silent: no checklist, no link check.
pub fn verify(path: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return vec!["cannot read file".into()];
    };
    let html = scannable(&raw);
    let ts = tags(&html);
    let mut errs = vec![];

    let root = root(&ts);
    if root.is_some_and(|r| attr(r, "data-status") == Some("draft")) {
        return errs;
    }
    if let Some(root) = root {
        let status = attr(root, "data-status").unwrap_or("");
        if let Some(art) = attr(root, "data-artifact") {
            if !status.is_empty() && status != "draft" {
                checklist(&html, &ts, art, &mut errs);
            }
        }
    }

    for t in ts.iter().filter(|t| !t.closing && attr(t, "class").is_some_and(|c| c.split_whitespace().any(|c| c == "mermaid"))) {
        let inner = &html[t.end..];
        let inner = &inner[..inner.find(&format!("</{}>", t.name)).unwrap_or(inner.len())];
        for (found, fix) in unquoted_labels(&decode(inner)) {
            errs.push(format!("diagram label not in quotes: {found} - write {fix}"));
        }
    }

    let dir = path.parent().unwrap_or(Path::new("."));
    for t in ts.iter().filter(|t| t.name == "a" && !t.closing) {
        let Some(h) = attr(t, "href") else { continue };
        let file = h.split(['#', '?']).next().unwrap_or("");
        if file.is_empty() || file.contains(':') || file.starts_with('/') {
            continue; // anchors, external schemes, site-absolute: not ours to resolve
        }
        if !dir.join(file).exists() {
            errs.push(format!("dangling relative link: {h}"));
        }
    }
    errs
}

fn checklist(html: &str, ts: &[Tag], artifact: &str, errs: &mut Vec<String>) {
    // Collapsed roles ("epic-plan wu-plan"): union of the checklists, a shared tag counts once.
    let mut expected: Vec<&str> = vec![];
    for a in artifact.split_whitespace() {
        expected.extend(match a {
            "epic-plan" => EPIC_PLAN,
            "wu-plan" => WU_PLAN,
            WU_RESULT_ROLE => WU_RESULT,
            EPIC_RESULT_ROLE => EPIC_RESULT,
            _ => {
                errs.push(format!("unknown data-artifact: {a}"));
                return;
            }
        });
    }
    let epic = artifact.split_whitespace().any(|a| a == "epic-plan");
    let mut seen: Vec<&str> = vec![];
    for (i, t) in ts.iter().enumerate().filter(|(_, t)| !t.closing) {
        let Some(item) = attr(t, "data-item") else { continue };
        if expected.contains(&item) {
            if filled(html, ts, i) {
                seen.push(item);
            } else {
                errs.push(format!("item \"{item}\" is empty"));
            }
        } else if !(epic && WU_SUB.contains(&item)) {
            errs.push(format!("unknown tag data-item=\"{item}\" (typo? expected one of: {})", expected.join(", ")));
        }
    }
    for e in &expected {
        if !seen.contains(e) && !errs.iter().any(|m| m.starts_with(&format!("item \"{e}\" is empty"))) {
            errs.push(format!("missing item \"{e}\""));
        }
    }
    if epic {
        let n = top_words(html, ts);
        if n > TOP_CAP {
            errs.push(format!("epic plan top has {n} words above the first WU, over the cap of {TOP_CAP}: cut it, or move details below the work"));
        }
        let wus: Vec<usize> = (0..ts.len()).filter(|&i| !ts[i].closing && attr(&ts[i], "data-wu").is_some()).collect();
        if wus.is_empty() {
            errs.push("epic plan has no data-wu block".into());
        }
        for i in wus {
            let (a, b) = span(ts, i).unwrap_or((ts[i].end, html.len()));
            for sub in WU_SUB {
                let ok = (0..ts.len()).any(|j| {
                    !ts[j].closing && ts[j].start >= a && ts[j].start < b && attr(&ts[j], "data-item") == Some(sub) && filled(html, ts, j)
                });
                if !ok {
                    errs.push(format!("data-wu {}: missing or empty \"{sub}\"", attr(&ts[i], "data-wu").unwrap_or("?")));
                }
            }
        }
    }
}

fn decode(s: &str) -> String {
    s.replace("&gt;", ">").replace("&lt;", "<").replace("&quot;", "\"").replace("&#34;", "\"").replace("&#39;", "'").replace("&nbsp;", " ").replace("&amp;", "&")
}

/// Words in an epic plan's top: the text from the start of `<main>` (else `<body>`) up to the
/// first `data-wu` block. A word is a whitespace-separated run holding a letter or digit.
fn top_words(html: &str, ts: &[Tag]) -> usize {
    let open = |n: &str| ts.iter().find(|t| t.name == n && !t.closing).map(|t| t.end);
    let start = open("main").or_else(|| open("body")).unwrap_or(0);
    let end = ts.iter().find(|t| !t.closing && t.start >= start && attr(t, "data-wu").is_some()).map_or(html.len(), |t| t.start);
    let top = &html[start..end.max(start)];
    let mut text = String::new();
    let mut in_tag = false;
    for (i, c) in top.char_indices() {
        let opens = top[i + c.len_utf8()..].starts_with(|n: char| n.is_ascii_alphabetic() || n == '/' || n == '!');
        match c {
            '<' if !in_tag && opens => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    decode(&text).split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

const DIRECTIVES: &[&str] = &["classDef", "class", "style", "linkStyle", "click", "direction", "end", "accTitle", "accDescr"];

/// Flowchart labels not in double quotes, as (found, fix): node shapes (`A[x]`, `A(x)`, `A{x}`,
/// `A>x]` and their doubled forms) and edge labels (`-->|x|`, `-- x -->`). Only `flowchart` and
/// `graph` diagrams are scanned; comments and style lines are skipped.
fn unquoted_labels(src: &str) -> Vec<(String, String)> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%")).peekable();
    if lines.peek() == Some(&"---") {
        lines.next();
        for l in lines.by_ref() {
            if l == "---" {
                break;
            }
        }
    }
    let Some(head) = lines.next() else { return vec![] };
    let kind = head.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("");
    if kind != "flowchart" && kind != "graph" {
        return vec![];
    }
    let mut out = vec![];
    let first = head.split_once(';').map(|(_, r)| r);
    for line in first.into_iter().chain(lines) {
        let line = line.trim();
        let word = line.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
        if line.starts_with("%%") || DIRECTIVES.contains(&word) {
            continue;
        }
        scan_line(line.strip_prefix("subgraph").unwrap_or(line), &mut out);
    }
    out
}

fn is_link(c: u8) -> bool {
    matches!(c, b'-' | b'=' | b'.' | b'<' | b'>')
}

fn scan_line(l: &str, out: &mut Vec<(String, String)>) {
    let b = l.as_bytes();
    let skip_quoted = |i: usize| b[i + 1..].iter().position(|&c| c == b'"').map_or(b.len(), |q| i + q + 2);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            i = skip_quoted(i);
        } else if c == b'@' && b.get(i + 1) == Some(&b'{') {
            let mut j = i + 2;
            while j < b.len() && b[j] != b'}' {
                j = if b[j] == b'"' { skip_quoted(j) } else { j + 1 };
            }
            i = j + 1;
        } else if c == b'|' {
            let Some(k) = b[i + 1..].iter().position(|&c| c == b'|').map(|k| i + 1 + k) else { return };
            let label = l[i + 1..k].trim();
            if !label.starts_with('"') {
                out.push((format!("|{label}|"), format!("|\"{label}\"|")));
            }
            i = k + 1;
        } else if matches!(c, b'[' | b'(' | b'{') || (c == b'>' && i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) {
            let mut j = i + 1;
            while c != b'>' && j < b.len() && matches!(b[j], b'[' | b'(' | b'{') {
                j += 1;
            }
            if b[j - 1] == b'[' && j < b.len() && matches!(b[j], b'/' | b'\\') {
                j += 1;
            }
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            if j < b.len() && b[j] == b'"' {
                i = skip_quoted(j);
                continue;
            }
            let mut k = b[j..].iter().position(|c| matches!(c, b']' | b')' | b'}')).map_or(b.len(), |k| j + k);
            let mut close = k;
            while close < b.len() && matches!(b[close], b']' | b')' | b'}') {
                close += 1;
            }
            if k > j && matches!(b[k - 1], b'/' | b'\\') {
                k -= 1;
            }
            let (open, label, end) = (&l[i..j].trim_end(), l[j..k].trim(), &l[k..close]);
            out.push((format!("{open}{label}{end}"), format!("{open}\"{label}\"{end}")));
            i = close;
        } else if is_link(c) {
            let run = b[i..].iter().take_while(|&&c| is_link(c)).count();
            let arrow = l[i..i + run].trim_start_matches('<');
            let opens_text = matches!(arrow, "--" | "==" | "-.") && b.get(i + run).is_some_and(u8::is_ascii_whitespace);
            i += run;
            if opens_text {
                let Some(k) = (i..b.len().saturating_sub(1)).find(|&k| matches!(b[k], b'-' | b'=' | b'.') && matches!(b[k + 1], b'-' | b'=' | b'.' | b'>')) else { return };
                let label = l[i..k].trim();
                let end = b[k..].iter().take_while(|&&c| is_link(c)).count();
                let close = &l[k..k + end];
                if !label.starts_with('"') {
                    out.push((format!("{arrow} {label} {close}"), format!("{arrow} \"{label}\" {close}")));
                }
                i = k + end;
            }
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{artifact, has_role, result_role, tags, top_words, unquoted_labels, verify};

    #[test]
    fn a_stray_closing_html_before_the_root_neither_hides_the_checklist_nor_the_role() {
        let doc = r#"<p>see </html> below</p><html data-artifact="wu-result" data-status="final"><p data-item="outcome">done</p></html>"#;
        let path = std::env::temp_dir().join(format!("keel-stray-root-{}.html", std::process::id()));
        std::fs::write(&path, doc).unwrap();
        let errs = verify(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(artifact(doc).as_deref(), Some("wu-result"), "the doc is a wu-result report");
        assert!(errs.iter().any(|e| e == "missing item \"deviations\""), "the checklist judges the same root: {errs:?}");
    }

    #[test]
    fn artifact_reads_the_root_role_and_is_none_for_an_ungoverned_doc() {
        let doc = r#"<!doctype html><html lang="en" data-artifact="wu-result" data-status="final"><body><div data-artifact="epic-plan"></div></body></html>"#;
        assert_eq!(artifact(doc).as_deref(), Some("wu-result"));
        assert_eq!(artifact(r#"<html data-artifact="epic-result wu-result">"#).as_deref(), Some("epic-result wu-result"));
        assert_eq!(artifact("<html lang=\"en\"><p>x-result</p></html>"), None);
        assert_eq!(artifact("<p>wu a: done</p>"), None);
        assert_eq!(artifact(r#"<script>"<html data-artifact=\"wu-result\">"</script><html>"#), None);
    }

    #[test]
    fn the_result_role_a_sign_off_attests_is_one_the_checklist_knows() {
        for epic in [false, true] {
            let role = result_role(epic);
            let path = std::env::temp_dir().join(format!("keel-role-{epic}-{}.html", std::process::id()));
            std::fs::write(&path, format!(r#"<html data-artifact="{role}" data-status="final"></html>"#)).unwrap();
            let errs = verify(&path);
            std::fs::remove_file(&path).unwrap();
            assert!(errs.contains(&"missing item \"outcome\"".to_string()) && !errs.iter().any(|e| e.starts_with("unknown")), "{role}: {errs:?}");
        }
    }

    #[test]
    fn a_collapsed_result_doc_has_both_roles_and_an_ungoverned_one_none() {
        let collapsed = r#"<html data-artifact="epic-result wu-result">"#;
        assert!(has_role(collapsed, "wu-result") && has_role(collapsed, "epic-result"));
        assert!(has_role(r#"<html data-artifact="wu-result">"#, "wu-result"));
        assert!(!has_role(r#"<html data-artifact="wu-result">"#, "epic-result"));
        assert!(!has_role("<p>x-result</p>", "wu-result"));
    }

    fn labels(src: &str) -> Vec<String> {
        unquoted_labels(src).into_iter().map(|(found, _)| found).collect()
    }

    #[test]
    fn every_node_shape_is_scanned_and_a_quoted_label_passes() {
        for shape in ["[x]", "(x)", "([x])", "[[x]]", "[(x)]", "((x))", "{x}", "{{x}}", ">x]", "[/x/]", "[\\x\\]", "[/x\\]"] {
            let open = &shape[..shape.find('x').unwrap()];
            let close = &shape[shape.find('x').unwrap() + 1..];
            assert_eq!(labels(&format!("flowchart LR\n  A{shape} --> B")), vec![shape.to_string()], "{shape}");
            assert!(labels(&format!("flowchart LR\n  A{open}\"x\"{close} --> B")).is_empty(), "{shape} quoted");
        }
        assert_eq!(unquoted_labels("graph TD\n  A[Build step]"), vec![("[Build step]".into(), "[\"Build step\"]".into())]);
    }

    #[test]
    fn both_edge_label_forms_are_scanned_and_plain_arrows_are_not_labels() {
        assert_eq!(labels("flowchart LR\n  A -->|yes| B -.->|no| C"), ["|yes|", "|no|"]);
        assert_eq!(labels("flowchart LR\n  A -- yes --> B\n  B == go ==> C\n  C -. later .-> D\n  D <-- back --> A"), ["-- yes -->", "== go ==>", "-. later .->", "-- back -->"]);
        assert_eq!(unquoted_labels("flowchart LR\n  A -- yes --> B")[0].1, "-- \"yes\" -->");
        let quoted = "flowchart LR\n  A -->|\"yes\"| B -- \"no\" --> C == \"go\" ==> D -. \"x\" .-> E\n  A --> B --- C -.-> D ==> E <--> F --x G --o H";
        assert!(labels(quoted).is_empty(), "{:?}", labels(quoted));
    }

    #[test]
    fn quoted_text_hides_delimiters_and_non_label_lines_are_skipped() {
        let src = "---\ntitle: A[x]\n---\nflowchart TD;  A[\"a [b] | c -- d\"] --> B\n  %% C[comment]\n  classDef hot fill:#f96\n  style A fill:#f9f\n  class A hot\n  click A \"https://x.io\" \"tip (x)\"\n  linkStyle 0 stroke:#f00\n  direction LR\n  N@{ shape: rect, label: \"x [y]\" }\n  end";
        assert!(labels(src).is_empty(), "{:?}", labels(src));
        assert_eq!(labels("flowchart LR; A[raw]"), ["[raw]"]);
        assert_eq!(labels("flowchart LR\n  subgraph S [Title]\n  A\n  end"), ["[Title]"]);
        assert_eq!(labels("flowchart LR\n  A[W1 · the skill]"), ["[W1 · the skill]"]);
    }

    #[test]
    fn only_flowcharts_are_scanned() {
        for src in ["sequenceDiagram\n  A->>B: hi [x]", "stateDiagram-v2\n  A --> B: go [x]", "pie\n  \"a\" : 1", "", "flowchartX\n  A[x]"] {
            assert!(labels(src).is_empty(), "{src}");
        }
    }

    fn top(html: &str) -> usize {
        top_words(html, &tags(html))
    }

    #[test]
    fn the_top_runs_from_main_or_body_to_the_first_wu_and_counts_words_with_a_letter_or_digit() {
        assert_eq!(top("<body><header>one two</header><main><p>three four</p><div data-wu=\"W1\">five</div></main></body>"), 2);
        assert_eq!(top("<body><header>one two</header><p>three</p><div data-wu=\"W1\">four</div></body>"), 3);
        assert_eq!(top("<main><p>a · b — c --> | 1</p></main>"), 4);
        assert_eq!(top("<main><td>a</td><td>b</td>x&amp;y A &lt;-- B</main>"), 5);
        assert_eq!(top("<main><pre>A <-- B</pre></main>"), 2);
        assert_eq!(top("<main><!-- hidden words --><p>shown</p></main>"), 1);
        assert_eq!(top("<p>no main nor body</p><div data-wu=\"W1\">x</div>"), 4);
    }
}
