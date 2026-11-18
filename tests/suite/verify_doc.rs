compiled!();

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    common::path_with(bin)
}

fn run(prog: &str, d: &Path, a: &[&str], stdin: &str) -> Output {
    let mut c = common::cmd(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", path_with_keel())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

fn keel(d: &Path, a: &[&str]) -> Output {
    run(env!("CARGO_BIN_EXE_keel"), d, a, "")
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// A page with no data file: today's shape, its tags no longer read. `status` is its own
/// `data-status`, which silences it when "draft".
fn doc(status: &str, item: &str, body: &str) -> String {
    format!(
        r#"<html data-artifact="wu-plan" data-status="{status}"><body>
<p data-item="requirement">r</p><p data-item="maps-to">m</p><p data-item="{item}">e</p><p data-item="test-plan">t</p>{body}
</body></html>"#
    )
}

fn repo() -> TempDir {
    let d = TempDir::new().unwrap();
    let p = d.path();
    run("git", p, &["init", "-q", "-b", "main"], "");
    std::fs::create_dir_all(p.join(".keel")).unwrap();
    std::fs::write(p.join(".keel/config.toml"), "[check]\nlint=\"true\"\ntest=\"true\"\n[branches]\nmain=\"main\"\n[docs]\nroot=\"docs/\"\n").unwrap();
    std::fs::create_dir(p.join("docs")).unwrap();
    d
}

/// A ready WU plan's data file serving `page`, with `extra` lines appended.
fn wu_plan_data(page: &str, extra: &str) -> String {
    format!(
        "artifact: wu-plan\nstatus: ready\nid: E-X.W1\nepic: E-X\ntitle: t\ndate: \"2026-11-18\"\npage: {page}\nrequirement: r\nmaps-to: m\nexpected: [e]\ntest-plan:\n  unit: u\n  integration: i\n  e2e: none\n{extra}"
    )
}

/// A ready epic plan's data file serving `page`, its two WUs as given (`wus`, YAML list entries).
fn epic_plan_data(page: &str, roles: &str, wus: &str) -> String {
    format!(
        "artifact: {roles}\nstatus: ready\nid: E-X\ntitle: t\ndate: \"2026-11-18\"\npage: {page}\nintent: i\nowner-changes:\n  - {{when: w, today: t, after: a}}\nuse-cases: none\nexpected: [e]\nnon-goals: [n]\nwu-summaries:\n{wus}e2e-plan: p\ndistillation-plan: d\nopen-questions: none\n"
    )
}

const TWO_WUS: &str = "  - {wu: E-X.W1, wu-summary: s, wu-maps-to: m, wu-expected: e, wu-deps: none}\n  - {wu: E-X.W2, wu-summary: s, wu-maps-to: m, wu-expected: e, wu-deps: [E-X.W1]}\n";

/// A page of `n` words in its main, `top` of them above the heading with id "work" (all when `top` is None).
fn page_of(n: usize, top: Option<usize>) -> String {
    let body = match top {
        Some(t) => format!("<p>{}</p><h2 id=\"work\">{}</h2>", words(t), words(n - t)),
        None => format!("<p>{}</p>", words(n)),
    };
    format!(r#"<html lang="en"><head><title>a long title here</title></head><body><header><h1>{}</h1></header><main>{body}</main></body></html>"#, words(40))
}

/// UC-DOC-1 (E-DOCSPLIT.W2): a ready data file's missing or empty field is red and named, at the
/// page's path or the data file's; complete, the pair is green.
#[test]
fn a_ready_data_file_missing_or_emptying_a_field_is_red_and_names_it() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/a.html"), page_of(10, None)).unwrap();
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "")).unwrap();
    for f in ["docs/a.html", "docs/a.yaml"] {
        let o = keel(p, &["verify-doc", f]);
        assert!(o.status.success(), "{f}: {}", text(&o));
    }
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "").replace("requirement: r\n", "")).unwrap();
    for f in ["docs/a.html", "docs/a.yaml"] {
        let o = keel(p, &["verify-doc", f]);
        assert_eq!(o.status.code(), Some(1), "{f}");
        assert!(text(&o).contains("docs/a.yaml: missing field \"requirement\""), "{f}: {}", text(&o));
    }
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "").replace("maps-to: m", "maps-to: \"\"")).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/a.yaml"]));
    assert!(t.contains("docs/a.yaml: field \"maps-to\" is empty"), "{t}");
    let t = text(&keel(p, &["verify-doc", "docs/a.html", "docs/a.yaml"]));
    assert_eq!(t.matches("field \"maps-to\" is empty").count(), 1, "a pair is checked once: {t}");
}

/// UC-DOC-1: a misspelt field is named next to the missing one; a draft data file is silent, page included.
#[test]
fn a_misspelt_field_is_named_and_a_draft_is_silent() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/a.html"), page_of(900, None) + r#"<a href="gone.html">x</a>"#).unwrap();
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "").replace("expected:", "expcted:")).unwrap();
    let o = keel(p, &["verify-doc", "docs/a.yaml"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1));
    assert!(t.contains("unknown field \"expcted\"") && t.contains("missing field \"expected\""), "{t}");
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "").replace("expected:", "expcted:").replace("status: ready", "status: draft")).unwrap();
    let o = keel(p, &["verify-doc", "docs/a.yaml"]);
    assert!(o.status.success(), "a draft silences the pair: {}", text(&o));
    assert!(keel(p, &["verify-doc"]).status.success());
}

/// UC-DOC-1: the page's old checklist tags are not read - a page whose tags are wrong but whose
/// data file is complete is green, and a page with tags but no data file is checked as a plain page.
#[test]
fn old_page_tags_are_not_read() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/a.html"), doc("ready", "expcted", "")).unwrap();
    let o = keel(p, &["verify-doc", "docs/a.html"]);
    assert!(o.status.success(), "no data file: links and labels only: {}", text(&o));
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "")).unwrap();
    let o = keel(p, &["verify-doc", "docs/a.html"]);
    assert!(o.status.success(), "a complete data file and a mistagged page: {}", text(&o));
    std::fs::write(p.join("docs/e.html"), r#"<html data-artifact="epic-plan" data-status="ready"><body><p data-item="intent">x</p></body></html>"#).unwrap();
    let o = keel(p, &["verify-doc", "docs/e.html"]);
    assert!(o.status.success(), "a tagged epic plan page with no data file: {}", text(&o));
}

/// UC-DOC-1: an epic plan's data file lists its WUs once each, every entry with its five fields,
/// every dependency a WU of the plan.
#[test]
fn an_epic_plan_data_file_needs_each_wu_and_its_dependencies_to_resolve() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/e.html"), page_of(100, Some(50))).unwrap();
    std::fs::write(p.join("docs/e.yaml"), epic_plan_data("e.html", "epic-plan", TWO_WUS)).unwrap();
    let o = keel(p, &["verify-doc", "docs/e.yaml"]);
    assert!(o.status.success(), "{}", text(&o));
    std::fs::write(p.join("docs/e.yaml"), epic_plan_data("e.html", "epic-plan", &TWO_WUS.replace(", wu-deps: none}", "}").replace("[E-X.W1]", "[E-X.W3]")))
        .unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/e.yaml"]));
    assert!(t.contains("wu-summaries entry 1 (E-X.W1): missing field \"wu-deps\""), "{t}");
    assert!(t.contains("wu-deps of E-X.W2 names \"E-X.W3\", which is not a WU of this plan"), "{t}");
    std::fs::write(p.join("docs/e.yaml"), epic_plan_data("e.html", "epic-plan", "").replace("wu-summaries:\n", "wu-summaries: []\n")).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/e.yaml"]));
    assert!(t.contains("field \"wu-summaries\" is empty"), "{t}");
}

/// UC-DOC-1: a one-part epic's data file needs both roles' fields, a shared one once.
#[test]
fn a_one_part_epic_needs_the_union() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/e.html"), page_of(100, Some(50))).unwrap();
    std::fs::write(p.join("docs/e.yaml"), epic_plan_data("e.html", "[epic-plan, wu-plan]", TWO_WUS)).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/e.yaml"]));
    assert!(t.contains("missing field \"requirement\"") && t.contains("missing field \"test-plan\"") && !t.contains("missing field \"expected\""), "{t}");
    let full = epic_plan_data("e.html", "[epic-plan, wu-plan]", TWO_WUS) + "requirement: r\nmaps-to: m\ntest-plan: {unit: u, integration: i, e2e: e}\n";
    std::fs::write(p.join("docs/e.yaml"), full).unwrap();
    let o = keel(p, &["verify-doc", "docs/e.yaml"]);
    assert!(o.status.success(), "{}", text(&o));
}

/// UC-DOC-6 (E-DOCSPLIT.W2): a ready page is capped by its data file's role - a WU plan page of 501
/// words is red with the count, of 500 green; the title block outside main is not counted; a
/// draft is silent.
#[test]
fn a_page_over_its_role_cap_is_red_with_the_count() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "")).unwrap();
    std::fs::write(p.join("docs/a.html"), page_of(500, None)).unwrap();
    let o = keel(p, &["verify-doc", "docs/a.html"]);
    assert!(o.status.success(), "500 words: {}", text(&o));
    std::fs::write(p.join("docs/a.html"), page_of(501, None)).unwrap();
    let o = keel(p, &["verify-doc", "docs/a.html"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("docs/a.html: page has 501 words, over the wu-plan cap of 500"), "{}", text(&o));
    for (role, cap) in [("wu-result", 500), ("epic-result", 700)] {
        let data = format!("artifact: {role}\nstatus: ready\nid: E-X.W1\nepic: E-X\ntitle: t\ndate: \"2026-11-18\"\npage: r.html\noutcome: delivered\nwu-rollup: [x]\ndeviations: none\nevidence: e\ndistillation: nothing citable\nfollow-ups: none\n");
        let data = if role == "wu-result" { data.replace("wu-rollup: [x]\n", "") } else { data.replace("epic: E-X\n", "").replace("id: E-X.W1", "id: E-X") };
        std::fs::write(p.join("docs/r.yaml"), &data).unwrap();
        std::fs::write(p.join("docs/r.html"), page_of(cap, None)).unwrap();
        let o = keel(p, &["verify-doc", "docs/r.yaml"]);
        assert!(o.status.success(), "{role} at {cap}: {}", text(&o));
        std::fs::write(p.join("docs/r.html"), page_of(cap + 1, None)).unwrap();
        let t = text(&keel(p, &["verify-doc", "docs/r.yaml"]));
        assert!(t.contains(&format!("page has {} words, over the {role} cap of {cap}", cap + 1)), "{t}");
    }
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "").replace("status: ready", "status: draft")).unwrap();
    std::fs::write(p.join("docs/a.html"), page_of(900, None)).unwrap();
    assert!(keel(p, &["verify-doc", "docs/a.html"]).status.success(), "a draft is silent");
}

/// UC-DOC-6: an epic plan page holds 700 words, its top - main above the heading with id "work" -
/// 300; with no such heading the whole page is the top.
#[test]
fn an_epic_plan_page_over_700_or_its_top_over_300_is_red_with_the_count() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/e.yaml"), epic_plan_data("e.html", "epic-plan", TWO_WUS)).unwrap();
    for (n, top, ok, named) in [
        (700, Some(300), true, ""),
        (701, Some(300), false, "page has 701 words, over the epic-plan cap of 700"),
        (400, Some(301), false, "epic plan top has 301 words above the heading with id=\"work\", over the cap of 300"),
        (301, None, false, "epic plan top has 301 words, over the cap of 300: no heading has id=\"work\""),
        (300, None, true, ""),
    ] {
        std::fs::write(p.join("docs/e.html"), page_of(n, top)).unwrap();
        let o = keel(p, &["verify-doc", "docs/e.yaml"]);
        assert_eq!(o.status.success(), ok, "{n} {top:?}: {}", text(&o));
        assert!(text(&o).contains(named), "{n} {top:?}: {}", text(&o));
    }
    std::fs::write(
        p.join("docs/e.yaml"),
        epic_plan_data("e.html", "[epic-plan, wu-plan]", TWO_WUS) + "requirement: r\nmaps-to: m\ntest-plan: {unit: u, integration: i, e2e: e}\n",
    )
    .unwrap();
    std::fs::write(p.join("docs/e.html"), page_of(700, Some(300))).unwrap();
    assert!(keel(p, &["verify-doc", "docs/e.yaml"]).status.success(), "a one-part epic takes the epic caps");
}

/// UC-DOC-6: any other page is capped by its path, from [docs.caps]; with no table, by nothing.
#[test]
fn a_page_with_no_data_file_is_capped_by_its_path_from_the_config() {
    let d = repo();
    let p = d.path();
    std::fs::create_dir_all(p.join("docs/decisions")).unwrap();
    std::fs::write(p.join("docs/decisions/d.html"), page_of(60, None)).unwrap();
    std::fs::write(p.join("docs/install.html"), page_of(60, None)).unwrap();
    assert!(keel(p, &["verify-doc"]).status.success(), "no [docs.caps]: no path cap");
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    std::fs::write(p.join(".keel/config.toml"), format!("{cfg}[docs.caps]\n\"docs/decisions/*.html\" = 50\n\"docs/install.html\" = 60\n")).unwrap();
    let o = keel(p, &["verify-doc"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(t.contains("d.html: page has 60 words, over the cap of 50 that [docs.caps] sets for docs/decisions/*.html"), "{t}");
    assert!(!t.contains("install.html"), "60 of 60 is green: {t}");
    let abs = p.join("docs/decisions/d.html");
    assert_eq!(keel(&p.join("docs"), &["verify-doc", abs.to_str().unwrap()]).status.code(), Some(1), "an absolute path from a subdirectory finds its glob");
    std::fs::write(p.join("docs/decisions/d.html"), page_of(60, None).replace("<html lang=\"en\">", "<html lang=\"en\" data-status=\"draft\">")).unwrap();
    assert!(keel(p, &["verify-doc"]).status.success(), "a draft page is silent");
}

/// E-DOCSPLIT.W2: a .yaml outside docs.root is no data file and is skipped; one under it with no
/// artifact and no page is other data (W5's), skipped too; broken YAML beside a page is named; a
/// data file must name its own page, and that page must be there.
#[test]
fn yaml_outside_docs_and_other_data_are_skipped_and_a_data_file_names_its_page() {
    let d = repo();
    let p = d.path();
    std::fs::create_dir_all(p.join(".github/workflows")).unwrap();
    std::fs::write(p.join(".github/workflows/x.yaml"), "on: [push]\nnot: [valid\n").unwrap();
    let o = keel(p, &["verify-doc", ".github/workflows/x.yaml"]);
    assert!(o.status.success() && text(&o).is_empty(), "{}", text(&o));
    std::fs::write(p.join("docs/index.yaml"), "rows:\n  - DEC-1\n").unwrap();
    assert!(keel(p, &["verify-doc"]).status.success());
    std::fs::write(p.join("docs/a.html"), page_of(10, None)).unwrap();
    std::fs::write(p.join("docs/a.yaml"), "artifact: [wu-plan\n").unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/a.html"]));
    assert!(t.contains("docs/a.yaml: not valid YAML"), "{t}");
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("b.html", "")).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/a.yaml"]));
    assert!(t.contains("page \"b.html\" is not this data file's page: write \"a.html\""), "{t}");
    std::fs::remove_file(p.join("docs/a.html")).unwrap();
    std::fs::write(p.join("docs/a.yaml"), wu_plan_data("a.html", "")).unwrap();
    let t = text(&keel(p, &["verify-doc"]));
    assert!(t.contains("docs/a.yaml: page a.html is not beside the data file"), "{t}");
    let o = keel(p, &["verify-doc", "docs/gone.yaml"]);
    assert!(o.status.code() == Some(1) && text(&o).contains("docs/gone.yaml: cannot read file"), "{}", text(&o));
}

/// E-DOCSPLIT.W1's two example data files, beside their pages, are green.
#[test]
fn w1s_example_data_files_are_green() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/docsplit");
    let d = repo();
    let p = d.path();
    std::fs::create_dir_all(p.join("docs/work")).unwrap();
    for e in std::fs::read_dir(&root).unwrap().map(Result::unwrap).filter(|e| e.file_type().unwrap().is_file()) {
        std::fs::copy(e.path(), p.join("docs/work").join(e.file_name())).unwrap();
    }
    std::fs::write(p.join("docs/work/2026-11-18-e-docsplit-w1-inventory.html"), "<html><body><h2 id=\"example-data\">x</h2></body></html>").unwrap();
    for f in ["2026-11-17-e-docsplit-plan.yaml", "2026-11-18-e-docsplit-w1-plan.yaml"] {
        let o = keel(p, &["verify-doc", &format!("docs/work/{f}")]);
        assert!(o.status.success(), "{f}: {}", text(&o));
        let data = std::fs::read_to_string(p.join("docs/work").join(f)).unwrap();
        std::fs::write(p.join("docs/work").join(f), data.replace("\nexpected:", "\nexpcted:")).unwrap();
        let t = text(&keel(p, &["verify-doc", &format!("docs/work/{f}")]));
        assert!(t.contains("missing field \"expected\"") && t.contains("unknown field \"expcted\""), "{f}: {t}");
        std::fs::write(p.join("docs/work").join(f), data).unwrap();
    }
    assert!(keel(p, &["verify-doc"]).status.success());
}

#[test]
fn dangling_relative_link_is_red_existing_is_not() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/b.html"), doc("ready", "expected", "")).unwrap();
    std::fs::write(p.join("docs/a.html"), doc("ready", "expected", r#"<a href="b.html#x">ok</a><a href="https://x.io/n">ext</a>"#)).unwrap();
    assert!(keel(p, &["verify-doc", "docs/a.html"]).status.success());
    std::fs::write(p.join("docs/a.html"), doc("ready", "expected", r#"<a href="gone.html">bad</a>"#)).unwrap();
    assert!(text(&keel(p, &["verify-doc", "docs/a.html"])).contains("dangling relative link: gone.html"));
    std::fs::write(p.join("docs/a.html"), doc("draft", "expected", r#"<a href="gone.html">bad</a>"#)).unwrap();
    assert!(keel(p, &["verify-doc", "docs/a.html"]).status.success());
}

#[test]
fn stop_refuses_a_red_doc_and_post_hook_reads_claude_and_codex_payloads() {
    let d = repo();
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    let hooks = run("git", p, &["config", "core.hooksPath"], "").stdout;
    let hooks = Path::new(std::str::from_utf8(&hooks).unwrap().trim()).to_owned();
    let stop = hooks.join("verify-doc-stop");
    let post = hooks.join("verify-doc-post");
    let bad = doc("ready", "expected", r#"<a href="gone.html">bad</a>"#);
    std::fs::write(p.join("docs/a.html"), &bad).unwrap();

    let o = run(stop.to_str().unwrap(), p, &[], "");
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("gone.html"));

    let claude = r#"{"tool_input":{"file_path":"docs/a.html"}}"#;
    assert_eq!(run(post.to_str().unwrap(), p, &[], claude).status.code(), Some(2));
    let codex = r#"{"tool_input":{"command":"*** Begin Patch\n*** Update File: docs/a.html\n@@\n*** End Patch"}}"#;
    assert_eq!(run(post.to_str().unwrap(), p, &[], codex).status.code(), Some(2));

    std::fs::write(p.join("docs/a.html"), doc("ready", "expected", "")).unwrap();
    assert!(run(stop.to_str().unwrap(), p, &[], "").status.success());
    assert!(run(post.to_str().unwrap(), p, &[], claude).status.success());
}

fn words(n: usize) -> String {
    "word ".repeat(n)
}

fn flowchart(body: &str) -> String {
    format!("<html lang=\"en\"><body><main><pre class=\"mermaid\">\nflowchart LR\n{body}\n</pre></main></body></html>")
}

fn verify(p: &Path, file: &str, body: &str) -> (bool, String) {
    std::fs::write(p.join("docs").join(file), body).unwrap();
    let o = keel(p, &["verify-doc", &format!("docs/{file}")]);
    (o.status.success(), text(&o))
}

/// E-DOCSTYLE.W2 (UC-DOC-4): every finished doc's flowchart labels are quoted; the error names the
/// label and shows it quoted. Other diagram kinds and drafts are not scanned.
#[test]
fn an_unquoted_flowchart_label_is_red_and_named_in_any_finished_doc() {
    let d = repo();
    let p = d.path();
    let (ok, t) = verify(p, "f.html", &flowchart(r#"A["a"] --&gt;|yes| B["b"]"#));
    assert!(!ok && t.contains(r#"diagram label not in quotes: |yes| - write |"yes"|"#), "{t}");
    let quoted = r#"A["Build · step"] -->|"yes"| B(["b"]) -- "no" --> Q{"Ready?"}
  Q -.->|"later"| C[("db")] == "x" ==> D
  %% A[comment]
  classDef hot fill:#f96,stroke:#333
  style A fill:#f9f"#;
    let (ok, t) = verify(p, "f.html", &flowchart(quoted));
    assert!(ok, "{t}");
    let seq = flowchart("x").replace("flowchart LR\nx", "sequenceDiagram\n  A->>B: hi [there]");
    let (ok, t) = verify(p, "f.html", &seq);
    assert!(ok, "{t}");
    let draft = doc("draft", "expected", &flowchart("A[raw]"));
    let (ok, t) = verify(p, "f.html", &draft);
    assert!(ok, "{t}");
    let (ok, t) = verify(p, "f.html", &doc("ready", "expected", "<pre class=\"mermaid\">graph TD\n  A[raw]</pre>"));
    assert!(!ok && t.contains("[raw]"), "{t}");
}

fn copy_docs(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue; // docs/.archive: untracked, never checked
        }
        if e.file_type().unwrap().is_dir() {
            copy_docs(&e.path(), &to.join(&name));
        } else if [".html", ".yaml"].iter().any(|x| name.to_string_lossy().ends_with(x)) {
            std::fs::copy(e.path(), to.join(&name)).unwrap();
        }
    }
}

/// E-DOCSTYLE.W2, E-DOCSPLIT.W2: the checks turn no existing doc red - every page and data file in
/// Keel's docs/ and the writing skill's example pass.
#[test]
fn every_doc_in_docs_and_the_writing_example_pass_the_checks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let d = repo();
    let p = d.path();
    copy_docs(&root.join("docs"), &p.join("docs"));
    std::fs::create_dir_all(p.join("docs/work")).unwrap();
    std::fs::copy(root.join("skills/keel-write/references/example.html"), p.join("docs/work/keel-write-example.html")).unwrap();
    let o = keel(p, &["verify-doc"]);
    assert!(o.status.success(), "{}", text(&o));
}

/// E-DOCSTYLE epic test, rewritten for E-DOCSPLIT.W2: `keel init` in a scratch repo installs the
/// writing skill; its example, written as a plan page (its parts' heading marked `work`, as the
/// split's pages are) with a data file beside it, passes the installed after-write hook; an
/// unquoted label, a data file missing "what changes for you" and a top over the cap each make the
/// after-write and end-of-turn hooks fail and name the fault; fixed, the end-of-turn hook passes.
#[test]
fn epic_the_installed_hooks_refuse_each_doc_fault_and_pass_the_fixed_plan() {
    let d = TempDir::new().unwrap();
    let p = d.path();
    run("git", p, &["init", "-q", "-b", "main"], "");
    let o = keel(p, &["init"]);
    assert!(o.status.success(), "{}", text(&o));
    let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p.join(".claude/settings.json")).unwrap()).unwrap();
    let cmd = |ev: &str| settings["hooks"][ev][0]["hooks"][0]["command"].as_str().unwrap().to_string();
    let (post, stop) = (cmd("PostToolUse"), cmd("Stop"));
    let hook = |c: &str, payload: &str| run("sh", p, &["-c", c], payload);
    let (plan, data) = ("docs/work/2026-01-01-x-epic-plan.html", "docs/work/2026-01-01-x-epic-plan.yaml");
    let payload = |f: &str| format!(r#"{{"tool_input":{{"file_path":"{f}"}}}}"#);

    let example = std::fs::read_to_string(p.join(".claude/skills/keel-write/references/example.html")).expect("keel init did not install the writing example");
    let page = example.replacen(r#"<h2 id="wus">"#, r#"<h2 id="work">"#, 1);
    assert_ne!(page, example, "the example has no parts heading to mark");
    let fields = epic_plan_data("2026-01-01-x-epic-plan.html", "epic-plan", TWO_WUS);
    std::fs::write(p.join(plan), &page).unwrap();
    std::fs::write(p.join(data), &fields).unwrap();
    for f in [plan, data] {
        let o = hook(&post, &payload(f));
        assert!(o.status.success(), "the example pair fails the after-write hook on {f}: {}", text(&o));
    }

    let unquoted = page.replacen(r#"R["Review round"]"#, "R[Review round]", 1);
    let long = page.replacen(r#"<h2 id="work">"#, &format!(r#"<p>{}</p><h2 id="work">"#, words(300)), 1);
    let no_table: String = fields.lines().filter(|l| !l.starts_with("owner-changes") && !l.starts_with("  - {when")).map(|l| format!("{l}\n")).collect();
    for (fault, file, broken, named) in [
        ("unquoted label", plan, unquoted, "[Review round]"),
        ("missing field", data, no_table, "missing field \"owner-changes\""),
        ("long top", plan, long, "words above the heading with id=\"work\""),
    ] {
        let good = if file == plan { &page } else { &fields };
        assert_ne!(&broken, good, "{fault}: the fixture did not change the pair");
        std::fs::write(p.join(file), &broken).unwrap();
        for (name, c, pay) in [("after-write", &post, payload(file)), ("end-of-turn", &stop, String::new())] {
            let o = hook(c, &pay);
            assert_eq!(o.status.code(), Some(2), "{fault}: the {name} hook did not refuse: {}", text(&o));
            assert!(text(&o).contains(named), "{fault}: the {name} hook does not name it: {}", text(&o));
        }
        std::fs::write(p.join(file), good).unwrap();
    }

    let o = hook(&stop, "");
    assert!(o.status.success(), "the fixed plan fails the end-of-turn hook: {}", text(&o));
}

/// Code review round 1, important 6: an HTML comment in a diagram block is not diagram source.
#[test]
fn an_html_comment_in_a_diagram_block_is_not_scanned() {
    let d = repo();
    let p = d.path();
    let (ok, t) = verify(p, "f.html", &flowchart("<!-- x > y [z] -->\n  A[\"a\"] --> B[\"b\"]"));
    assert!(ok, "{t}");
}

/// Code review round 1, important 8: the diagram block ends at its own closing tag, spaced or
/// nested, never at the first same-name close nor at the end of the doc.
#[test]
fn a_diagram_block_ends_at_its_own_closing_tag() {
    let d = repo();
    let p = d.path();
    let spaced = "<html lang=\"en\"><body><main><pre class=\"mermaid\">flowchart LR\n  A[\"a\"]</pre ><p>see [this]</p></main></body></html>";
    let (ok, t) = verify(p, "f.html", spaced);
    assert!(ok, "the text after the block is not diagram source: {t}");
    let nested = "<html lang=\"en\"><body><main><div class=\"mermaid\">flowchart LR\n<div>A[\"a\"]</div>\n  B[raw]</div></main></body></html>";
    let (ok, t) = verify(p, "f.html", nested);
    assert!(!ok && t.contains("[raw]"), "a nested div does not end the block: {t}");
}

/// Code review round 1, nit (E-DOCSPLIT.W2: of the data file): a typo hint names each known field
/// once, a one-part epic's shared fields included, and every role may carry its today/after table.
#[test]
fn a_typo_hint_names_each_field_once_and_any_role_may_carry_owner_changes() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/e.html"), page_of(10, None)).unwrap();
    let both = epic_plan_data("e.html", "[epic-plan, wu-plan]", TWO_WUS)
        + "requirement: r\nmaps-to: m\ntest-plan: {unit: u, integration: i, e2e: e}\nowner-chages: x\n";
    std::fs::write(p.join("docs/e.yaml"), both).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/e.yaml"]));
    assert!(t.contains("unknown field \"owner-chages\"") && t.matches("owner-changes").count() == 1 && t.matches("expected,").count() == 1, "{t}");
    std::fs::write(p.join("docs/w.html"), page_of(10, None)).unwrap();
    std::fs::write(p.join("docs/w.yaml"), wu_plan_data("w.html", "owner-changes:\n  - {when: w, today: t, after: a}\n")).unwrap();
    let o = keel(p, &["verify-doc", "docs/w.yaml"]);
    assert!(o.status.success(), "a WU plan with the table: {}", text(&o));
    std::fs::write(p.join("docs/w.yaml"), wu_plan_data("w.html", "owner-changes: []\n")).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/w.yaml"]));
    assert!(t.contains("field \"owner-changes\" is empty"), "{t}");
}

/// Code review round 2, B3: a fully quoted diagram with a `%%{…}%%` directive is green.
#[test]
fn a_quoted_diagram_with_a_directive_is_green() {
    let d = repo();
    let p = d.path();
    for directive in
        ["%%{init: {'theme': 'dark'}}%%\nflowchart LR", "%%{init: {'theme': 'dark'}}%% flowchart LR", "flowchart LR\n  %%{\n  init: {'theme': 'dark'}\n  }%%"]
    {
        let doc = format!("<html lang=\"en\"><body><main><pre class=\"mermaid\">\n{directive}\n  A[\"a\"] --> B[\"b\"]\n</pre></main></body></html>");
        let (ok, t) = verify(p, "f.html", &doc);
        assert!(ok, "{directive}: {t}");
        // B22: the label scan reads on after the directive, so an unquoted label there is named.
        let doc = format!("<html lang=\"en\"><body><main><pre class=\"mermaid\">\n{directive}\n  A[\"a\"] --> B[\"b\"]\n  B --> C[after the directive]\n</pre></main></body></html>");
        let (ok, t) = verify(p, "f.html", &doc);
        assert!(!ok && t.contains("[after the directive]"), "{directive}: {t}");
    }
}

/// Epic code review round 1, blocker: Mermaid reads a diagram's source with its markup, so a raw
/// `<br>` or `<b>` in a label is part of the label. The fix hint quotes that source exactly.
#[test]
fn a_label_with_raw_markup_gets_a_hint_that_keeps_the_markup() {
    let d = repo();
    let p = d.path();
    for (body, hint) in [
        (r#"A[a<br>b] --> B["c"]"#, r#"write ["a<br>b"]"#),
        (r#"A[a<br/>b] --> B["c"]"#, r#"write ["a<br/>b"]"#),
        (r#"A[a<b>x</b>b] --> B["c"]"#, r#"write ["a<b>x</b>b"]"#),
        (r#"A["a"] -->|x<br>y| B["c"]"#, r#"write |"x<br>y"|"#),
    ] {
        let (ok, t) = verify(p, "f.html", &flowchart(body));
        assert!(!ok && t.contains(hint), "{body}: {t}");
    }
    let (ok, t) = verify(p, "f.html", &flowchart(r#"A["a<br>b"] -->|"x<br>y"| B["a<b>x</b>b"]"#));
    assert!(ok, "quoted labels with markup are green: {t}");
}

/// Epic code review round 2, blocker 1: edge text that opens with a tag right after the arrow is
/// edge text, not part of the arrow; the hint keeps the diagram.
#[test]
fn edge_text_that_opens_with_a_tag_is_edge_text() {
    let d = repo();
    let p = d.path();
    for (body, named) in [
        ("A--<b>x</b>-->B", r#"diagram label not in quotes: --<b>x</b>--> - write -- "<b>x</b>" -->"#),
        ("A==<b>x</b>==>B", r#"diagram label not in quotes: ==<b>x</b>==> - write == "<b>x</b>" ==>"#),
        ("A-.<i>x</i>.->B", r#"diagram label not in quotes: -.<i>x</i>.-> - write -. "<i>x</i>" .->"#),
    ] {
        let (ok, t) = verify(p, "f.html", &flowchart(body));
        assert!(!ok && t.contains(named) && t.matches("not in quotes").count() == 1, "{body}: {t}");
    }
    let (ok, t) = verify(p, "f.html", &flowchart(r#"A--"<b>x</b>"-->B"#));
    assert!(ok, "{t}");
}

/// Epic code review round 2, blocker 2: a label and its hint are shown as written, entities kept,
/// so the hint pasted back into the page draws the same diagram.
#[test]
fn a_label_with_entities_is_named_and_fixed_as_written() {
    let d = repo();
    let p = d.path();
    for (body, named) in [
        ("A[Vec&lt;String&gt;]", r#"diagram label not in quotes: [Vec&lt;String&gt;] - write ["Vec&lt;String&gt;"]"#),
        ("A[a&amp;lt;b]", r#"diagram label not in quotes: [a&amp;lt;b] - write ["a&amp;lt;b"]"#),
        ("A[a&nbsp;b]", r#"diagram label not in quotes: [a&nbsp;b] - write ["a&nbsp;b"]"#),
    ] {
        let (ok, t) = verify(p, "f.html", &flowchart(body));
        assert!(!ok && t.contains(named), "{body}: {t}");
    }
    let (ok, t) = verify(p, "f.html", &flowchart(r#"A[&quot;a&quot;] --&gt;|&quot;b&quot;| B["Vec&lt;String&gt;"]"#));
    assert!(ok, "entity-written quotes still quote: {t}");
}

/// Epic code review round 2, blocker 3: comment-like text inside a quoted label is label text; it
/// never flips which quotes pair up.
#[test]
fn a_comment_opener_inside_a_quoted_label_is_label_text() {
    let d = repo();
    let p = d.path();
    let (ok, t) = verify(p, "f.html", &flowchart(r#"A["<!--"] --> B["b"] --> C["[y]"]"#));
    assert!(ok, "{t}");
}

/// Epic code review round 2, nit: a diagram block with no closing tag is scanned once, and a later
/// block inside it is not scanned again.
#[test]
fn an_unclosed_diagram_block_is_scanned_once() {
    let d = repo();
    let p = d.path();
    let doc = "<html lang=\"en\"><body><main><div class=\"mermaid\">flowchart LR\n  <pre class=\"mermaid\">flowchart LR\n  A[x]</pre></main></body></html>";
    let (ok, t) = verify(p, "f.html", doc);
    assert!(!ok && t.matches("not in quotes: [x]").count() == 1, "{t}");
}

/// Epic code review round 3, important: a quote written as an entity is a quote, so a comment
/// opener inside an entity-quoted label is label text too.
#[test]
fn a_comment_opener_inside_an_entity_quoted_label_is_label_text() {
    let d = repo();
    let p = d.path();
    for body in [
        r#"A[&quot;<!--&quot;] --> B["b"] --> C["[y]"]"#,
        r#"A[&#34;<!--&#34;] --> B["b"] --> C["[y]"]"#,
        r#"A[&quot;a&quot;] --> B[&quot;<!--&quot;] --> C[&quot;[y]&quot;]"#,
    ] {
        let (ok, t) = verify(p, "f.html", &flowchart(body));
        assert!(ok, "{body}: {t}");
    }
}

/// Epic code review round 4, I1: a quote on a line the scan skips (a `%%` comment, a `style` line)
/// does not decide whether a later HTML comment sits inside a quoted label.
#[test]
fn a_quote_on_a_skipped_line_does_not_unhide_a_later_comment() {
    let d = repo();
    let p = d.path();
    for skipped in ["%% the 5&quot; screen", "%% the 5\" screen", "style A stroke-dasharray: 5&quot;"] {
        let body = format!("{skipped}\n  A[\"a\"] --> B[\"b\"]\n  <!-- A ==> B[x] -->");
        let (ok, t) = verify(p, "f.html", &flowchart(&body));
        assert!(ok, "{skipped}: {t}");
    }
}

/// Epic code review round 4, I2: a signed character reference (`&#+34;`) is text, not a quote, so
/// the hint keeps it as written.
#[test]
fn a_signed_character_reference_is_text() {
    let d = repo();
    let p = d.path();
    for (body, named) in [("A -->|x &#+34; y| B", r#"write |"x &#+34; y"|"#), ("A -->|x &#x+22; y| B", r#"write |"x &#x+22; y"|"#)] {
        let (ok, t) = verify(p, "f.html", &flowchart(body));
        assert!(!ok && t.contains(named), "{body}: {t}");
    }
}
