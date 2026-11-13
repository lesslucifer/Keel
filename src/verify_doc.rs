//! `keel verify-doc`: htmldoc checklist, relative links, quoted diagram labels (UC-DOC-1/2/4).
//! ponytail: hand-rolled tag scanner, not an HTML parser; fine for htmldoc's
//! well-formed output, swap in a real parser if docs start using odd markup.
use std::path::Path;

const EPIC_PLAN: &[&str] = &["intent", "owner-changes", "use-cases", "expected", "non-goals", "wu-summaries", "e2e-plan", "distillation-plan", "open-questions"];
const WU_PLAN: &[&str] = &["requirement", "maps-to", "expected", "test-plan"];
const WU_RESULT: &[&str] = &["outcome", "deviations", "evidence", "distillation", "follow-ups"];
const EPIC_RESULT: &[&str] = &["outcome", "wu-rollup", "deviations", "evidence", "distillation", "follow-ups"];
/// Known in every governed doc, required only where a checklist lists it (keel-write's today/after table).
const OPTIONAL: &[&str] = &["owner-changes"];
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

/// Tags in document order. A comment or CDATA section is one tag named `!--` and a doctype or
/// processing instruction one named `!`, so `text` drops them whole; a `>` inside a quoted
/// attribute value does not end its tag; a `<` not followed by a letter, `/`, `!` or `?` is text.
fn tags(html: &str) -> Vec<Tag<'_>> {
    let b = html.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while let Some(p) = html[i..].find('<') {
        let start = i + p;
        i = start + 1;
        let hidden = [("<!--", "-->"), ("<![CDATA[", "]]>")].into_iter().find(|(open, _)| html[start..].starts_with(open));
        if let Some((open, close)) = hidden {
            let from = start + open.len();
            let end = html[from..].find(close).map_or(html.len(), |q| from + q + close.len());
            out.push(Tag { name: "!--", attrs: "", start, end, closing: false });
            i = end;
            continue;
        }
        if !b.get(start + 1).is_some_and(|&c| c.is_ascii_alphabetic() || matches!(c, b'/' | b'!' | b'?')) {
            continue;
        }
        let mut j = start + 1;
        let mut quote = None;
        while j < b.len() {
            match (quote, b[j]) {
                (None, b'>') => break,
                (None, q @ (b'"' | b'\'')) => quote = Some(q),
                (Some(q), c) if c == q => quote = None,
                _ => {}
            }
            j += 1;
        }
        if j == b.len() {
            break;
        }
        let end = j + 1;
        let body = &html[start + 1..j];
        i = end;
        if body.starts_with('!') || body.starts_with('?') {
            out.push(Tag { name: "!", attrs: "", start, end, closing: false });
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

/// Elements that end a run of words where they open or close; every other tag, a comment
/// included, joins the text on its two sides, as a browser shows it (`non-<code>.md</code>`).
const BLOCK: &[&str] = &[
    "address", "article", "aside", "blockquote", "body", "br", "dd", "details", "div", "dl", "dt", "figcaption", "figure", "footer", "form", "h1", "h2", "h3",
    "h4", "h5", "h6", "head", "header", "hr", "html", "li", "main", "nav", "ol", "p", "pre", "section", "summary", "table", "tbody", "td", "tfoot", "th", "thead",
    "title", "tr", "ul", "!",
];

/// Calls `f` on each run of raw text in html[a..b] with the tag that ends it (`None` for the last).
fn walk<'h>(html: &'h str, ts: &[Tag], a: usize, b: usize, mut f: impl FnMut(&'h str, Option<&Tag>)) {
    let lo = ts.partition_point(|t| t.start < a);
    let hi = ts.partition_point(|t| t.start < b);
    let mut at = a;
    for t in &ts[lo..hi] {
        f(&html[at..t.start.max(at)], Some(t));
        at = t.end.min(b).max(at);
    }
    f(&html[at..b.max(at)], None);
}

/// The text of html[a..b] as a reader sees its words: tags dropped, a block tag one space.
fn text(html: &str, ts: &[Tag], a: usize, b: usize) -> String {
    let mut out = String::new();
    walk(html, ts, a, b, |run, t| {
        out.push_str(run);
        if t.is_some_and(|t| BLOCK.iter().any(|n| n.eq_ignore_ascii_case(t.name))) {
            out.push(' ');
        }
    });
    out
}

/// The source of html[a..b] as Mermaid reads it: markup kept, comments and CDATA hidden unless
/// they open inside a `"…"` string, where they are label text.
fn source(html: &str, ts: &[Tag], a: usize, b: usize) -> String {
    let mut out = String::new();
    let mut quoted = false;
    let mut keep = |out: &mut String, s: &str| {
        out.push_str(s);
        quoted ^= decode_with(s, None).matches('"').count() % 2 == 1; // a quote as the scan sees it
        quoted
    };
    walk(html, ts, a, b, |run, t| {
        let quoted = keep(&mut out, run);
        if let Some(t) = t.filter(|t| t.end <= b && (t.name != "!--" || quoted)) {
            keep(&mut out, &html[t.start..t.end]);
        }
    });
    out
}

fn filled(html: &str, ts: &[Tag], i: usize) -> bool {
    let mut any = false;
    if let Some((a, b)) = span(ts, i) {
        walk(html, ts, a, b, |run, _| any |= run.chars().any(|c| !c.is_whitespace()));
    }
    any
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

    let mut scanned = 0;
    for (i, t) in ts.iter().enumerate().filter(|(_, t)| !t.closing && attr(t, "class").is_some_and(|c| c.split_whitespace().any(|c| c == "mermaid"))) {
        if t.start < scanned {
            continue; // inside a block already scanned (an unclosed one runs to the end)
        }
        let (a, b) = span(&ts, i).unwrap_or((t.end, html.len()));
        scanned = b;
        // Scan the decoded source (Mermaid decodes it), report from the source as written.
        let raw = source(&html, &ts, a, b);
        let (src, map) = decode_mapped(&raw);
        for h in hits(&src) {
            let (found, fix) = h.render(|(x, y)| &raw[map[x]..map[y]]);
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
    let mut known: Vec<&str> = vec![];
    for k in expected.iter().chain(OPTIONAL) {
        if !known.contains(k) {
            known.push(k);
        }
    }
    let mut seen: Vec<&str> = vec![];
    for (i, t) in ts.iter().enumerate().filter(|(_, t)| !t.closing) {
        let Some(item) = attr(t, "data-item") else { continue };
        if known.contains(&item) {
            if filled(html, ts, i) {
                seen.push(item);
            } else {
                errs.push(format!("item \"{item}\" is empty"));
            }
        } else if !(epic && WU_SUB.contains(&item)) {
            errs.push(format!("unknown tag data-item=\"{item}\" (typo? expected one of: {})", known.join(", ")));
        }
    }
    for e in &expected {
        if !seen.contains(e) && !errs.iter().any(|m| m.starts_with(&format!("item \"{e}\" is empty"))) {
            errs.push(format!("missing item \"{e}\""));
        }
    }
    if epic {
        let wus = wu_blocks(ts);
        let n = top_words(html, ts, &wus);
        if n > TOP_CAP {
            errs.push(format!("epic plan top has {n} words above the first WU, over the cap of {TOP_CAP}: cut it, or move details below the work"));
        }
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

/// Decodes numeric character references (decimal and hex) and the named entities docs use; any
/// other `&name;` stays as written.
#[cfg(test)]
fn decode(s: &str) -> String {
    decode_with(s, None)
}

/// `decode`, with every unknown `&name;` replaced by `unknown` when one is given.
fn decode_with(s: &str, unknown: Option<char>) -> String {
    decode_core(s, unknown, None)
}

/// `decode`, with `map[j]` the offset in `s` where decoded byte `j` comes from (one entry past the
/// end too), so a decoded range `x..y` is `s[map[x]..map[y]]` as written.
fn decode_mapped(s: &str) -> (String, Vec<usize>) {
    let mut map = Vec::with_capacity(s.len() + 1);
    let out = decode_core(s, None, Some(&mut map));
    (out, map)
}

fn decode_core(s: &str, unknown: Option<char>, mut map: Option<&mut Vec<usize>>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let hit = rest.starts_with('&').then(|| rest.as_bytes()[1..].iter().take(32).position(|&c| c == b';').filter(|&n| n > 0)).flatten().and_then(|n| {
            let name = &rest[1..=n];
            let named = name.bytes().all(|c| c.is_ascii_alphanumeric());
            entity(name).or(unknown.filter(|_| named)).map(|c| (c, n + 2))
        });
        let (c, len) = hit.unwrap_or_else(|| {
            let c = rest.chars().next().unwrap_or('&');
            (c, c.len_utf8())
        });
        if let Some(m) = map.as_deref_mut() {
            m.extend(std::iter::repeat_n(i, c.len_utf8()));
        }
        out.push(c);
        i += len;
    }
    if let Some(m) = map {
        m.push(s.len());
    }
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(n) = name.strip_prefix('#') {
        let code = match n.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => n.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "mdash" => '—',
        "ndash" => '–',
        "rarr" => '→',
        "larr" => '←',
        "harr" => '↔',
        "hellip" => '…',
        "middot" => '·',
        "bull" => '•',
        "times" => '×',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        _ => return None,
    })
}

/// The open tags carrying `data-wu`, in document order.
fn wu_blocks(ts: &[Tag]) -> Vec<usize> {
    (0..ts.len()).filter(|&i| !ts[i].closing && attr(&ts[i], "data-wu").is_some()).collect()
}

/// Words in an epic plan's top: the text from the start of `<main>` (else `<body>`) up to the
/// first `data-wu` block (`wus`, from `wu_blocks`). A word is a whitespace-separated run, entities
/// decoded, holding a letter or digit: a dash, arrow, `·` or unknown `&name;` is not one; inline
/// tags join the text around them, block tags (`BLOCK`) separate it.
fn top_words(html: &str, ts: &[Tag], wus: &[usize]) -> usize {
    let open = |n: &str| ts.iter().find(|t| t.name == n && !t.closing).map(|t| t.end);
    let start = open("main").or_else(|| open("body")).unwrap_or(0);
    let end = wus.iter().map(|&i| ts[i].start).find(|&s| s >= start).unwrap_or(html.len());
    let words = decode_with(&text(html, ts, start, end.max(start)), Some('\u{fffd}'));
    words.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

const FLOWCHARTS: &[&str] = &["flowchart", "graph", "flowchart-elk"];
/// Lines skipped to their end.
const DIRECTIVES: &[&str] = &["classDef", "style", "linkStyle", "direction", "accTitle", "accDescr"];
/// Statements skipped to their end: the line's end or a `;` outside quotes.
const STATEMENTS: &[&str] = &["class", "click", "end"];

/// `hits` read back from `src` itself, as (found, fix).
#[cfg(test)]
fn unquoted_labels(src: &str) -> Vec<(String, String)> {
    hits(src).into_iter().map(|h| h.render(|(x, y)| &src[x..y])).collect()
}

/// A byte range of the scanned source.
type Span = (usize, usize);

/// One unquoted label: the text that names it, and the parts its fix is built from.
struct Hit {
    found: Span,
    pre: Span,
    label: Span,
    post: Span,
    spaced: bool,
}

impl Hit {
    /// (found, fix), each part read through `at` (the source as written).
    fn render<'s>(&self, at: impl Fn(Span) -> &'s str) -> (String, String) {
        let sp = if self.spaced { " " } else { "" };
        (at(self.found).to_string(), format!("{}{sp}\"{}\"{sp}{}", at(self.pre), inner(at(self.label)), at(self.post)))
    }
}

/// Flowchart labels not in double quotes, as spans of `src`.
///
/// The grammar this scanner knows, a subset of Mermaid 11's flowchart: after optional `---` front
/// matter (indented or not), `%%` lines and `%%{…}%%` directives, the header `flowchart`, `graph`
/// or `flowchart-elk` (other diagram kinds are not scanned). The rest is one token stream; a statement starts at a line start or after
/// `;`. A `"…"` string runs across lines and hides every delimiter in it. Labels: a node shape
/// (`A[x]`, `A(x)`, `A{x}`, `A>x]`, doubled and slanted forms) from its opening brackets to the
/// matching close, brackets nesting inside; an edge label between `|…|`; edge text from `--`, `==`
/// or `-.` to the next `--`, `==` or `-.`/`.-` outside quotes, spaced or not. Skipped whole: `%%`
/// lines, `%%{…}%%` directives anywhere and over any lines, `accDescr {…}` blocks (spaced or not),
/// lines opening with `accTitle`, `accDescr:`, `classDef`, `style`, `linkStyle` or `direction`,
/// statements opening with `class`, `click` or `end` (to the `;` or line end), and `N@{…}` shape
/// blocks. `src` is the block's decoded source as Mermaid reads it, markup included (`A[a<br>b]`);
/// a `<` that opens a tag never belongs to an arrow. Each hit is a set of spans the caller reads
/// back from the source as written (entities kept), so a fix hint keeps the label exact, markup
/// and entities included, a `"` written `#quot;`.
///
/// Best effort (owner ruling 2026-11-13): a label this misses is a known limit; refusing a valid
/// diagram, or a hint that changes it, is a bug. Known misses: a bracketless subgraph title, the
/// inside of a quoted label, `N@{…}` shapes, other diagram kinds, a `>…]` shape after an id that
/// ends in a non-ASCII or punctuation character, and a statement after a `;` on a skipped line.
fn hits(src: &str) -> Vec<Hit> {
    let mut rest = src;
    loop {
        let t = rest.trim_start();
        if let Some(d) = t.strip_prefix("%%{") {
            rest = d.find("}%%").map_or("", |p| &d[p + 3..]);
        } else if t.starts_with("%%") {
            rest = t.split_once('\n').map_or("", |(_, r)| r);
        } else if let Some(fm) = t.strip_prefix("---") {
            let fm = fm.split_once('\n').map_or("", |(_, r)| r);
            let mut off = 0;
            rest = "";
            for line in fm.split_inclusive('\n') {
                off += line.len();
                if line.trim() == "---" {
                    rest = &fm[off..];
                    break;
                }
            }
        } else {
            rest = t;
            break;
        }
    }
    let kind = rest.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("");
    if !FLOWCHARTS.contains(&kind) {
        return vec![];
    }
    let body = rest.find(['\n', ';']).map_or("", |p| &rest[p + 1..]);
    let off = src.len() - body.len();
    let mut out = vec![];
    scan(body, &mut out);
    let shift = |(x, y): Span| (x + off, y + off);
    for h in &mut out {
        (h.found, h.pre, h.label, h.post) = (shift(h.found), shift(h.pre), shift(h.label), shift(h.post));
    }
    out
}

/// A label's text as it goes inside the quotes of a fix: Mermaid writes a `"` as `#quot;`, and so
/// is every entity that `entity` decodes to one.
fn inner(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut rest = label;
    while let Some(p) = rest.find(['"', '&']) {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let quote = if rest.starts_with('"') {
            Some(1)
        } else {
            rest.as_bytes()[1..].iter().take(32).position(|&c| c == b';').filter(|&n| n > 0 && entity(&rest[1..=n]) == Some('"')).map(|n| n + 2)
        };
        let n = quote.unwrap_or(1);
        out.push_str(if quote.is_some() { "#quot;" } else { &rest[..1] });
        rest = &rest[n..];
    }
    out.push_str(rest);
    out
}

/// `s[a..b]` without its outer whitespace, as a span.
fn trimmed(s: &str, a: usize, b: usize) -> Span {
    let t = &s[a..b];
    let lead = t.len() - t.trim_start().len();
    (a + lead, (a + lead).max(b - (t.len() - t.trim_end().len())))
}

/// The length of the arrow run at `i`; a `<` that opens a tag (`<b>`) is not part of it.
fn link_run(b: &[u8], i: usize) -> usize {
    let tag = |j: usize| b[j] == b'<' && b.get(j + 1).is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, b'/' | b'!'));
    (i..b.len()).take_while(|&j| is_link(b[j]) && !tag(j)).count()
}

fn is_link(c: u8) -> bool {
    matches!(c, b'-' | b'=' | b'.' | b'<' | b'>')
}

fn is_close(c: u8) -> bool {
    matches!(c, b']' | b')' | b'}')
}

fn is_open(c: u8) -> bool {
    matches!(c, b'[' | b'(' | b'{')
}

fn scan(s: &str, out: &mut Vec<Hit>) {
    let b = s.as_bytes();
    let skip_quoted = |i: usize| b[i + 1..].iter().position(|&c| c == b'"').map_or(b.len(), |q| i + q + 2);
    let line_end = |i: usize| s[i..].find('\n').map_or(s.len(), |p| i + p);
    let stmt_end = |mut j: usize| {
        while j < b.len() && b[j] != b'\n' && b[j] != b';' {
            j = if b[j] == b'"' { skip_quoted(j) } else { j + 1 };
        }
        j.min(b.len())
    };
    let mut i = 0;
    let mut at_start = true;
    while i < b.len() {
        let c = b[i];
        if c == b'%' && s[i..].starts_with("%%{") {
            i = s[i..].find("}%%").map_or(s.len(), |p| i + p + 3);
            continue;
        }
        if c == b'\n' || c == b';' {
            at_start = true;
            i += 1;
            continue;
        }
        if at_start {
            if c.is_ascii_whitespace() {
                i += 1;
                continue;
            }
            at_start = false;
            if s[i..].starts_with("%%") {
                i = line_end(i);
                continue;
            }
            let n = s[i..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len() - i);
            let word = &s[i..i + n];
            if word == "accDescr" && s[i + n..].trim_start().starts_with('{') {
                i = s[i..].find('}').map_or(s.len(), |p| i + p + 1);
                continue;
            }
            if STATEMENTS.contains(&word) {
                i = stmt_end(i);
                continue;
            }
            if DIRECTIVES.contains(&word) {
                i = line_end(i);
                continue;
            }
            if word == "subgraph" {
                i += n;
                continue;
            }
        }
        if c == b'"' {
            i = skip_quoted(i);
        } else if c == b'@' && b.get(i + 1) == Some(&b'{') {
            let mut j = i + 2;
            while j < b.len() && b[j] != b'}' {
                j = if b[j] == b'"' { skip_quoted(j) } else { j + 1 };
            }
            i = j + 1;
        } else if c == b'|' {
            let mut j = i + 1;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            let from = if j < b.len() && b[j] == b'"' { skip_quoted(j) } else { j };
            let Some(k) = b.get(from..).and_then(|r| r.iter().position(|&c| c == b'|')).map(|k| from + k) else { return };
            let label = trimmed(s, i + 1, k);
            if !s[label.0..].starts_with('"') {
                out.push(Hit { found: (i, k + 1), pre: (i, i + 1), label, post: (k, k + 1), spaced: false });
            }
            i = k + 1;
        } else if is_open(c) || (c == b'>' && i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) {
            let mut j = i + 1;
            let mut opens = 1;
            while c != b'>' && j < b.len() && is_open(b[j]) {
                j += 1;
                opens += 1;
            }
            if b[j - 1] == b'[' && j < b.len() && matches!(b[j], b'/' | b'\\') {
                j += 1;
            }
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            if j < b.len() && b[j] == b'"' {
                i = skip_quoted(j);
                while i < b.len() && is_close(b[i]) {
                    i += 1;
                }
                continue;
            }
            let mut k = j;
            let mut depth = opens;
            while k < b.len() {
                if b[k] == b'"' {
                    k = skip_quoted(k);
                    continue;
                }
                if is_open(b[k]) {
                    depth += 1;
                } else if is_close(b[k]) {
                    depth -= 1;
                    if depth < opens {
                        break;
                    }
                }
                k += 1;
            }
            let mut close = k;
            while close < b.len() && close - k < opens && is_close(b[close]) {
                close += 1;
            }
            if k > j && matches!(b[k - 1], b'/' | b'\\') {
                k -= 1;
            }
            let pre = (i, i + s[i..j].trim_end().len());
            out.push(Hit { found: (i, close), pre, label: trimmed(s, j, k), post: (k, close), spaced: false });
            i = close;
        } else if is_link(c) {
            let run = link_run(b, i);
            if run == 0 {
                i = s[i..].find('>').map_or(s.len(), |p| i + p + 1); // a tag between statements
                continue;
            }
            let from = i + s[i..i + run].len() - s[i..i + run].trim_start_matches('<').len();
            let arrow = &s[from..i + run];
            let ends_arrow = |n: &u8| matches!(n, b'x' | b'o') && arrow != "-.";
            let opens_text = matches!(arrow, "--" | "==" | "-.") && b.get(i + run).is_some_and(|n| !ends_arrow(n));
            i += run;
            if opens_text {
                let shut: &[u8] = if arrow == "-." { b".-" } else { arrow.as_bytes() };
                let mut k = i;
                while k < b.len() && !b[k..].starts_with(shut) {
                    k = if b[k] == b'"' { skip_quoted(k) } else { k + 1 };
                }
                if k >= b.len() {
                    return;
                }
                if arrow == "-." {
                    while k > i && b[k - 1] == b'.' {
                        k -= 1;
                    }
                    if k > i && b[k - 1] == b'-' {
                        k -= 1;
                    }
                }
                let label = trimmed(s, i, k);
                let end = link_run(b, k);
                if !s[label.0..].starts_with('"') {
                    out.push(Hit { found: (from, k + end), pre: (from, i), label, post: (k, k + end), spaced: true });
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
    use super::{artifact, decode, decode_mapped, has_role, hits, result_role, tags, top_words, unquoted_labels, verify, wu_blocks};

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

    /// Epic code review round 2, blocker 2: a decoded range maps back to the source as written.
    #[test]
    fn a_decoded_range_maps_back_to_the_source() {
        let raw = "A[Vec&lt;String&gt;] é&amp;lt;";
        let (d, map) = decode_mapped(raw);
        assert_eq!(d, "A[Vec<String>] é&lt;");
        assert_eq!(map.len(), d.len() + 1);
        assert_eq!(&raw[map[1]..map[14]], "[Vec&lt;String&gt;]");
        assert_eq!(&raw[map[15]..map[d.len()]], "é&amp;lt;");
        let raw = "flowchart LR\n  A[a&nbsp;b]";
        let (d, map) = decode_mapped(raw);
        let h = hits(&d);
        assert_eq!(h[0].render(|(x, y)| &raw[map[x]..map[y]]), ("[a&nbsp;b]".into(), "[\"a&nbsp;b\"]".into()));
        assert_eq!(super::inner("a&quot;b\"c"), "a#quot;b#quot;c");
    }

    /// Epic code review round 3, nit: every form entity() decodes to `"` is written `#quot;`.
    #[test]
    fn every_quote_entity_is_written_as_quot() {
        assert_eq!(super::inner(r#"a"b&quot;c&#34;d&#034;e&#x22;f&#X22;g&#x0022;h&amp;i"#), "a#quot;b#quot;c#quot;d#quot;e#quot;f#quot;g#quot;h&amp;i");
    }

    /// Epic code review round 4, I2: a character reference with a sign is no entity.
    #[test]
    fn a_signed_character_reference_is_no_entity() {
        assert_eq!(super::entity("#34"), Some('"'));
        assert_eq!(super::entity("#x22"), Some('"'));
        for signed in ["#+34", "#-34", "#x+22", "#X+22", "#x", "#"] {
            assert_eq!(super::entity(signed), None, "{signed}");
        }
        assert_eq!(decode("&#+34;"), "&#+34;");
        assert_eq!(super::inner("a&#+34;b"), "a&#+34;b");
    }

    /// Epic code review round 2, blocker 1: a `<` that opens a tag ends the arrow.
    #[test]
    fn a_tag_after_an_arrow_is_edge_text() {
        assert_eq!(super::link_run(b"--<b>x", 0), 2);
        assert_eq!(super::link_run(b"<-->B", 0), 4);
        assert_eq!(super::link_run(b"--></b>", 0), 3);
        assert_eq!(unquoted_labels("flowchart LR\n  A--<b>x</b>-->B"), vec![("--<b>x</b>-->".into(), "-- \"<b>x</b>\" -->".into())]);
        assert!(labels("flowchart LR\n  A--\"<b>x</b>\"-->B").is_empty());
    }

    /// Epic code review round 2, blocker 3: a comment is hidden only outside a quoted string.
    #[test]
    fn a_comment_inside_a_quoted_string_is_kept() {
        let html = r#"A["<!--"] --> B["b"] <!-- [z] --> C"#;
        let ts = tags(html);
        assert_eq!(super::source(html, &ts, 0, html.len()), r#"A["<!--"] --> B["b"]  C"#);
        let html = r#"A[&#34;<!--&#34;] --> B <!-- [z] --> C"#;
        let ts = tags(html);
        assert_eq!(super::source(html, &ts, 0, html.len()), r#"A[&#34;<!--&#34;] --> B  C"#, "an entity quote counts");
    }

    /// Epic code review round 1, nit: a label is reported as written in the source, spaces kept.
    #[test]
    fn a_label_is_reported_as_written() {
        assert_eq!(unquoted_labels("flowchart LR\n  A -->| yes | B"), vec![("| yes |".into(), "|\"yes\"|".into())]);
        assert_eq!(unquoted_labels("flowchart LR\n  A[ yes ] --> B"), vec![("[ yes ]".into(), "[\"yes\"]".into())]);
    }

    #[test]
    fn an_unspaced_edge_label_is_scanned_and_a_cross_or_circle_end_is_not_a_label() {
        assert_eq!(labels("flowchart LR\n  A--yes-->B\n  B==go==>C\n  C-.later.->D\n  D<--back-->A"), ["--yes-->", "==go==>", "-.later.->", "--back-->"]);
        assert_eq!(unquoted_labels("flowchart LR\n  A--yes-->B")[0].1, "-- \"yes\" -->");
        let fine = "flowchart LR\n  A--\"yes\"-->B\n  A--xB\n  A--oB\n  A==xB\n  A--x C\n  A--o C\n  A--well-known-->B";
        assert_eq!(labels(fine), ["--well-known-->"]);
    }

    #[test]
    fn quoted_text_hides_delimiters_and_non_label_lines_are_skipped() {
        let src = "---\ntitle: A[x]\n---\nflowchart TD;  A[\"a [b] | c -- d\"] --> B\n  %% C[comment]\n  classDef hot fill:#f96\n  style A fill:#f9f\n  class A hot\n  click A \"https://x.io\" \"tip (x)\"\n  linkStyle 0 stroke:#f00\n  direction LR\n  N@{ shape: rect, label: \"x [y]\" }\n  end";
        assert!(labels(src).is_empty(), "{:?}", labels(src));
        assert_eq!(labels("flowchart LR; A[raw]"), ["[raw]"]);
        assert_eq!(labels("flowchart LR\n  subgraph S [Title]\n  A\n  end"), ["[Title]"]);
        assert_eq!(labels("flowchart LR\n  A[W1 · the skill]"), ["[W1 · the skill]"]);
    }

    /// Code review round 1, blocker 1: a quoted edge text hides the link-like dots inside it.
    #[test]
    fn a_quoted_edge_text_hides_its_dots_from_the_link_search() {
        assert!(labels("flowchart LR\n  A -- \"wait...\" --> B(\"retry (3x)\")").is_empty(), "{:?}", labels("flowchart LR\n  A -- \"wait...\" --> B(\"retry (3x)\")"));
        assert_eq!(labels("flowchart LR\n  A -- \"wait...\" --> B(retry)"), ["(retry)"]);
    }

    /// Code review round 1, blocker 2: Mermaid's own multi-line quoted label.
    #[test]
    fn a_quoted_label_runs_across_lines() {
        let src = "flowchart LR\n  A[\"first line\n  second (x) line\"] --> B[raw]";
        assert_eq!(labels(src), ["[raw]"]);
    }

    /// Code review round 1, blocker 3: a whole accDescr block is skipped, spaced or not.
    #[test]
    fn an_acc_descr_block_is_skipped_whole_spaced_or_not() {
        for open in ["accDescr {", "accDescr{"] {
            let src = format!("flowchart LR\n  {open}\n    a diagram (x) of [y]\n    and more {{z}}\n  }}\n  accTitle: the [t]\n  accDescr: one (line)\n  A[raw]");
            assert_eq!(labels(&src), ["[raw]"], "{open}");
        }
        assert_eq!(labels("flowchart LR\n  accDescr{ one (x) }\n  A[raw]"), ["[raw]"]);
    }

    /// Code review round 1, blocker 4: Mermaid 11 parses `flowchart-elk` as a flowchart.
    #[test]
    fn a_flowchart_elk_diagram_is_scanned() {
        assert_eq!(labels("flowchart-elk TD\n  A[x] --> B"), ["[x]"]);
        assert_eq!(labels("%% lead\nflowchart-elk\n  A[x]"), ["[x]"]);
    }

    /// Code review round 1, important 5: a label ends at its shape's matching bracket.
    #[test]
    fn a_label_ends_at_its_matching_bracket_and_the_hint_quotes_all_of_it() {
        assert_eq!(unquoted_labels("flowchart LR\n  A[foo (bar)] --> B"), vec![("[foo (bar)]".into(), "[\"foo (bar)\"]".into())]);
        assert_eq!(unquoted_labels("flowchart LR\n  A([a {b} c]) --> B"), vec![("([a {b} c])".into(), "([\"a {b} c\"])".into())]);
        assert_eq!(labels("flowchart LR\n  A[/a (b)/] --> B{c [d]}"), ["[/a (b)/]", "{c [d]}"]);
    }

    /// Code review round 1, important 6: comments and quoted attribute values hide their `>`.
    #[test]
    fn a_comment_or_attribute_value_hides_its_angle_bracket_from_the_word_count() {
        assert_eq!(top("<main><!-- x > y z --><p title=\"b > c d\">one</p></main>"), 1);
        assert_eq!(top("<main><p data-x='a > b'>one two</p></main>"), 2);
    }

    /// Code review round 1, important 7: numeric entities and the named ones docs use decode;
    /// a decoded punctuation mark is not a word.
    #[test]
    fn entities_decode_numeric_and_named_and_punctuation_is_no_word() {
        assert_eq!(decode("&#x22;hi&#34;&quot; &mdash;&#8212;&#x2014; &rarr; &amp;lt; &bogus; &"), "\"hi\"\" ——— → &lt; &bogus; &");
        assert_eq!(top("<main>a &mdash; b &rarr; c &#8212; d &hellip;</main>"), 4);
        assert!(labels(&decode("flowchart LR\n  A[&#x22;hi&#x22;] --> B[&#34;b&#34;]")).is_empty());
    }

    /// Code review round 2, B3: a `%%{…}%%` directive is skipped whole, multi-line or inline.
    #[test]
    fn a_directive_is_skipped_whole_multi_line_or_inline() {
        assert!(labels("%%{\n  init: {'theme': 'dark'}\n}%%\nflowchart LR\n  A[\"a\"] --> B[\"b\"]").is_empty());
        assert_eq!(labels("%%{init: {'theme': 'dark'}}%% flowchart LR\n  A[x]"), ["[x]"]);
        assert!(labels("flowchart LR\n  %%{init: {'theme': 'dark'}}%%\n  A[\"a\"]\n  %%{\n  init: {'x': 'y'}\n  }%%\n  B[\"b\"]").is_empty());
    }

    /// Code review round 2, B5: the hint keeps the label's text exact.
    #[test]
    fn the_hint_keeps_the_label_text_exact() {
        assert_eq!(unquoted_labels("flowchart LR\n  A[\"a\"] -. yes -.-> B[\"b\"]"), vec![("-. yes -.->".into(), "-. \"yes\" -.->".into())]);
        assert_eq!(unquoted_labels("flowchart LR\n  A[say \"hi\" now]"), vec![("[say \"hi\" now]".into(), "[\"say #quot;hi#quot; now\"]".into())]);
    }

    /// Code review round 2, B6: inline tags join the words around them; block tags separate.
    #[test]
    fn inline_tags_join_words_and_block_tags_separate_them() {
        assert_eq!(top("<main><p>non-<code>.md</code> <b>re</b>use <a href=\"x\">li</a>nk</p></main>"), 3);
        assert_eq!(top("<main><p>one</p><p>two</p><ul><li>three</li></ul><table><tr><td>four</td><th>five</th></tr></table><h2>six</h2><div>seven</div>eight<br>nine<pre>ten</pre></main>"), 10);
    }

    /// Code review round 2, B7: an unknown named entity is one symbol, like its numeric form.
    #[test]
    fn an_unknown_named_entity_is_no_word() {
        assert_eq!(top("<main>a &copy; b &#169; c &reg;</main>"), 3);
    }

    /// Code review round 2, B1, B2, B8 (known limits made trivial): `class`, `click` and `end`
    /// end at `;`; indented front matter; CDATA is skipped whole.
    #[test]
    fn a_class_click_or_end_statement_ends_at_a_semicolon_front_matter_may_be_indented_and_cdata_is_skipped() {
        assert_eq!(labels("flowchart LR\n  class A hot; B[x]\n  click A cb \"t;ip\"; C[y]\n  end; D[z]"), ["[x]", "[y]", "[z]"]);
        assert_eq!(labels("  ---\n  title: t\n  ---\n  flowchart LR\n  A[x]"), ["[x]"]);
        assert_eq!(top("<main><svg><![CDATA[ a > b c ]]></svg>one</main>"), 1);
    }

    #[test]
    fn only_flowcharts_are_scanned() {
        for src in ["sequenceDiagram\n  A->>B: hi [x]", "stateDiagram-v2\n  A --> B: go [x]", "pie\n  \"a\" : 1", "", "flowchartX\n  A[x]"] {
            assert!(labels(src).is_empty(), "{src}");
        }
    }

    fn top(html: &str) -> usize {
        let ts = tags(html);
        top_words(html, &ts, &wu_blocks(&ts))
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
