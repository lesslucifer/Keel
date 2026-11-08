use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

fn run(prog: &str, d: &Path, a: &[&str], stdin: &str) -> Output {
    let mut c = Command::new(prog)
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
    let items: String = ["intent", "use-cases", "expected", "non-goals", "wu-summaries", "e2e-plan", "distillation-plan", "open-questions"]
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
