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
    std::fs::write(p.join(".keel/config.toml"), "[gate0]\nlint=\"true\"\ntest=\"true\"\n[branches]\nmain=\"main\"\n[docs]\nroot=\"docs/\"\n").unwrap();
    std::fs::create_dir(p.join("docs")).unwrap();
    d
}

#[test]
fn seed_plan_verifies_green() {
    let plan = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/work/2026-11-07-seed-plan.html");
    let d = repo();
    let o = keel(d.path(), &["verify-doc", plan.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
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
    let stop = p.join(".keel/hooks/verify-doc-stop");
    let post = p.join(".keel/hooks/verify-doc-post");
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
