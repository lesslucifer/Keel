//! `keel verify-doc`: htmldoc checklist + relative-link checks (UC-DOC-1/2).
//! ponytail: hand-rolled tag scanner, not an HTML parser; fine for htmldoc's
//! well-formed output, swap in a real parser if docs start using odd markup.
use std::path::Path;

const EPIC_PLAN: &[&str] = &["intent", "use-cases", "expected", "non-goals", "wu-summaries", "e2e-plan", "distillation-plan", "open-questions"];
const WU_PLAN: &[&str] = &["requirement", "maps-to", "expected", "test-plan"];
const WU_RESULT: &[&str] = &["outcome", "deviations", "evidence", "distillation", "follow-ups"];
const EPIC_RESULT: &[&str] = &["outcome", "wu-rollup", "deviations", "evidence", "distillation", "follow-ups"];
const WU_SUB: &[&str] = &["wu-summary", "wu-maps-to", "wu-expected", "wu-deps"];

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

/// The root `<html>`'s `data-artifact` (its governed role(s)); `None` for an ungoverned doc.
pub fn artifact(raw: &str) -> Option<String> {
    let html = scannable(raw);
    let ts = tags(&html);
    let root = ts.iter().find(|t| t.name == "html" && !t.closing)?;
    attr(root, "data-artifact").map(String::from)
}

/// Problems with one doc; empty = green. Drafts are fully silent: no checklist, no link check.
pub fn verify(path: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return vec!["cannot read file".into()];
    };
    let html = scannable(&raw);
    let ts = tags(&html);
    let mut errs = vec![];

    let root = ts.iter().find(|t| t.name == "html");
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
            "wu-result" => WU_RESULT,
            "epic-result" => EPIC_RESULT,
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

#[cfg(test)]
mod tests {
    use super::artifact;

    #[test]
    fn artifact_reads_the_root_role_and_is_none_for_an_ungoverned_doc() {
        let doc = r#"<!doctype html><html lang="en" data-artifact="wu-result" data-status="final"><body><div data-artifact="epic-plan"></div></body></html>"#;
        assert_eq!(artifact(doc).as_deref(), Some("wu-result"));
        assert_eq!(artifact(r#"<html data-artifact="epic-result wu-result">"#).as_deref(), Some("epic-result wu-result"));
        assert_eq!(artifact("<html lang=\"en\"><p>x-result</p></html>"), None);
        assert_eq!(artifact("<p>wu a: done</p>"), None);
        assert_eq!(artifact(r#"<script>"<html data-artifact=\"wu-result\">"</script><html>"#), None);
    }
}
