//! `keel verify-doc`: a plan's or result's data file against its role's fields, and every page's
//! relative links, quoted diagram labels and word cap (UC-DOC-1/2/4, DEC-69).
//! ponytail: hand-rolled tag scanner, not an HTML parser; fine for htmldoc's
//! well-formed output, swap in a real parser if docs start using odd markup.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::{Yaml, YamlLoader};

const EPIC_PLAN: &[&str] =
    &["intent", "owner-changes", "use-cases", "expected", "non-goals", "wu-summaries", "e2e-plan", "distillation-plan", "open-questions"];
const WU_PLAN: &[&str] = &["requirement", "maps-to", "expected", "test-plan"];
const WU_RESULT: &[&str] = &["outcome", "deviations", "evidence", "distillation", "follow-ups"];
const EPIC_RESULT: &[&str] = &["outcome", "wu-rollup", "deviations", "evidence", "distillation", "follow-ups"];
/// Every data file's own fields (DEC-69): what it is, whether it is done, and which page it serves.
const COMMON: &[&str] = &["artifact", "status", "id", "title", "date", "page"];
/// Known in every data file, required only where a role lists it; `details` is free and never checked.
const OPTIONAL: &[&str] = &["owner-changes", "details"];
/// Each entry of an epic plan's `wu-summaries`, one per WU.
const WU_SUB: &[&str] = &["wu", "wu-summary", "wu-maps-to", "wu-expected", "wu-deps"];
/// The levels a WU plan's `test-plan` names; other keys under it are free.
const TEST_LEVELS: &[&str] = &["unit", "integration", "e2e"];
const WU_RESULT_ROLE: &str = "wu-result";
const EPIC_RESULT_ROLE: &str = "epic-result";
/// The one table of roles (DEC-69): each role, its own required fields (beyond the common ones and
/// `epic`), and the words a ready page with that role may hold; a one-part epic takes the epic cap.
const ROLE_TABLE: &[(&str, &[&str], usize)] =
    &[("epic-plan", EPIC_PLAN, 700), ("wu-plan", WU_PLAN, 500), (WU_RESULT_ROLE, WU_RESULT, 500), (EPIC_RESULT_ROLE, EPIC_RESULT, 700)];
/// The role names, in the table's order.
const ROLES: [&str; ROLE_TABLE.len()] = {
    let mut names = [""; ROLE_TABLE.len()];
    let mut i = 0;
    while i < ROLE_TABLE.len() {
        names[i] = ROLE_TABLE[i].0;
        i += 1;
    }
    names
};
/// Words allowed in an epic plan page's top: its main content above the heading with id `work`.
const TOP_CAP: usize = 300;
/// The id that marks where an epic plan page's top ends: the heading of its parts.
const WORK_ID: &str = "work";

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
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "br",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
    "!",
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

/// The text of html[a..b] as a reader sees its words: tags dropped, a block tag one space. With it,
/// the offset in that text where the tag starting at html offset `cut` stands (the text's length
/// when no tag starts there).
fn text(html: &str, ts: &[Tag], a: usize, b: usize, cut: Option<usize>) -> (String, usize) {
    let mut out = String::new();
    let mut at = None;
    walk(html, ts, a, b, |run, t| {
        out.push_str(run);
        if at.is_none() && t.is_some_and(|t| Some(t.start) == cut) {
            at = Some(out.len());
        }
        if t.is_some_and(|t| BLOCK.iter().any(|n| n.eq_ignore_ascii_case(t.name))) {
            out.push(' ');
        }
    });
    let at = at.unwrap_or(out.len());
    (out, at)
}

/// The source of html[a..b] as Mermaid reads it: markup kept, comments and CDATA hidden unless
/// they open inside a `"…"` string, where they are label text.
fn source(html: &str, ts: &[Tag], a: usize, b: usize) -> String {
    let mut out = String::new();
    walk(html, ts, a, b, |run, t| {
        out.push_str(run);
        if let Some(t) = t.filter(|t| t.end <= b && (t.name != "!--" || in_quote(&out))) {
            out.push_str(&html[t.start..t.end]);
        }
    });
    out
}

/// Does `src` end inside a `"…"` string? Quotes count as the scan sees them: decoded, and not on
/// a line the scan skips unread (`%%` lines, `classDef`, `style` and the other directive lines).
fn in_quote(src: &str) -> bool {
    let mut quoted = false;
    for line in decode_with(src, None).split('\n') {
        let w = line.trim_start();
        let word = &w[..w.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(w.len())];
        if !quoted && (w.starts_with("%%") || DIRECTIVES.contains(&word)) {
            continue;
        }
        quoted ^= line.matches('"').count() % 2 == 1;
    }
    quoted
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

/// The result role a sign-off at this level attests.
pub fn result_role(epic: bool) -> &'static str {
    if epic {
        EPIC_RESULT_ROLE
    } else {
        WU_RESULT_ROLE
    }
}

/// The roles a data file's `artifact` gives its page (DEC-69, E-DOCSPLIT.W3), read as the doc check
/// reads them (`load_data`, `holding_artifact`, `artifact_roles`, `role_problem`): its text split on
/// white space, or its list. A file of several YAML documents gives the one holding `artifact`. `Err`
/// names why there are none: the bytes are too many or not UTF-8 text, the text is no YAML, is too
/// deeply nested, holds an anchor, has no `artifact`, one that is neither text nor a list, or roles
/// the doc check refuses. `src` is the file's bytes as they are - never trimmed, never decoded
/// lossily - so the sign-off reads a committed data file as the doc check reads it on disk.
pub fn data_roles(src: impl AsRef<[u8]>) -> Result<Vec<String>, String> {
    let bytes = src.as_ref();
    if let Some(p) = size_problem(bytes.len() as u64) {
        return Err(p);
    }
    let src = std::str::from_utf8(bytes).map_err(|_| "the data file is not UTF-8 text".to_string())?;
    let docs = load_data(src)?;
    let Some(a) = holding_artifact(&docs).and_then(|d| d.as_hash()).and_then(|h| h.get(&Yaml::String("artifact".into()))) else {
        return Err("no field \"artifact\"".into());
    };
    let roles: Vec<String> = artifact_roles(a)
        .map_err(|other| format!("artifact {} is neither text nor a list", shown(other)))?
        .into_iter()
        .map(|r| r.map_err(|r| format!("artifact holds {}, not a role", shown(r))))
        .collect::<Result<_, _>>()?;
    match role_problem(&roles) {
        Some(p) => Err(p),
        None => Ok(roles),
    }
}

/// A data file's text as YAML documents: a byte-order mark read past; its shape read first
/// (`shape_problem`: size, nesting, anchors). `Err` is the problem: a shape refusal, or "not valid
/// YAML".
fn load_data(src: &str) -> Result<Vec<Yaml>, String> {
    // A byte-order mark may open a YAML stream; the parser would read it as part of the first key.
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    if let Some(p) = shape_problem(src) {
        return Err(p);
    }
    YamlLoader::load_from_str(src).map_err(|e| format!("not valid YAML: {e}"))
}

/// The first document that is a mapping holding `artifact`: the one a data file of several is read by.
fn holding_artifact(docs: &[Yaml]) -> Option<&Yaml> {
    docs.iter().find(|d| matches!(d, Yaml::Hash(h) if h.contains_key(&Yaml::String("artifact".into()))))
}

/// The roles an `artifact` value names (DEC-69): text split on white space, or a list's items, each
/// item that is not text kept as `Err(item)`; `Err(value)` when it is neither text nor a list.
fn artifact_roles(a: &Yaml) -> Result<Vec<Result<String, &Yaml>>, &Yaml> {
    match a {
        Yaml::String(s) => Ok(s.split_whitespace().map(|r| Ok(r.to_string())).collect()),
        Yaml::Array(l) => Ok(l.iter().map(|r| r.as_str().map(String::from).ok_or(r)).collect()),
        other => Err(other),
    }
}

/// The doc pages a list of changed paths names (DEC-69): each path's `pair_named` page - a page for
/// itself, a data file under docs.root (`in_docs`) for its page - each page once, in first-named
/// order; a path that names no pair is dropped. Whether the page is there is the caller's to ask.
pub fn pages_of(names: &[String], in_docs: impl Fn(&Path) -> bool) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for n in names {
        let Some(page) = pair_named(Path::new(n), &in_docs).map(|p| p.page.to_string_lossy().into_owned()) else { continue };
        if !out.contains(&page) {
            out.push(page);
        }
    }
    out
}

/// One problem: the file it is in, and what is wrong.
pub type Problem = (PathBuf, String);

/// One doc as DEC-69 pairs it: a page and, when it has one, the data file beside it.
#[derive(Debug, PartialEq)]
pub struct Pair {
    pub page: PathBuf,
    pub data: Option<PathBuf>,
}

/// Whether a path is under docs.root (`root`, as `[docs] root` writes it), component by component:
/// the one docs.root test, the `in_docs` the pairing helpers take for a repo-relative path, at a tip
/// or (keel verify-doc) on disk.
pub fn under_root(root: &str) -> impl Fn(&Path) -> bool {
    let root = PathBuf::from(root.trim_end_matches('/'));
    move |p: &Path| p.starts_with(&root)
}

/// The pair a path names, by name only (DEC-69): the one pairing rule, so it also works at a tip,
/// where no file is on disk. A page `<stem>.html` and its data file `<stem>.yaml`, side by side; a
/// `.yaml` is a data file only under docs.root (`in_docs`), else no doc (`None`), and its page is
/// `<stem>.html`. A `.html` page's data file is `<stem>.yaml` when that name is under docs.root,
/// there or not. Any other path names no pair (`None`).
pub fn pair_named(path: &Path, in_docs: impl Fn(&Path) -> bool) -> Option<Pair> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("yaml") => in_docs(path).then(|| Pair { page: path.with_extension("html"), data: Some(path.to_owned()) }),
        Some("html") => {
            let data = path.with_extension("yaml");
            Some(Pair { page: path.to_owned(), data: in_docs(&data).then_some(data) })
        }
        _ => None,
    }
}

/// The other file of the doc a path names (`pair_named`): the data file's name for a page that has
/// one under docs.root, the page's for a data file; `None` for a page outside docs.root or any other
/// path.
pub fn partner_named(path: &Path, in_docs: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let Pair { page, data } = pair_named(path, in_docs)?;
    if page == path {
        data
    } else {
        Some(page)
    }
}

/// The pair a path names on disk (DEC-69): `pair_named`, with a page's data file only when that file
/// is there. A data file is named even when it is not there (it is read, and a missing one is
/// "cannot read file"). A `.yaml` outside docs.root is no doc (`None`); any other path is a page
/// alone. Two paths of one doc give one pair.
pub fn pair_of(path: &Path, in_docs: impl Fn(&Path) -> bool) -> Option<Pair> {
    let is_data = |pair: &Pair| pair.data.as_deref() == Some(path);
    match pair_named(path, in_docs) {
        Some(pair) if is_data(&pair) => Some(pair),
        Some(Pair { page, data }) => Some(Pair { page, data: data.filter(|d| d.is_file()) }),
        // No pair: a data file's name outside docs.root is no doc, any other path is a page alone.
        None if pair_named(path, |_| true).is_some_and(|p| is_data(&p)) => None,
        None => Some(Pair { page: path.to_owned(), data: None }),
    }
}

/// Problems with one doc - a page and the data file beside it, either one possibly absent; empty =
/// green. `data` is the data file to read, when there is one (`pair_of` finds it). `path_cap` is the `[docs.caps]` glob and cap that match the page's path,
/// used when no ready data file gives the page a role. A data file marked draft silences the pair; a
/// page with no data file is silenced by its own `data-status="draft"`.
pub fn verify_pair(page: &Path, data: Option<&Path>, path_cap: Option<(&str, usize)>) -> Vec<Problem> {
    let mut out = vec![];
    let name = page.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut roles = None;
    if let Some(d) = data {
        if let Some(p) = std::fs::metadata(d).ok().and_then(|m| size_problem(m.len())) {
            return vec![(d.to_owned(), p)];
        }
        let Ok(src) = std::fs::read_to_string(d) else {
            return vec![(d.to_owned(), "cannot read file".into())];
        };
        match read_data(&src, &name, page.is_file()) {
            Data::None => {}
            Data::Draft => return out,
            Data::Checked { roles: r, problems } => {
                out.extend(problems.into_iter().map(|p| (d.to_owned(), p)));
                roles = Some(r);
            }
        }
    }
    if data.is_some() && !page.is_file() {
        return out; // read_data named the missing page, or the file is other data with no page
    }
    out.extend(page_problems(page, roles.as_deref(), path_cap).into_iter().map(|p| (page.to_owned(), p)));
    out
}

/// What a data file is: not one (no `artifact` field, no page beside it - another kind of data), a
/// draft (silent), or one to check: its roles (empty when unknown) and its problems.
#[derive(Debug, PartialEq)]
enum Data {
    None,
    Draft,
    Checked { roles: Vec<String>, problems: Vec<String> },
}

/// Reads one data file's text against its role's fields (DEC-69). `page` is the page file name the
/// data file must serve (its own name, ending `.html`); `page_exists`, whether that file is beside it.
fn read_data(src: &str, page: &str, page_exists: bool) -> Data {
    let docs = match load_data(src) {
        Ok(d) => d,
        Err(p) => return Data::Checked { roles: vec![], problems: vec![p] },
    };
    match docs.as_slice() {
        [d] => check_doc(d, page, page_exists),
        [] => unfit("the data file is empty: it needs its role's fields".into(), page_exists),
        _ => {
            // Several documents: a data file still when one of them holds `artifact`; that one is
            // checked as a single document would be, and the file is red for its several documents.
            let many = format!("the data file holds {} YAML documents; it is one", docs.len());
            let Some(d) = holding_artifact(&docs) else {
                return unfit(many, page_exists);
            };
            match check_doc(d, page, page_exists) {
                Data::Checked { roles, problems } => Data::Checked { roles, problems: std::iter::once(many).chain(problems).collect() },
                // A draft does not silence a file of several documents (it holds `artifact`, so never None).
                Data::None | Data::Draft => Data::Checked { roles: vec![], problems: vec![many] },
            }
        }
    }
}

/// A data file of the wrong shape: red beside its page, other data with none.
fn unfit(problem: String, page_exists: bool) -> Data {
    if page_exists {
        Data::Checked { roles: vec![], problems: vec![problem] }
    } else {
        Data::None
    }
}

/// Checks one YAML document of a data file against its role's fields (DEC-69); `page` and
/// `page_exists` as in `read_data`.
fn check_doc(doc: &Yaml, page: &str, page_exists: bool) -> Data {
    let Yaml::Hash(h) = doc else {
        return unfit("the data file is not a mapping of fields".into(), page_exists);
    };
    let get = |k: &str| h.get(&Yaml::String(k.into()));
    if get("artifact").is_none() && !page_exists {
        return Data::None;
    }
    if get("status").and_then(Yaml::as_str) == Some("draft") {
        return Data::Draft;
    }
    let mut problems = vec![];
    let roles: Vec<String> = match get("artifact") {
        None => return Data::Checked { roles: vec![], problems: vec!["missing field \"artifact\"".into()] },
        Some(a) => match artifact_roles(a) {
            Ok(rs) => rs.into_iter().map(|r| r.unwrap_or_else(|r| format!("{r:?}"))).collect(),
            Err(other) => vec![format!("{other:?}")],
        },
    };
    if let Some(p) = role_problem(&roles) {
        problems.push(p);
        return Data::Checked { roles: vec![], problems };
    }
    let rs: Vec<&str> = roles.iter().map(String::as_str).collect();
    let (required, known) = fields(&rs);
    for k in h.keys() {
        let key = k.as_str().map_or_else(|| format!("{k:?}"), String::from);
        if !known.contains(&key.as_str()) {
            problems.push(format!("unknown field \"{key}\" (typo? expected one of: {})", known.join(", ")));
        }
    }
    for f in known.iter().filter(|f| **f != "details") {
        match get(f) {
            None if required.contains(f) => problems.push(format!("missing field \"{f}\"")),
            Some(v) if empty(v) => problems.push(format!("field \"{f}\" is empty")),
            _ => {}
        }
    }
    let wu_doc = rs.iter().all(|r| r.starts_with("wu-"));
    for (f, like) in [("status", Some("ready")), ("id", Some(if wu_doc { WU_ID } else { EPIC_ID })), ("epic", Some(EPIC_ID)), ("title", None)] {
        if let Some(p) = get(f).and_then(|v| wrong_type(f, v, like)) {
            problems.push(p);
        }
    }
    let text = |k: &str| get(k).and_then(Yaml::as_str).map(str::trim).filter(|s| !s.is_empty());
    if let Some(s) = text("status").filter(|s| *s != "ready") {
        problems.push(format!("status \"{s}\" is neither draft nor ready"));
    }
    if let Some(d) = get("date").filter(|d| !empty(d)) {
        if !d.as_str().is_some_and(is_date) {
            problems.push(format!("date {} is not a quoted \"YYYY-MM-DD\"", shown(d)));
        }
    }
    if let Some(p) = get("page").filter(|p| !empty(p)) {
        if p.as_str() != Some(page) {
            problems.push(format!("page {} is not this data file's page: write \"{page}\"", shown(p)));
        } else if !page_exists {
            problems.push(format!("page {page} is not beside the data file"));
        }
    }
    let id = text("id");
    if wu_doc {
        if let (Some(id), Some(epic)) = (id, text("epic")) {
            if !is_wu_of(id, epic) {
                problems.push(format!("id \"{id}\" is not a WU of epic \"{epic}\": write {epic}.W<n>"));
            }
        }
    }
    for (f, entries, check) in LIST_FIELDS.iter().filter(|(f, ..)| required.contains(f)) {
        if let Some(v) = get(f).filter(|v| !empty(v)) {
            problems.extend(check(v, entries, id));
        }
    }
    Data::Checked { roles, problems }
}

/// Why these `artifact` roles give a page no role, if they do not: none, a name that is no role, or
/// roles that cannot share one doc. The one rule the doc check and the sign-off's finder share.
fn role_problem(roles: &[String]) -> Option<String> {
    if roles.is_empty() {
        return Some("field \"artifact\" is empty".into());
    }
    if let Some(r) = roles.iter().find(|r| !ROLES.contains(&r.as_str())) {
        return Some(format!("unknown artifact \"{r}\" (expected one of: {})", ROLES.join(", ")));
    }
    combination_problem(&roles.iter().map(String::as_str).collect::<Vec<_>>())
}

/// A one-part epic pairs an epic role with the WU role of the same kind; nothing else shares a doc.
fn combination_problem(roles: &[&str]) -> Option<String> {
    let mut set = roles.to_vec();
    set.sort();
    set.dedup();
    match set.as_slice() {
        [_] | ["epic-plan", "wu-plan"] | ["epic-result", "wu-result"] => None,
        _ => Some(format!("artifact {} cannot share one doc: a one-part epic pairs epic-plan with wu-plan, or epic-result with wu-result", set.join(" and "))),
    }
}

/// (required, known) fields for these roles: the common ones, `epic` for a WU's own doc, and the
/// union of the roles' checklists, a shared field once.
fn fields(roles: &[&str]) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut required = COMMON.to_vec();
    let add = |v: &mut Vec<&'static str>, f: &'static str| {
        if !v.contains(&f) {
            v.push(f);
        }
    };
    if !roles.is_empty() && roles.iter().all(|r| r.starts_with("wu-")) {
        add(&mut required, "epic");
    }
    for r in roles {
        for f in role_fields(r) {
            add(&mut required, f);
        }
    }
    let mut known = required.clone();
    for f in OPTIONAL {
        add(&mut known, f);
    }
    (required, known)
}

/// A role's own required checklist, beyond the common fields and `epic` (`ROLE_TABLE`); none for a
/// name that is no role.
fn role_fields(role: &str) -> &'static [&'static str] {
    ROLE_TABLE.iter().find(|(n, ..)| *n == role).map_or(&[], |(_, f, _)| f)
}

/// What reads a list field's entries: its value, its entries, the doc's `id`; its problems.
type EntryCheck = fn(&Yaml, &[&str], Option<&str>) -> Vec<String>;

/// The list fields whose entries the check reads, each with its entries and the checker that reads
/// them, in the order their problems come: an epic plan's `wu-summaries` and each WU's fields, a WU
/// plan's `test-plan` and each level. The one map from a field to its entries and its checker.
const LIST_FIELDS: &[(&str, &[&str], EntryCheck)] = &[("wu-summaries", WU_SUB, wu_summaries), ("test-plan", TEST_LEVELS, |t, levels, _| test_plan(t, levels))];

/// The cap a page with these roles holds (DEC-69): a one-part epic takes the larger, its epic's.
fn role_cap(roles: &[String]) -> Option<usize> {
    roles.iter().filter_map(|r| ROLE_TABLE.iter().find(|(n, ..)| n == r).map(|(.., c)| *c)).max()
}

/// How deep a data file's collections may nest, the root mapping counted as one level. The loader
/// recurses once per level, so a few thousand levels overflow the stack; a plan or result needs a
/// handful.
const MAX_DEPTH: usize = 64;
/// How many bytes a data file may hold; the largest of Keel's own is under 50 KB. Checked on the
/// file's size before it is read (`size_problem`).
const MAX_DATA_BYTES: usize = 1 << 20;

/// Why a data file of `len` bytes is not read at all, if it is not: over `MAX_DATA_BYTES`. Asked of
/// a file's size before its bytes are read (the doc check's metadata, the sign-off's `git cat-file
/// -s`), and again of the text in hand.
pub fn size_problem(len: u64) -> Option<String> {
    (len > MAX_DATA_BYTES as u64).then(|| format!("the data file is {len} bytes, over the {MAX_DATA_BYTES} a data file may hold"))
}

/// A data file's anchors and aliases are refused, not expanded: the loader copies an alias's value
/// at each use with no bound, so a few hundred bytes of nested ones (billion laughs) would hang.
const ANCHORS: &str = "the data file uses a YAML anchor or alias (&name, *name): write each value out";

/// Why a data file's text must not reach the loader, if it must not: over `MAX_DATA_BYTES`, nested
/// deeper than `MAX_DEPTH`, or holding an anchor or an alias (`ANCHORS`). Read from the parser's
/// events one at a time, in a loop with a depth count: no recursion, and no alias expanded. Text that
/// does not parse stops the walk; the loader names it.
fn shape_problem(src: &str) -> Option<String> {
    if let Some(p) = size_problem(src.len() as u64) {
        return Some(p);
    }
    let mut parser = Parser::new_from_str(src);
    let mut depth = 0usize;
    loop {
        match parser.next_token() {
            Err(_) | Ok((Event::StreamEnd, _)) => return None,
            Ok((Event::Alias(_), _)) => return Some(ANCHORS.into()),
            Ok((Event::Scalar(_, _, a, _), _)) if a > 0 => return Some(ANCHORS.into()),
            Ok((Event::SequenceStart(a, _) | Event::MappingStart(a, _), _)) => {
                if a > 0 {
                    return Some(ANCHORS.into());
                }
                depth += 1;
                if depth > MAX_DEPTH {
                    return Some(format!("the data file nests deeper than {MAX_DEPTH} levels: write it flatter"));
                }
            }
            Ok((Event::SequenceEnd | Event::MappingEnd, _)) => depth = depth.saturating_sub(1),
            Ok(_) => {}
        }
    }
}

const WU_ID: &str = "E-<CODE>.W<n>";
const EPIC_ID: &str = "E-<CODE>";

/// A field that must be text but holds another non-empty type (`true`, `7`, a list): its problem.
fn wrong_type(field: &str, v: &Yaml, like: Option<&str>) -> Option<String> {
    if v.as_str().is_some() || empty(v) {
        return None;
    }
    let what = match v {
        Yaml::Array(_) => "a list".to_string(),
        Yaml::Hash(_) => "a mapping".to_string(),
        other => shown(other),
    };
    let like = like.map(|l| format!(", such as \"{l}\"")).unwrap_or_default();
    Some(format!("field \"{field}\" is the wrong type ({what}): write it as text{like}"))
}

/// Empty: null, blank text, or a list or mapping holding nothing but empty values.
fn empty(y: &Yaml) -> bool {
    match y {
        Yaml::Null | Yaml::BadValue => true,
        Yaml::String(s) => s.trim().is_empty(),
        Yaml::Array(a) => a.iter().all(empty),
        Yaml::Hash(h) => h.values().all(empty),
        _ => false,
    }
}

/// A value as a problem line shows it: text quoted, anything else as YAML would read it.
fn shown(y: &Yaml) -> String {
    match y {
        Yaml::String(s) => format!("\"{s}\""),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Real(r) => r.clone(),
        Yaml::Boolean(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

/// `wu` is `<epic>.W<n>`.
fn is_wu_of(wu: &str, epic: &str) -> bool {
    wu.strip_prefix(epic).and_then(|r| r.strip_prefix(".W")).is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// An epic plan's `wu-summaries`: a list, one entry per WU, each with its fields `subs`; each `wu`
/// is `<id>.W<n>` and listed once; each `wu-deps` is "none" or names other WUs of this list.
fn wu_summaries(w: &Yaml, subs: &[&str], epic: Option<&str>) -> Vec<String> {
    let mut out = vec![];
    let Yaml::Array(entries) = w else {
        return vec!["field \"wu-summaries\" is not a list: write one entry per WU".into()];
    };
    let key = |k: &str| Yaml::String(k.into());
    let mut ids: HashSet<&str> = HashSet::new();
    for (n, e) in entries.iter().enumerate() {
        let n = n + 1;
        let Yaml::Hash(h) = e else {
            out.push(format!("wu-summaries entry {n} is not a mapping of {}", subs.join(", ")));
            continue;
        };
        let wu = h.get(&key("wu")).and_then(Yaml::as_str).map(str::trim).filter(|s| !s.is_empty());
        let at = format!("wu-summaries entry {n} ({})", wu.unwrap_or("no wu"));
        if let Some(p) = h.get(&key("wu")).and_then(|v| wrong_type("wu", v, Some(WU_ID))) {
            out.push(format!("{at}: {p}"));
        }
        for k in h.keys() {
            let name = k.as_str().map_or_else(|| format!("{k:?}"), String::from);
            if !subs.contains(&name.as_str()) {
                out.push(format!("{at}: unknown field \"{name}\" (typo? expected one of: {})", subs.join(", ")));
            }
        }
        for f in subs {
            match h.get(&key(f)) {
                None => out.push(format!("{at}: missing field \"{f}\"")),
                Some(v) if empty(v) => out.push(format!("{at}: field \"{f}\" is empty")),
                _ => {}
            }
        }
        if let Some(wu) = wu {
            if epic.is_some_and(|e| !is_wu_of(wu, e)) {
                out.push(format!("{at}: wu \"{wu}\" is not a WU of this epic: write {}.W<n>", epic.unwrap_or_default()));
            }
            if !ids.insert(wu) {
                out.push(format!("{at}: wu \"{wu}\" is listed twice"));
            }
        }
    }
    for e in entries {
        let Yaml::Hash(h) = e else { continue };
        let Some(wu) = h.get(&key("wu")).and_then(Yaml::as_str).map(str::trim) else { continue };
        let deps: Vec<&Yaml> = match h.get(&key("wu-deps")) {
            Some(Yaml::String(s)) if s.trim().eq_ignore_ascii_case("none") => vec![],
            Some(Yaml::Array(a)) => a.iter().collect(),
            Some(d) if !empty(d) => vec![d],
            _ => vec![],
        };
        for d in deps {
            match d.as_str().map(str::trim) {
                Some(d) if d == wu => out.push(format!("wu-deps of {wu} names {wu} itself")),
                Some(d) if ids.contains(&d) => {}
                _ => out.push(format!("wu-deps of {wu} names {}, which is not a WU of this plan (write \"none\" when it waits for none)", shown(d))),
            }
        }
    }
    out
}

/// A WU plan's `test-plan`: a mapping that gives each level of `levels` its tests, or "none" or a
/// waiver with why.
fn test_plan(t: &Yaml, levels: &[&str]) -> Vec<String> {
    let Yaml::Hash(h) = t else {
        return vec![format!("field \"test-plan\" is not a mapping: give each level, {}", levels.join(", "))];
    };
    let mut out = vec![];
    for l in levels {
        match h.get(&Yaml::String((*l).into())) {
            None => out.push(format!("test-plan: missing level \"{l}\" (its tests, or \"none: <why>\")")),
            Some(v) if empty(v) => out.push(format!("test-plan: level \"{l}\" is empty")),
            _ => {}
        }
    }
    out
}

/// A page's own problems: diagram labels and relative links, and its word cap - by `roles` when a
/// ready data file gives it some, else by `path_cap`. With no data file (`roles` is `None`), a page
/// marked `data-status="draft"` is silent.
fn page_problems(path: &Path, roles: Option<&[String]>, path_cap: Option<(&str, usize)>) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return vec!["cannot read file".into()];
    };
    let html = scannable(&raw);
    let ts = tags(&html);
    let mut errs = vec![];
    if roles.is_none() && root(&ts).is_some_and(|r| attr(r, "data-status") == Some("draft")) {
        return errs;
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

    let roles = roles.filter(|r| !r.is_empty());
    if roles.is_none() && path_cap.is_none() {
        return errs; // no cap applies: no words to count
    }
    let (words, top, marked) = page_words(&html, &ts);
    match (roles, path_cap) {
        (Some(roles), _) => {
            let role = roles.join(" + ");
            if let Some(cap) = role_cap(roles).filter(|c| words > *c) {
                errs.push(format!("page has {words} words, over the {role} cap of {cap}: cut it, or move details to the data file"));
            }
            if roles.iter().any(|r| r == "epic-plan") && top > TOP_CAP {
                errs.push(if marked {
                    format!("epic plan top has {top} words above the heading with id=\"{WORK_ID}\", over the cap of {TOP_CAP}: cut it, or move details below the parts")
                } else {
                    format!("epic plan top has {top} words, over the cap of {TOP_CAP}: no heading has id=\"{WORK_ID}\", so the whole page is the top - mark the parts' heading with it, or cut")
                });
            }
        }
        (None, Some((glob, cap))) if words > cap => {
            errs.push(format!("page has {words} words, over the cap of {cap} that [docs.caps] sets for {glob}: cut it"));
        }
        _ => {}
    }
    errs
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
        let hit = entity_span(rest).and_then(|(name, len)| {
            let named = name.bytes().all(|c| c.is_ascii_alphanumeric());
            entity(name).or(unknown.filter(|_| named)).map(|c| (c, len))
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

/// At an `&name;` (within 32 bytes), the name and the length of the whole reference.
fn entity_span(rest: &str) -> Option<(&str, usize)> {
    let n = rest.strip_prefix('&')?.bytes().take(32).position(|c| c == b';').filter(|&n| n > 0)?;
    Some((&rest[1..=n], n + 2))
}

fn entity(name: &str) -> Option<char> {
    if let Some(n) = name.strip_prefix('#') {
        // Digits only: a browser leaves a signed reference (`&#+34;`) as text.
        let code = match n.strip_prefix(['x', 'X']) {
            Some(h) if h.bytes().all(|c| c.is_ascii_hexdigit()) => u32::from_str_radix(h, 16).ok()?,
            None if n.bytes().all(|c| c.is_ascii_digit()) => n.parse().ok()?,
            _ => return None,
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

/// Words on a page and in its top, and whether a heading with id `work` ends that top: the text of
/// `<main>` (else `<body>`, else the whole file), the top running from its start to the element with
/// id `work`, or to its end when none has it. A word is a whitespace-separated run, entities decoded,
/// holding a letter or digit: a dash, arrow, `·` or unknown `&name;` is not one; inline tags join
/// the text around them, block tags (`BLOCK`) separate it.
fn page_words(html: &str, ts: &[Tag]) -> (usize, usize, bool) {
    let open = |n: &str| ts.iter().position(|t| t.name == n && !t.closing);
    let (start, end) = match open("main").or_else(|| open("body")) {
        Some(i) => span(ts, i).unwrap_or((ts[i].end, html.len())),
        None => (0, html.len()),
    };
    let work = ts.iter().find(|t| !t.closing && t.start >= start && t.start < end && attr(t, "id") == Some(WORK_ID)).map(|t| t.start);
    // The text is built and decoded once; a word is in the top when it starts before the work heading.
    let (raw, cut) = text(html, ts, start, end, work);
    let mut map = vec![];
    let words = decode_core(&raw, Some('\u{fffd}'), work.is_some().then_some(&mut map));
    let top_end = if work.is_some() { map.partition_point(|&m| m < cut) } else { words.len() };
    let (mut all, mut top) = (0, 0);
    for w in words.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)) {
        all += 1;
        top += usize::from(w.as_ptr() as usize - (words.as_ptr() as usize) < top_end);
    }
    (all, top, work.is_some())
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
        let quote = if rest.starts_with('"') { Some(1) } else { entity_span(rest).filter(|&(name, _)| entity(name) == Some('"')).map(|(_, n)| n) };
        rest = match quote {
            Some(n) => {
                out.push_str("#quot;");
                &rest[n..]
            }
            None => {
                out.push('&');
                &rest[1..]
            }
        };
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
    use super::{
        data_roles, decode, decode_mapped, fields, hits, page_words, pages_of, pair_named, pair_of, partner_named, read_data, result_role, role_cap,
        role_fields, size_problem, tags, under_root, unquoted_labels, Data, Pair, ANCHORS, LIST_FIELDS, MAX_DATA_BYTES, MAX_DEPTH, ROLES,
    };
    use std::path::Path;

    #[test]
    fn the_result_role_a_sign_off_attests_is_one_the_data_file_knows() {
        for epic in [false, true] {
            let role = result_role(epic);
            assert!(ROLES.contains(&role), "{role}");
            let (required, _) = fields(&[role]);
            assert!(required.contains(&"outcome") && required.contains(&"follow-ups"), "{role}: {required:?}");
        }
    }

    #[test]
    fn data_roles_reads_artifact_as_text_or_a_list() {
        assert_eq!(data_roles("artifact: wu-result\nstatus: ready\n"), Ok(vec!["wu-result".to_string()]));
        let both = Ok(vec!["epic-result".to_string(), "wu-result".to_string()]);
        assert_eq!(data_roles("artifact: epic-result wu-result\n"), both);
        assert_eq!(data_roles("artifact: [epic-result, wu-result]\n"), both);
        assert_eq!(data_roles("artifact:\n  - epic-result\n  - wu-result\n"), both);
        assert_eq!(data_roles("\u{feff}artifact: wu-plan\n"), Ok(vec!["wu-plan".to_string()]), "a byte-order mark is read past");
        assert_eq!(data_roles("x: 1\n---\nartifact: wu-result\n"), Ok(vec!["wu-result".to_string()]), "the document holding artifact");
    }

    /// Epic review round 1 (E-DOCSPLIT close, important): the sign-off's report finder reads the
    /// roles the doc check accepts, or none - a data file red at the doc check is never a report.
    #[test]
    fn data_roles_gives_no_role_where_the_doc_check_finds_a_role_problem() {
        let err = |src: &str| data_roles(src).unwrap_err();
        assert!(err("artifact: notes\n").starts_with("unknown artifact \"notes\""), "{}", err("artifact: notes\n"));
        assert!(err("artifact: wu-result bogus\n").starts_with("unknown artifact \"bogus\""), "{}", err("artifact: wu-result bogus\n"));
        assert!(err("artifact: [wu-result, bogus]\n").starts_with("unknown artifact \"bogus\""));
        assert!(err("artifact: wu-result epic-plan\n").contains("cannot share one doc"), "{}", err("artifact: wu-result epic-plan\n"));
        for src in ["artifact: notes\n", "artifact: wu-result bogus\n", "artifact: wu-result epic-plan\n"] {
            let Data::Checked { roles, problems } = read_data(src, "a.html", true) else { panic!("{src:?} is checked") };
            assert!(roles.is_empty() && problems.iter().any(|p| p == &err(src)), "{src:?}: {problems:?} vs {}", err(src));
        }
    }

    /// Epic review round 1 (E-DOCSPLIT close, blocker): the YAML loader recurses once per nesting
    /// level, so a deeply nested data file overflowed the stack (exit 134) in the doc check and the
    /// sign-off. Its shape is read first, without recursion, and refused past the bounds.
    #[test]
    fn a_deeply_nested_or_oversized_data_file_is_refused_before_it_is_loaded() {
        let deep = format!("artifact: wu-result\nx:\n{}a\n", "- ".repeat(5000));
        let nested = |e: &str| e.contains("nests deeper than") && e.contains(&MAX_DEPTH.to_string());
        assert!(data_roles(&deep).is_err_and(|e| nested(&e)), "{:?}", data_roles(&deep));
        let Data::Checked { roles, problems } = read_data(&deep, "a.html", true) else { panic!("checked") };
        assert!(roles.is_empty() && problems.len() == 1 && nested(&problems[0]), "{problems:?}");
        // flow collections nest too; and the bound counts levels, not lines
        let flow = format!("artifact: wu-result\nx: {}a{}\n", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(data_roles(&flow).is_err_and(|e| nested(&e)), "{:?}", data_roles(&flow));
        // the root mapping is level 1: MAX_DEPTH levels in all pass, one more is refused
        let at = |n: usize| format!("artifact: wu-result\nx:\n{}a\n", "- ".repeat(n));
        assert_eq!(data_roles(at(MAX_DEPTH - 1)), Ok(vec!["wu-result".to_string()]));
        assert!(data_roles(at(MAX_DEPTH)).is_err_and(|e| nested(&e)));
        let big = format!("artifact: wu-result\nx: \"{}\"\n", "a".repeat(MAX_DATA_BYTES));
        assert!(data_roles(&big).is_err_and(|e| e.contains("bytes") && e.contains(&MAX_DATA_BYTES.to_string())), "{:?}", data_roles(&big).map_err(|e| e.len()));
        // an anchor is still named, wherever it sits
        assert_eq!(data_roles("artifact: wu-result\nx:\n  - &a [1]\n"), Err(ANCHORS.to_string()));
    }

    /// Epic review round 2 (E-DOCSPLIT close, important): `data_roles` takes the file's bytes as they
    /// are - no trim, no lossy decoding - so it refuses what the doc check refuses.
    #[test]
    fn data_roles_reads_the_bytes_as_the_doc_check_reads_the_file() {
        let not_utf8: &[u8] = b"artifact: wu-result\ntitle: caf\xe9\n";
        assert!(data_roles(not_utf8).is_err_and(|e| e.contains("UTF-8")), "{:?}", data_roles(not_utf8));
        let indented = "  x: 1\nartifact: wu-result\n";
        assert!(data_roles(indented).is_err_and(|e| e.starts_with("not valid YAML")), "{:?}", data_roles(indented));
        assert!(matches!(read_data(indented, "a.html", true), Data::Checked { ref roles, .. } if roles.is_empty()));
        assert_eq!(data_roles("artifact: wu-result\n".as_bytes()), Ok(vec!["wu-result".to_string()]));
        assert_eq!(size_problem(MAX_DATA_BYTES as u64), None);
        assert!(size_problem(MAX_DATA_BYTES as u64 + 1).is_some_and(|e| e.contains("bytes")));
    }

    /// Epic review round 1 (E-DOCSPLIT close, important): each role has its fields and its cap from
    /// one table, so a role cannot be known to the check without both.
    #[test]
    fn every_role_has_its_own_fields_and_a_cap() {
        for role in ROLES {
            assert!(!role_fields(role).is_empty(), "{role} has no fields of its own");
            assert!(role_cap(&[role.to_string()]).is_some_and(|c| c > 0), "{role} has no cap");
            assert_eq!(data_roles(format!("artifact: {role}\n")), Ok(vec![role.to_string()]), "{role} is a role data_roles accepts");
        }
    }

    #[test]
    fn data_roles_names_a_data_file_that_gives_no_role() {
        let err = |src: &str| data_roles(src).unwrap_err();
        assert!(err("artifact: [wu-result\n").starts_with("not valid YAML"), "{}", err("artifact: [wu-result\n"));
        assert_eq!(err("status: ready\n"), "no field \"artifact\"");
        assert_eq!(err("- artifact\n"), "no field \"artifact\"", "a list is no mapping");
        assert_eq!(err(""), "no field \"artifact\"");
        assert_eq!(err("artifact: \"\"\n"), "field \"artifact\" is empty");
        assert_eq!(err("artifact: []\n"), "field \"artifact\" is empty");
        assert_eq!(err("artifact: 3\n"), "artifact 3 is neither text nor a list");
        assert_eq!(err("artifact: [wu-result, 3]\n"), "artifact holds 3, not a role");
        assert_eq!(err("a: &x wu-result\nartifact: *x\n"), ANCHORS);
    }

    /// Review round 1 (E-DOCSPLIT.W3): the doc check and `data_roles` read `artifact` through one
    /// loader and one roles reading, so a data file gives its page the same roles in both.
    #[test]
    fn the_doc_check_and_data_roles_read_the_same_roles() {
        for src in [
            "artifact: wu-result\n",
            "artifact: \"  wu-plan \"\n",
            "artifact: [epic-plan]\n",
            "\u{feff}artifact: wu-result\n",
            "x: 1\n---\nartifact: epic-result\n",
        ] {
            let Data::Checked { roles, .. } = read_data(src, "a.html", true) else { panic!("{src:?} is checked") };
            assert_eq!(Ok(roles), data_roles(src), "{src:?}");
        }
        for src in ["artifact: 3\n", "artifact: [wu-result, 3]\n", "a: &x wu-result\nartifact: *x\n", "artifact: [wu-result\n"] {
            let Data::Checked { roles, problems } = read_data(src, "a.html", true) else { panic!("{src:?} is checked") };
            assert!(roles.is_empty() && !problems.is_empty() && data_roles(src).is_err(), "{src:?}: {problems:?}");
        }
    }

    #[test]
    fn pair_named_pairs_by_name_alone_under_docs_root_only() {
        // Nothing is on disk: the names alone decide, as at a tip.
        let in_docs = under_root("docs/");
        let pair = |page: &str, data: Option<&str>| Some(Pair { page: page.into(), data: data.map(Into::into) });
        assert_eq!(pair_named(Path::new("docs/w/a.html"), &in_docs), pair("docs/w/a.html", Some("docs/w/a.yaml")), "named, there or not");
        assert_eq!(pair_named(Path::new("docs/w/a.yaml"), &in_docs), pair("docs/w/a.html", Some("docs/w/a.yaml")));
        assert_eq!(pair_named(Path::new("notes/a.html"), &in_docs), pair("notes/a.html", None), "a page outside docs.root has no data file");
        assert_eq!(pair_named(Path::new("notes/a.yaml"), &in_docs), None, "a .yaml outside docs.root is no doc");
        assert_eq!(pair_named(Path::new("docsx/a.yaml"), &in_docs), None, "docs.root by component, not by prefix");
        for none in ["docs/a.md", "docs/a.yml", "docs/.yaml", "docs/.html"] {
            assert_eq!(pair_named(Path::new(none), &in_docs), None, "{none}");
        }
        assert_eq!(partner_named(Path::new("docs/a.html"), &in_docs), Some("docs/a.yaml".into()));
        assert_eq!(partner_named(Path::new("docs/a.yaml"), &in_docs), Some("docs/a.html".into()));
        assert_eq!(partner_named(Path::new("notes/a.html"), &in_docs), None);
        assert_eq!(partner_named(Path::new("notes/a.yaml"), &in_docs), None);
        assert!(under_root("docs")(Path::new("docs/a.html")) && !under_root("docs")(Path::new("src/docs/a.html")));
    }

    #[test]
    fn a_page_tag_gives_no_role() {
        // DEC-69's clean cut: a page is never read as a data file, whatever tag it carries.
        assert!(data_roles(r#"<html data-artifact="wu-result"><p>done</p></html>"#).is_err());
    }

    #[test]
    fn pages_of_maps_changed_paths_to_doc_pages_each_once() {
        let names: Vec<String> =
            ["docs/work/a-plan.yaml", "docs/work/a-plan.html", "docs/work/b-result.yaml", "docs/x.css", "src/c.yaml", "docs/d.html", "README"]
                .map(String::from)
                .to_vec();
        let in_docs = |p: &Path| p.starts_with("docs");
        assert_eq!(pages_of(&names, in_docs), ["docs/work/a-plan.html", "docs/work/b-result.html", "docs/d.html"]);
        assert!(pages_of(&[], in_docs).is_empty());
        assert_eq!(pages_of(&["src/c.yaml".to_string()], |_: &Path| true), ["src/c.html"], "in_docs decides what a .yaml is");
    }

    #[test]
    fn pages_of_and_pair_of_follow_pair_named_odd_names_too() {
        // pair_named is the one pairing rule: pages_of gives each name's pair page, and pair_of its
        // pair, a data file named there or not, a page's data file only when it is on disk.
        let d = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(d.path().join("docs")).unwrap();
        std::fs::write(d.path().join("docs/a.yaml"), "").unwrap();
        let in_docs = |p: &Path| p.starts_with(d.path().join("docs"));
        let names = [
            "docs/a.html",
            "docs/a.yaml",
            "docs/b.html",
            "docs/b.yaml",
            "docs/.html",
            "docs/.yaml",
            "docs/a.yml",
            "docs/a.HTML",
            "docs/a.YAML",
            "notes/a.html",
            "notes/a.yaml",
            "notes/a.md",
            "README",
        ];
        for n in names.map(|n| d.path().join(n)) {
            let named = pair_named(&n, in_docs);
            let s = n.to_string_lossy().into_owned();
            let want: Vec<String> = named.iter().map(|p| p.page.to_string_lossy().into_owned()).collect();
            assert_eq!(pages_of(std::slice::from_ref(&s), in_docs), want, "{s}");
            let on_disk = match named {
                Some(Pair { page, data }) if page == n => Some(Pair { page, data: data.filter(|x| x.is_file()) }),
                Some(data_file) => Some(data_file),
                None if n.extension().is_some_and(|e| e == "yaml") => None,
                None => Some(Pair { page: n.clone(), data: None }),
            };
            assert_eq!(pair_of(&n, in_docs), on_disk, "{s}");
        }
        let p = |n: &str| pair_of(&d.path().join(n), in_docs);
        assert_eq!(p("docs/b.yaml").and_then(|x| x.data), Some(d.path().join("docs/b.yaml")), "a data file is named, there or not");
        assert_eq!(p("docs/b.html").and_then(|x| x.data), None, "a page's data file only when it is there");
        assert_eq!(p("notes/a.yaml"), None, "a .yaml outside docs.root is no doc");
        assert_eq!(p("docs/a.yml").map(|x| x.page), Some(d.path().join("docs/a.yml")), "any other path is a page alone");
    }

    #[test]
    fn pages_of_names_each_page_once_whatever_the_order_of_its_paths() {
        let mut seed: u64 = 7;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n
        };
        let stems = ["docs/work/a", "docs/work/b", "docs/c"];
        for _ in 0..200 {
            let names: Vec<String> = (0..next(8)).map(|_| format!("{}.{}", stems[next(3)], ["html", "yaml"][next(2)])).collect();
            let pages = pages_of(&names, |p: &Path| p.starts_with("docs"));
            let mut want: Vec<String> = names.iter().map(|n| format!("{}.html", n.rsplit_once('.').unwrap().0)).collect();
            want.dedup();
            let mut seen = vec![];
            want.retain(|w| {
                if seen.contains(w) {
                    false
                } else {
                    seen.push(w.clone());
                    true
                }
            });
            assert_eq!(pages, want, "{names:?}");
        }
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
        assert_eq!(
            labels("flowchart LR\n  A -- yes --> B\n  B == go ==> C\n  C -. later .-> D\n  D <-- back --> A"),
            ["-- yes -->", "== go ==>", "-. later .->", "-- back -->"]
        );
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
        assert!(super::in_quote("A[\"a\n  b") && super::in_quote("A[&quot;a"));
        assert!(!super::in_quote("%% 5&quot; screen\n  style A x:\"\n  classDef c y:&#34;\n  A[\"a\"] --> "), "skipped lines do not count");
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
        assert!(
            labels("flowchart LR\n  A -- \"wait...\" --> B(\"retry (3x)\")").is_empty(),
            "{:?}",
            labels("flowchart LR\n  A -- \"wait...\" --> B(\"retry (3x)\")")
        );
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
            let src =
                format!("flowchart LR\n  {open}\n    a diagram (x) of [y]\n    and more {{z}}\n  }}\n  accTitle: the [t]\n  accDescr: one (line)\n  A[raw]");
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
        assert_eq!(labels("%%{\n  init: {'theme': 'dark'}\n}%%\nflowchart LR\n  A[\"a\"] --> B[\"b\"]\n  B --> C[c]"), ["[c]"]);
        assert_eq!(labels("%%{init: {'theme': 'dark'}}%% flowchart LR\n  A[x]"), ["[x]"]);
        assert_eq!(labels("flowchart LR\n  %%{init: {'theme': 'dark'}}%%\n  A[a]\n  %%{\n  init: {'x': 'y'}\n  }%%\n  B[\"b\"] --> C[c]"), ["[a]", "[c]"]);
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
        page_words(html, &ts).1
    }

    #[test]
    fn the_top_runs_from_main_or_body_to_the_work_heading_and_counts_words_with_a_letter_or_digit() {
        assert_eq!(top("<body><header>one two</header><main><p>three four</p><h2 id=\"work\">five</h2><p>six</p></main></body>"), 2);
        assert_eq!(top("<body><header>one two</header><p>three</p><h2 id=\"work\">four</h2></body>"), 3);
        assert_eq!(top("<main><p>a · b — c --> | 1</p></main>"), 4);
        assert_eq!(top("<main><td>a</td><td>b</td>x&amp;y A &lt;-- B</main>"), 5);
        assert_eq!(top("<main><pre>A <-- B</pre></main>"), 2);
        assert_eq!(top("<main><!-- hidden words --><p>shown</p></main>"), 1);
        assert_eq!(top("<p>no main nor body</p><h2 id=\"work\">x</h2>"), 4);
        assert_eq!(top("<main><p>one</p><div data-wu=\"W1\">two</div></main>"), 2, "a data-wu tag no longer ends the top");
    }

    #[test]
    fn a_page_counts_its_main_and_marks_whether_a_work_heading_ends_its_top() {
        let html = "<html><head><title>t t</title></head><body><header>a b c</header><main><p>one two</p><h2 id=\"work\">three</h2><p>four</p></main><footer>x</footer></body></html>";
        let ts = tags(html);
        assert_eq!(page_words(html, &ts), (4, 2, true));
        let html = "<body><p>one two</p><h2 id=\"other\">three</h2></body>";
        let ts = tags(html);
        assert_eq!(page_words(html, &ts), (3, 3, false));
        let html = "<main><p data-htmldoc-id=\"work\">one</p><p>two</p></main>";
        let ts = tags(html);
        assert_eq!(page_words(html, &ts), (2, 2, false), "only an id attribute marks the work");
        let html = "<main><p>a &amp; b &mdash; c&#32;d &zz;</p><h2 id=\"work\">e&nbsp;f</h2><p>g</p></main>";
        let ts = tags(html);
        assert_eq!(page_words(html, &ts), (7, 4, true), "decoded once, the top split where the heading stands");
    }

    #[test]
    fn the_caps_are_dec_69s_and_a_one_part_epic_takes_the_epic_cap() {
        let cap = |r: &[&str]| role_cap(&r.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(cap(&["epic-plan"]), Some(700));
        assert_eq!(cap(&["wu-plan"]), Some(500));
        assert_eq!(cap(&["wu-result"]), Some(500));
        assert_eq!(cap(&["epic-result"]), Some(700));
        assert_eq!(cap(&["epic-plan", "wu-plan"]), Some(700));
        assert_eq!(cap(&["wu-result", "epic-result"]), Some(700));
        assert_eq!(cap(&[]), None);
        assert_eq!(super::TOP_CAP, 300);
    }

    #[test]
    fn the_fields_per_role_are_the_checklists_plus_the_common_ones() {
        let common = ["artifact", "status", "id", "title", "date", "page"];
        for (roles, own) in [
            (
                &["epic-plan"][..],
                &["intent", "owner-changes", "use-cases", "expected", "non-goals", "wu-summaries", "e2e-plan", "distillation-plan", "open-questions"][..],
            ),
            (&["wu-plan"], &["epic", "requirement", "maps-to", "expected", "test-plan"]),
            (&["wu-result"], &["epic", "outcome", "deviations", "evidence", "distillation", "follow-ups"]),
            (&["epic-result"], &["outcome", "wu-rollup", "deviations", "evidence", "distillation", "follow-ups"]),
        ] {
            let (required, known) = fields(roles);
            let want: Vec<&str> = common.iter().chain(own).copied().collect();
            assert_eq!(required, want, "{roles:?}");
            assert!(known.contains(&"details") && known.contains(&"owner-changes"), "{roles:?}: {known:?}");
        }
        let (required, _) = fields(&["epic-plan", "wu-plan"]);
        assert!(!required.contains(&"epic"), "a one-part epic is its own epic");
        assert_eq!(required.iter().filter(|f| **f == "expected").count(), 1, "a shared field once");
        assert!(required.contains(&"test-plan") && required.contains(&"wu-summaries"), "the union: {required:?}");
    }

    /// The entries LIST_FIELDS gives `field`, none for a field it does not hold.
    fn list_entries(field: &str) -> &'static [&'static str] {
        LIST_FIELDS.iter().find(|(f, ..)| *f == field).map_or(&[], |(_, e, _)| e)
    }

    /// E-DOCSPLIT.W4: each row of the one map from a list field to its entries and its checker is
    /// a role's own field, listed once, with entries for its checker to read.
    #[test]
    fn each_list_field_is_a_roles_field_listed_once_with_entries() {
        for (n, (f, entries, _)) in LIST_FIELDS.iter().enumerate() {
            assert!(!entries.is_empty(), "{f} has a checker and no entries");
            assert!(ROLES.iter().any(|r| role_fields(r).contains(f)), "{f} is no role's field");
            assert!(LIST_FIELDS[..n].iter().all(|(g, ..)| g != f), "{f} is listed twice");
        }
    }

    /// The text of the skill's section that starts with `head` (`## 3 ·`), up to the next `## `.
    fn section<'a>(skill: &'a str, head: &str) -> &'a str {
        let at = skill.find(head).unwrap_or_else(|| panic!("skill keel-plan has no section {head:?}"));
        let rest = &skill[at..];
        rest[1..].find("\n## ").map_or(rest, |n| &rest[..n + 1])
    }

    /// What skill keel-plan teaches `role` with in §3: the paragraph that opens with its bold head
    /// (`**WU plan**` for wu-plan), and each paragraph right after it that opens with one of its
    /// fields `own` that `entries` gives entries for, in backticks (`` `wu-summaries` is a list ``).
    fn taught(skill: &str, role: &str, own: &[&str], entries: fn(&str) -> &'static [&'static str]) -> String {
        let head = role.replace('-', " ");
        let opens = |p: &str| p.strip_prefix("**").and_then(|p| p.split_once("**")).is_some_and(|(h, _)| h.eq_ignore_ascii_case(&head));
        let lists = |p: &&str| own.iter().any(|f| !entries(f).is_empty() && p.starts_with(&format!("`{f}` ")));
        let mut paras = section(skill, "## 3 ·").split("\n\n").map(str::trim).skip_while(|p| !opens(p));
        let Some(first) = paras.next() else { return String::new() };
        std::iter::once(first).chain(paras.take_while(lists)).collect::<Vec<_>>().join("\n\n")
    }

    /// The field-like names in backticks in `text`: lowercase words and `-` (not `UC-*` or `<epic>.W<n>`).
    fn named(text: &str) -> Vec<&str> {
        let field = |t: &&str| t.starts_with(|c: char| c.is_ascii_lowercase()) && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        text.split('`').skip(1).step_by(2).filter(field).collect()
    }

    /// Where skill keel-plan and the check disagree on `role`: a field of its own checklist `own`, or
    /// an entry `entries` gives for one, that its §3 text does not name; any other field it knows
    /// (`known`) that is not a row of §2's table; and back, a name in its §3 text the check does not
    /// read for it.
    fn unnamed(skill: &str, role: &str, own: &[&str], known: &[&str], entries: fn(&str) -> &'static [&'static str]) -> Vec<String> {
        let para = taught(skill, role, own, entries);
        let table = section(skill, "## 2 ·");
        let read: Vec<&str> = own.iter().flat_map(|f| std::iter::once(*f).chain(entries(f).iter().copied())).collect();
        let mut out = Vec::new();
        for f in &read {
            if !para.contains(&format!("`{f}`")) {
                out.push(format!("{role}: `{f}` is not in its §3 paragraph"));
            }
        }
        for f in known.iter().filter(|f| !own.contains(f)) {
            if !table.contains(&format!("| `{f}` |")) {
                out.push(format!("{role}: `{f}` is not in the §2 table"));
            }
        }
        for f in named(&para) {
            let gone = format!("{role}: `{f}` is in its §3 paragraph, which the check does not read");
            if !read.contains(&f) && !known.contains(&f) && !out.contains(&gone) {
                out.push(gone);
            }
        }
        out
    }

    /// E-DOCSPLIT.W4: skill keel-plan names every field the check reads for a role where that role
    /// is taught - its own checklist, with the entries list_entries() gives for a list field, in its
    /// §3 paragraph; the rest of what fields() knows for it in §2's table - and names no field there
    /// the check does not read, so a field added to the check, moved to another role or dropped from
    /// it, without the skill, is red.
    #[test]
    fn keel_plan_names_every_field_the_check_reads() {
        let skill = include_str!("../skills/keel-plan/SKILL.md");
        for role in ROLES {
            let (_, known) = fields(&[role]);
            let missing = unnamed(skill, role, role_fields(role), &known, list_entries);
            assert!(missing.is_empty(), "skill keel-plan does not name {missing:?}");
            assert!(skill.contains(&format!("`{role}`")), "skill keel-plan does not name the role {role}");
        }
        // a field moved between roles is red in the role it moved to, though another role names it
        let (_, known) = fields(&["wu-plan"]);
        let own: Vec<&str> = role_fields("wu-plan").iter().copied().chain(["outcome"]).collect();
        assert_eq!(unnamed(skill, "wu-plan", &own, &known, list_entries), ["wu-plan: `outcome` is not in its §3 paragraph"]);
        let (_, known) = fields(&["wu-result"]);
        let own: Vec<&str> = role_fields("wu-result").iter().copied().chain(["expected"]).collect();
        assert_eq!(unnamed(skill, "wu-result", &own, &known, list_entries), ["wu-result: `expected` is not in its §3 paragraph"]);
        // a field fields() knows outside a role's checklist must be a §2 row
        let known: Vec<&str> = fields(&["epic-result"]).1.into_iter().chain(["x"]).collect();
        assert_eq!(unnamed(skill, "epic-result", role_fields("epic-result"), &known, list_entries), ["epic-result: `x` is not in the §2 table"]);
        // an entry of a list field is named in its role's paragraph
        let cut = skill.replace("`unit`, `integration`", "`integration`");
        assert_ne!(cut, skill);
        let (_, known) = fields(&["wu-plan"]);
        assert_eq!(unnamed(&cut, "wu-plan", role_fields("wu-plan"), &known, list_entries), ["wu-plan: `unit` is not in its §3 paragraph"]);
        // a new list field whose entries the check reads is red until the skill names its entries
        fn more(f: &str) -> &'static [&'static str] {
            if f == "e2e-plan" {
                &["smoke"]
            } else {
                list_entries(f)
            }
        }
        let (_, known) = fields(&["epic-plan"]);
        assert_eq!(unnamed(skill, "epic-plan", role_fields("epic-plan"), &known, more), ["epic-plan: `smoke` is not in its §3 paragraph"]);
        // the last paragraph ends where §3's prose starts: a field named only there is not taught
        let cut = skill.replace("archived) · `follow-ups`.\n\nEvery follow-up", "archived).\n\nEvery `follow-ups` entry");
        assert_ne!(cut, skill);
        let (_, known) = fields(&["epic-result"]);
        assert_eq!(unnamed(&cut, "epic-result", role_fields("epic-result"), &known, list_entries), ["epic-result: `follow-ups` is not in its §3 paragraph"]);
        // nor is one named only in prose that opens with a backticked name that is not a list field
        let cut =
            skill.replace("archived) · `follow-ups`.\n\nEvery follow-up, in either report,", "archived).\n\n`follow-ups`: every follow-up, in either report,");
        assert_ne!(cut, skill);
        assert_eq!(unnamed(&cut, "epic-result", role_fields("epic-result"), &known, list_entries), ["epic-result: `follow-ups` is not in its §3 paragraph"]);
        // and back: a field the check no longer reads is red while the skill still teaches it
        let own: Vec<&str> = role_fields("wu-plan").iter().copied().filter(|f| *f != "requirement").collect();
        let known: Vec<&str> = fields(&["wu-plan"]).1.into_iter().filter(|f| *f != "requirement").collect();
        assert_eq!(unnamed(skill, "wu-plan", &own, &known, list_entries), ["wu-plan: `requirement` is in its §3 paragraph, which the check does not read"]);
    }

    fn problems(yaml: &str) -> Vec<String> {
        match read_data(yaml, "x.html", true) {
            Data::Checked { problems, .. } => problems,
            other => panic!("not checked: {other:?}"),
        }
    }

    const WU_PLAN: &str = "artifact: wu-plan\nstatus: ready\nid: E-X.W1\nepic: E-X\ntitle: t\ndate: \"2026-11-18\"\npage: x.html\nrequirement: r\nmaps-to: m\nexpected: [e]\ntest-plan: {unit: u, integration: i, e2e: none}\n";

    #[test]
    fn a_complete_wu_plan_is_green_and_each_missing_or_empty_field_is_named() {
        assert_eq!(problems(WU_PLAN), Vec::<String>::new());
        for f in ["artifact", "status", "id", "epic", "title", "date", "page", "requirement", "maps-to", "expected", "test-plan"] {
            let gone: String = WU_PLAN.lines().filter(|l| !l.starts_with(&format!("{f}:"))).map(|l| format!("{l}\n")).collect();
            let p = problems(&gone);
            assert_eq!(p, vec![format!("missing field \"{f}\"")], "{f}");
        }
        let p = problems(&WU_PLAN.replace("requirement: r", "requirement: \"  \"").replace("expected: [e]", "expected: []"));
        assert_eq!(p, ["field \"requirement\" is empty", "field \"expected\" is empty"]);
    }

    #[test]
    fn a_misspelt_field_is_named_with_the_known_ones_and_details_is_free() {
        let p = problems(&WU_PLAN.replace("expected:", "expcted:"));
        assert!(p.iter().any(|e| e.starts_with("unknown field \"expcted\" (typo? expected one of: ") && e.contains("owner-changes")), "{p:?}");
        assert!(p.contains(&"missing field \"expected\"".to_string()), "{p:?}");
        assert!(problems(&format!("{WU_PLAN}details:\n  files: [a]\n  anything: {{}}\n")).is_empty());
        assert!(problems(&format!("{WU_PLAN}details:\n")).is_empty(), "details is never checked");
        let p = problems(&format!("{WU_PLAN}intent: i\n"));
        assert!(p.iter().any(|e| e.starts_with("unknown field \"intent\"")), "a field of another role is unknown: {p:?}");
    }

    #[test]
    fn a_draft_is_silent_and_a_yaml_with_no_artifact_and_no_page_is_no_data_file() {
        assert_eq!(read_data("artifact: wu-plan\nstatus: draft\nexpcted: x\n", "x.html", true), Data::Draft);
        assert_eq!(read_data("rows: [a, b]\n", "x.html", false), Data::None);
        assert_eq!(read_data("- a\n- b\n", "x.html", false), Data::None);
        assert_eq!(read_data("", "x.html", false), Data::None);
        assert!(matches!(read_data("rows: [a]\n", "x.html", true), Data::Checked { .. }), "beside a page it is that page's data file");
        assert!(matches!(read_data("a: [\n", "x.html", false), Data::Checked { .. }), "broken YAML is named");
    }

    #[test]
    fn status_date_and_page_are_checked_by_value() {
        let p = problems(&WU_PLAN.replace("status: ready", "status: final").replace("\"2026-11-18\"", "18 Nov").replace("page: x.html", "page: y.html"));
        assert_eq!(
            p,
            [
                "status \"final\" is neither draft nor ready",
                "date \"18 Nov\" is not a quoted \"YYYY-MM-DD\"",
                "page \"y.html\" is not this data file's page: write \"x.html\""
            ]
        );
        assert!(problems(&WU_PLAN.replace("\"2026-11-18\"", "2026-11-18")).is_empty(), "a bare date reads as text here");
        let gone = match read_data(WU_PLAN, "x.html", false) {
            Data::Checked { problems, .. } => problems,
            other => panic!("{other:?}"),
        };
        assert_eq!(gone, ["page x.html is not beside the data file"]);
    }

    #[test]
    fn a_wu_id_must_belong_to_its_epic_and_test_plan_names_each_level() {
        let p = problems(&WU_PLAN.replace("id: E-X.W1", "id: E-Y.W1"));
        assert_eq!(p, ["id \"E-Y.W1\" is not a WU of epic \"E-X\": write E-X.W<n>"]);
        let p = problems(&WU_PLAN.replace("test-plan: {unit: u, integration: i, e2e: none}", "test-plan: {unit: u, e2e: \"\", checks: [a]}"));
        assert_eq!(p, ["test-plan: missing level \"integration\" (its tests, or \"none: <why>\")", "test-plan: level \"e2e\" is empty"]);
        let p = problems(&WU_PLAN.replace("test-plan: {unit: u, integration: i, e2e: none}", "test-plan: all of them"));
        assert_eq!(p, ["field \"test-plan\" is not a mapping: give each level, unit, integration, e2e"]);
    }

    const EPIC_PLAN: &str = "artifact: epic-plan\nstatus: ready\nid: E-X\ntitle: t\ndate: \"2026-11-18\"\npage: x.html\nintent: i\nowner-changes: [{when: a, today: b, after: c}]\nuse-cases: none\nexpected: [e]\nnon-goals: [n]\nwu-summaries:\n  - {wu: E-X.W1, wu-summary: s, wu-maps-to: m, wu-expected: e, wu-deps: none}\n  - {wu: E-X.W2, wu-summary: s, wu-maps-to: m, wu-expected: e, wu-deps: [E-X.W1]}\ne2e-plan: p\ndistillation-plan: d\nopen-questions: none\n";

    #[test]
    fn an_epic_plan_needs_its_wus_listed_once_each_with_five_fields_and_deps_that_resolve() {
        assert_eq!(problems(EPIC_PLAN), Vec::<String>::new());
        let p = problems(&EPIC_PLAN.replace("owner-changes: [{when: a, today: b, after: c}]\n", ""));
        assert_eq!(p, ["missing field \"owner-changes\""]);
        let p = problems(&EPIC_PLAN.replace(", wu-deps: [E-X.W1]}", "}"));
        assert_eq!(p, ["wu-summaries entry 2 (E-X.W2): missing field \"wu-deps\""]);
        let p = problems(&EPIC_PLAN.replace("wu-deps: [E-X.W1]", "wu-deps: [E-X.W9, E-X.W2]"));
        assert_eq!(
            p,
            [
                "wu-deps of E-X.W2 names \"E-X.W9\", which is not a WU of this plan (write \"none\" when it waits for none)",
                "wu-deps of E-X.W2 names E-X.W2 itself"
            ]
        );
        let p = problems(&EPIC_PLAN.replace("{wu: E-X.W2,", "{wu: E-X.W1,").replace("wu-deps: [E-X.W1]", "wu-deps: none"));
        assert_eq!(p, ["wu-summaries entry 2 (E-X.W1): wu \"E-X.W1\" is listed twice"]);
        let p = problems(
            &EPIC_PLAN
                .replace("{wu: E-X.W2,", "{wu: E-Y.W2,")
                .replace("wu-deps: [E-X.W1]", "wu-deps: none")
                .replace("wu-expected: e, wu-deps: none}\n  -", "wu-expectd: e, wu-deps: none}\n  -"),
        );
        assert_eq!(
            p,
            [
                "wu-summaries entry 1 (E-X.W1): unknown field \"wu-expectd\" (typo? expected one of: wu, wu-summary, wu-maps-to, wu-expected, wu-deps)",
                "wu-summaries entry 1 (E-X.W1): missing field \"wu-expected\"",
                "wu-summaries entry 2 (E-Y.W2): wu \"E-Y.W2\" is not a WU of this epic: write E-X.W<n>"
            ]
        );
        assert_eq!(problems(&EPIC_PLAN.replace("wu-summaries:\n", "wu-summaries: x\nold:\n")).first().map(String::as_str), Some("unknown field \"old\" (typo? expected one of: artifact, status, id, title, date, page, intent, owner-changes, use-cases, expected, non-goals, wu-summaries, e2e-plan, distillation-plan, open-questions, details)"));
    }

    #[test]
    fn a_one_part_epic_needs_both_roles_fields_and_only_matching_roles_share_a_doc() {
        let p = problems(&EPIC_PLAN.replace("artifact: epic-plan", "artifact: [epic-plan, wu-plan]"));
        assert_eq!(p, ["missing field \"requirement\"", "missing field \"maps-to\"", "missing field \"test-plan\""]);
        let both = format!(
            "{}requirement: r\nmaps-to: m\ntest-plan: {{unit: u, integration: i, e2e: e}}\n",
            EPIC_PLAN.replace("artifact: epic-plan", "artifact: epic-plan wu-plan")
        );
        assert_eq!(problems(&both), Vec::<String>::new());
        // a list field's entry problems come in one order, wu-summaries then test-plan, whichever role it lists first
        let bad = both.replace(", wu-deps: [E-X.W1]}", "}").replace(", e2e: e}", "}");
        for listed in ["artifact: epic-plan wu-plan", "artifact: [wu-plan, epic-plan]"] {
            let p = problems(&bad.replace("artifact: epic-plan wu-plan", listed));
            assert_eq!(
                p,
                ["wu-summaries entry 2 (E-X.W2): missing field \"wu-deps\"", "test-plan: missing level \"e2e\" (its tests, or \"none: <why>\")"],
                "{listed}"
            );
        }
        let p = problems(&EPIC_PLAN.replace("artifact: epic-plan", "artifact: [epic-plan, wu-result]"));
        assert_eq!(p, ["artifact epic-plan and wu-result cannot share one doc: a one-part epic pairs epic-plan with wu-plan, or epic-result with wu-result"]);
        let p = problems(&EPIC_PLAN.replace("artifact: epic-plan", "artifact: epic-plna"));
        assert_eq!(p, ["unknown artifact \"epic-plna\" (expected one of: epic-plan, wu-plan, wu-result, epic-result)"]);
    }

    #[test]
    fn pair_of_pairs_a_page_with_the_data_file_beside_it_under_docs_root_only() {
        let d = tempfile::TempDir::new().unwrap();
        let (docs, out) = (d.path().join("docs"), d.path().join("out"));
        for dir in [&docs, &out] {
            std::fs::create_dir(dir).unwrap();
            std::fs::write(dir.join("a.yaml"), "").unwrap();
        }
        let in_docs = |p: &Path| p.starts_with(&docs);
        let pair = |page: &Path, data: Option<&Path>| Some(Pair { page: page.to_owned(), data: data.map(Path::to_owned) });
        let (page, data) = (docs.join("a.html"), docs.join("a.yaml"));
        assert_eq!(pair_of(&page, in_docs), pair(&page, Some(&data)), "a page finds its data file, there or not");
        assert_eq!(pair_of(&data, in_docs), pair(&page, Some(&data)), "a data file finds its page: one pair");
        assert_eq!(pair_of(&docs.join("gone.yaml"), in_docs), pair(&docs.join("gone.html"), Some(&docs.join("gone.yaml"))), "named, to be read");
        assert_eq!(pair_of(&docs.join("b.html"), in_docs), pair(&docs.join("b.html"), None), "no data file beside it");
        assert_eq!(pair_of(&out.join("a.yaml"), in_docs), None, "a .yaml outside docs.root is no doc");
        assert_eq!(pair_of(&out.join("a.html"), in_docs), pair(&out.join("a.html"), None), "a page outside docs.root has no data file");
        assert_eq!(pair_of(&docs.join("a.md"), in_docs), pair(&docs.join("a.md"), None));
    }

    #[test]
    fn a_text_field_of_another_type_is_named_wrong_type_never_green() {
        let p = problems(&WU_PLAN.replace("status: ready", "status: true"));
        assert_eq!(p, ["field \"status\" is the wrong type (true): write it as text, such as \"ready\""]);
        let p = problems(&WU_PLAN.replace("id: E-X.W1", "id: 7"));
        assert_eq!(p, ["field \"id\" is the wrong type (7): write it as text, such as \"E-<CODE>.W<n>\""]);
        let p = problems(&WU_PLAN.replace("epic: E-X", "epic: [E-X]"));
        assert_eq!(p, ["field \"epic\" is the wrong type (a list): write it as text, such as \"E-<CODE>\""]);
        let p = problems(&WU_PLAN.replace("title: t", "title: {a: b}"));
        assert_eq!(p, ["field \"title\" is the wrong type (a mapping): write it as text"]);
        let p = problems(&EPIC_PLAN.replace("id: E-X\n", "id: [E-X]\n"));
        assert_eq!(p, ["field \"id\" is the wrong type (a list): write it as text, such as \"E-<CODE>\""]);
        let p = problems(&EPIC_PLAN.replace("{wu: E-X.W1,", "{wu: 1.5,").replace("wu-deps: [E-X.W1]", "wu-deps: none"));
        assert_eq!(p, ["wu-summaries entry 1 (no wu): field \"wu\" is the wrong type (1.5): write it as text, such as \"E-<CODE>.W<n>\""]);
    }

    #[test]
    fn a_data_file_with_anchors_or_aliases_is_refused_before_it_is_expanded() {
        let want = ["the data file uses a YAML anchor or alias (&name, *name): write each value out"];
        assert_eq!(problems(&format!("{WU_PLAN}details: {{a: &x [1], b: *x}}\n")), want);
        assert_eq!(problems(&WU_PLAN.replace("requirement: r", "requirement: &r r")), want, "an anchor alone");
        // Billion laughs: ten levels of ten aliases each, 10^10 nodes once expanded; refused at once.
        let mut lol = String::from("artifact: wu-plan\na0: &a0 [lol]\n");
        for i in 1..=10 {
            let refs = vec![format!("*a{}", i - 1); 10].join(", ");
            lol.push_str(&format!("a{i}: &a{i} [{refs}]\n"));
        }
        let t = std::time::Instant::now();
        assert_eq!(problems(&lol), want);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "took {:?}", t.elapsed());
    }

    /// Its problems when the page is not there.
    fn pageless(yaml: &str) -> Vec<String> {
        match read_data(yaml, "x.html", false) {
            Data::Checked { problems, .. } => problems,
            other => panic!("not checked: {other:?}"),
        }
    }

    /// E-DOCSPLIT.W2 review round 2: a leading UTF-8 byte-order mark (legal at a YAML stream's start)
    /// is not glued to the first key, so a valid data file is green beside its page and red without it.
    #[test]
    fn a_leading_byte_order_mark_is_not_part_of_the_first_key() {
        let bom = format!("\u{feff}{WU_PLAN}");
        assert_eq!(problems(&bom), Vec::<String>::new());
        assert_eq!(pageless(&bom), ["page x.html is not beside the data file"]);
        assert_eq!(read_data("\u{feff}rows: [a]\n", "x.html", false), Data::None, "other data stays other data");
    }

    /// E-DOCSPLIT.W2 review round 2: a data file whose YAML documents hold an `artifact` is a data
    /// file, red with or without its page; pageless data of several documents with none is other data.
    #[test]
    fn a_second_yaml_document_after_an_artifact_is_red_with_or_without_its_page() {
        let two = format!("{WU_PLAN}---\nx: 1\n");
        let want = "the data file holds 2 YAML documents; it is one";
        assert_eq!(problems(&two), [want]);
        assert_eq!(pageless(&two), [want, "page x.html is not beside the data file"]);
        assert_eq!(pageless(&format!("{WU_PLAN}---\n")), [want, "page x.html is not beside the data file"]);
        assert_eq!(pageless(&format!("x: 1\n---\n{WU_PLAN}")), [want, "page x.html is not beside the data file"]);
        assert_eq!(read_data("rows: [a]\n---\nx: 1\n", "x.html", false), Data::None);
        assert_eq!(read_data("- a\n---\n- b\n", "x.html", false), Data::None);
    }

    /// E-DOCSPLIT.W2 review round 3: a data file of several documents checks the one holding
    /// `artifact` as a single document would be checked, so both name a missing or wrong page alike.
    #[test]
    fn several_documents_name_a_missing_or_wrong_page_as_one_document_does() {
        let many = "the data file holds 2 YAML documents; it is one".to_string();
        for one in [WU_PLAN.to_string(), WU_PLAN.replace("page: x.html", "page: y.html"), WU_PLAN.replace("id: E-X.W1", "id: E-Y.W1")] {
            for (two, page_exists) in [(format!("{one}---\nx: 1\n"), false), (format!("x: 1\n---\n{one}"), false), (format!("{one}---\nx: 1\n"), true)] {
                let single = match read_data(&one, "x.html", page_exists) {
                    Data::Checked { problems, .. } => problems,
                    other => panic!("{other:?}"),
                };
                let want: Vec<String> = std::iter::once(many.clone()).chain(single).collect();
                match read_data(&two, "x.html", page_exists) {
                    Data::Checked { problems, .. } => assert_eq!(problems, want, "{two}"),
                    other => panic!("{other:?}"),
                }
            }
        }
        let gone = pageless(&format!("{}---\nx: 1\n", WU_PLAN.replace("page: x.html", "page: y.html")));
        assert_eq!(gone, [many.as_str(), "page \"y.html\" is not this data file's page: write \"x.html\""]);
        assert_eq!(pageless(&format!("{}---\nx: 1\n", WU_PLAN.replace("status: ready", "status: draft"))), [many.as_str()], "a draft does not silence it");
    }

    #[test]
    fn broken_yaml_and_odd_shapes_are_named_never_a_panic() {
        assert!(problems("a: [\n")[0].starts_with("not valid YAML: "));
        assert!(problems("a: 1\na: 2\n")[0].starts_with("not valid YAML: "), "a duplicated key");
        assert_eq!(problems("--- \na: 1\n---\nb: 2\n"), ["the data file holds 2 YAML documents; it is one"]);
        assert_eq!(problems("- a\n"), ["the data file is not a mapping of fields"]);
        assert_eq!(problems(""), ["the data file is empty: it needs its role's fields"]);
        let p = problems(&EPIC_PLAN.replace("wu-summaries:\n  - {wu: E-X.W1", "wu-summaries:\n  - 5\n  - {wu: E-X.W1"));
        assert_eq!(p, ["wu-summaries entry 1 is not a mapping of wu, wu-summary, wu-maps-to, wu-expected, wu-deps"]);
        let p = problems(&WU_PLAN.replace("artifact: wu-plan", "artifact: 7"));
        assert_eq!(p, ["unknown artifact \"Integer(7)\" (expected one of: epic-plan, wu-plan, wu-result, epic-result)"]);
    }
}
