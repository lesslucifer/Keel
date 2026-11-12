use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

mod common;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
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

#[test]
fn complete_epic_plan_is_green_and_each_wu_block_needs_its_sub_items() {
    let d = repo();
    let p = d.path();
    let items: String = ["intent", "owner-changes", "use-cases", "expected", "non-goals", "wu-summaries", "e2e-plan", "distillation-plan", "open-questions"]
        .map(|i| format!(r#"<p data-item="{i}">x</p>"#))
        .concat();
    let plan = |subs: &[&str]| {
        let wu: String = subs.iter().map(|i| format!(r#"<p data-item="{i}">x</p>"#)).collect();
        format!(r#"<html data-artifact="epic-plan" data-status="ready"><body>{items}<section data-wu="W1">{wu}</section></body></html>"#)
    };
    std::fs::write(p.join("docs/e.html"), plan(&["wu-summary", "wu-maps-to", "wu-expected", "wu-deps"])).unwrap();
    let o = keel(p, &["verify-doc", "docs/e.html"]);
    assert!(o.status.success(), "{}", text(&o));
    std::fs::write(p.join("docs/e.html"), plan(&["wu-summary", "wu-maps-to", "wu-expected"])).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/e.html"]));
    assert!(t.contains("data-wu W1: missing or empty \"wu-deps\""), "{t}");
}

#[test]
fn misspelled_tag_is_reported_by_name_and_draft_is_silent() {
    let d = repo();
    let p = d.path();
    std::fs::write(p.join("docs/a.html"), doc("ready", "expcted", "")).unwrap();
    let o = keel(p, &["verify-doc", "docs/a.html"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1));
    assert!(t.contains("unknown tag data-item=\"expcted\"") && t.contains("missing item \"expected\""), "{t}");
    std::fs::write(p.join("docs/a.html"), doc("draft", "expcted", "")).unwrap();
    assert!(keel(p, &["verify-doc", "docs/a.html"]).status.success());
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
fn collapsed_roles_need_the_union() {
    let d = repo();
    let p = d.path();
    let both = doc("ready", "expected", "").replace("wu-plan", "wu-plan wu-result");
    std::fs::write(p.join("docs/a.html"), both).unwrap();
    let t = text(&keel(p, &["verify-doc", "docs/a.html"]));
    assert!(t.contains("missing item \"outcome\"") && !t.contains("missing item \"expected\""), "{t}");
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

/// A ready epic plan: `header` sits in the title block, `top` and two one-word items in the main
/// content above the first WU, `after` below it. `skip` leaves one checklist item out.
fn epic_plan(skip: &str, header: &str, top: &str, after: &str) -> String {
    let item = |i: &str| if i == skip { String::new() } else { format!(r#"<p data-item="{i}">x</p>"#) };
    let wu: String = ["wu-summary", "wu-maps-to", "wu-expected", "wu-deps"].map(|i| format!(r#"<p data-item="{i}">x</p>"#)).concat();
    let rest: String = ["use-cases", "expected", "non-goals", "e2e-plan", "distillation-plan", "open-questions"].map(item).concat();
    format!(
        r#"<html data-artifact="epic-plan" data-status="ready"><head><title>t</title></head><body><header><h1>{header}</h1></header><main>
<p>{top}</p>{}{}<div data-item="wu-summaries"><div data-wu="E-X.W1">{wu}</div></div>{after}{rest}</main></body></html>"#,
        item("intent"),
        item("owner-changes")
    )
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

/// E-DOCSTYLE.W2: an epic plan carries the "what changes for you" item; a WU plan does not need it.
#[test]
fn an_epic_plan_without_what_changes_for_you_is_red_a_wu_plan_needs_none() {
    let d = repo();
    let p = d.path();
    let (ok, t) = verify(p, "e.html", &epic_plan("owner-changes", "", "", ""));
    assert!(!ok && t.contains("missing item \"owner-changes\""), "{t}");
    let (ok, t) = verify(p, "e.html", &epic_plan("", "", "", ""));
    assert!(ok, "{t}");
    let (ok, t) = verify(p, "w.html", &doc("ready", "expected", ""));
    assert!(ok, "{t}");
}

/// E-DOCSTYLE.W2: an epic plan's top - its main content above the first WU - holds at most 300
/// words; the title block and everything from the first WU on are not counted.
#[test]
fn an_epic_plan_top_over_300_words_is_red_with_its_count() {
    let d = repo();
    let p = d.path();
    let (ok, t) = verify(p, "e.html", &epic_plan("", &words(500), &words(298), &words(500)));
    assert!(ok, "300 words must pass: {t}");
    let (ok, t) = verify(p, "e.html", &epic_plan("", "", &words(299), ""));
    assert!(!ok && t.contains("301 words above the first WU") && t.contains("300"), "{t}");
    let (ok, t) = verify(p, "e.html", &epic_plan("", "", "· — --> | 1 a", ""));
    assert!(ok, "{t}");
    let draft = epic_plan("owner-changes", "", &words(999), "").replace("data-status=\"ready\"", "data-status=\"draft\"");
    let (ok, t) = verify(p, "e.html", &draft);
    assert!(ok, "a draft is silent: {t}");
}

/// E-DOCSTYLE.W2 (UC-DOC-4): every finished doc's flowchart labels are quoted; the error names the
/// label and shows it quoted. Other diagram kinds and drafts are not scanned.
#[test]
fn an_unquoted_flowchart_label_is_red_and_named_in_any_finished_doc() {
    let d = repo();
    let p = d.path();
    for (body, found, fix) in [
        (r#"A[Build step] --> B["Done"]"#, "[Build step]", r#"["Build step"]"#),
        (r#"A["a"] -->|yes| B["b"]"#, "|yes|", r#"|"yes"|"#),
        (r#"A["a"] -- yes --> B["b"]"#, "-- yes -->", r#"-- "yes" -->"#),
        (r#"Q{Ready?} -->|"no"| A(["a"])"#, "{Ready?}", r#"{"Ready?"}"#),
        (r#"A["a"] --&gt;|yes| B["b"]"#, "|yes|", r#"|"yes"|"#),
    ] {
        let (ok, t) = verify(p, "f.html", &flowchart(body));
        assert!(!ok && t.contains(found) && t.contains(fix), "{body}: {t}");
    }
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
        } else if name.to_string_lossy().ends_with(".html") {
            std::fs::copy(e.path(), to.join(&name)).unwrap();
        }
    }
}

/// E-DOCSTYLE.W2: the three checks turn no existing doc red - every doc in Keel's docs/ and the
/// writing skill's example pass.
#[test]
fn every_doc_in_docs_and_the_writing_example_pass_the_new_checks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let d = repo();
    let p = d.path();
    copy_docs(&root.join("docs"), &p.join("docs"));
    std::fs::create_dir_all(p.join("docs/work")).unwrap();
    std::fs::copy(root.join("skills/keel-write/references/example.html"), p.join("docs/work/keel-write-example.html")).unwrap();
    let o = keel(p, &["verify-doc"]);
    assert!(o.status.success(), "{}", text(&o));
}

/// E-DOCSTYLE epic test: `keel init` in a scratch repo installs the writing skill; its example,
/// written as a plan, passes the installed after-write hook; an unquoted label, a missing "what
/// changes for you" section and a top over the cap each make the after-write and end-of-turn
/// hooks fail and name the fault; fixed, the end-of-turn hook passes.
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
    let plan = "docs/work/2026-01-01-x-epic-plan.html";
    let payload = format!(r#"{{"tool_input":{{"file_path":"{plan}"}}}}"#);

    let example = std::fs::read_to_string(p.join(".claude/skills/keel-write/references/example.html")).expect("keel init did not install the writing example");
    std::fs::write(p.join(plan), &example).unwrap();
    let o = hook(&post, &payload);
    assert!(o.status.success(), "the example fails the after-write hook: {}", text(&o));

    let unquoted = example.replacen(r#"R["Review round"]"#, "R[Review round]", 1);
    let no_section = example.replacen(r#" data-item="owner-changes""#, "", 1);
    let long = example.replacen(r#"<h2 id="wus">"#, &format!(r#"<p>{}</p><h2 id="wus">"#, words(300)), 1);
    for (fault, broken, named) in [("unquoted label", unquoted, "[Review round]"), ("missing section", no_section, "missing item \"owner-changes\""), ("long top", long, "words above the first WU")] {
        assert_ne!(broken, example, "{fault}: the fixture did not change the example");
        std::fs::write(p.join(plan), &broken).unwrap();
        for (name, c) in [("after-write", &post), ("end-of-turn", &stop)] {
            let o = hook(c, &payload);
            assert_eq!(o.status.code(), Some(2), "{fault}: the {name} hook did not refuse: {}", text(&o));
            assert!(text(&o).contains(named), "{fault}: the {name} hook does not name it: {}", text(&o));
        }
    }

    std::fs::write(p.join(plan), &example).unwrap();
    let o = hook(&stop, "");
    assert!(o.status.success(), "the fixed plan fails the end-of-turn hook: {}", text(&o));
}
