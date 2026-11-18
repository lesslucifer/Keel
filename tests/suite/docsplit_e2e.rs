//! E-DOCSPLIT epic test (DEC-69), through the built binary in a scratch repo. W2 starts it: the hooks
//! `keel init` installs check a plan's data file at the write and at turn end - one missing a field
//! is refused, naming it; fixed, it passes. W3 extends it to the sign-off and the archive.
compiled!();

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common;

fn run(prog: &str, d: &Path, a: &[&str], stdin: &str) -> Output {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    let mut c = common::cmd(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", common::path_with(bin))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

const PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>Keel - E-X.W1 plan</title></head>
<body>
  <main>
    <h2 id="problem">The problem</h2>
    <p>A reader cannot tell what this part changes.</p>
    <h2 id="work">What it makes</h2>
    <ul><li>One short page and its data file.</li></ul>
  </main>
</body>
</html>
"#;

const DATA: &str = r#"artifact: wu-plan
status: ready
id: E-X.W1
epic: E-X
title: What this part changes
date: "2026-11-18"
page: 2026-11-18-x-w1-plan.html
requirement: After W1, the page says what changes.
maps-to: E-X.W1
expected:
  - The page is green.
test-plan:
  unit: the field table
  integration: the doc check on the pair
  e2e: none, the epic's test covers it
"#;

/// The epic flow's first step: a WU plan written as a page and a data file. The data file missing
/// its requirement is refused by the after-write hook (as Claude Code and as Codex name the file)
/// and by the end-of-turn hook, each naming the field; written as a draft it is silent; fixed it
/// passes both. A YAML file written outside docs/ never reaches the check.
#[test]
fn epic_the_installed_hooks_refuse_a_data_file_missing_a_field_and_pass_it_fixed() {
    let d = TempDir::new().unwrap();
    let p = d.path();
    run("git", p, &["init", "-q", "-b", "main"], "");
    let o = run(env!("CARGO_BIN_EXE_keel"), p, &["init"], "");
    assert!(o.status.success(), "{}", text(&o));
    let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p.join(".claude/settings.json")).unwrap()).unwrap();
    let cmd = |ev: &str| settings["hooks"][ev][0]["hooks"][0]["command"].as_str().unwrap().to_string();
    let (post, stop) = (cmd("PostToolUse"), cmd("Stop"));
    let hook = |c: &str, payload: &str| run("sh", p, &["-c", c], payload);
    let (page, data) = ("docs/work/2026-11-18-x-w1-plan.html", "docs/work/2026-11-18-x-w1-plan.yaml");
    let claude = format!(r#"{{"tool_input":{{"file_path":"{}"}}}}"#, p.join(data).display());
    let codex = format!(r#"{{"tool_input":{{"command":"*** Begin Patch\n*** Add File: {data}\n+x\n*** End Patch"}}}}"#);

    std::fs::write(p.join(page), PAGE).unwrap();
    let missing: String = DATA.lines().filter(|l| !l.starts_with("requirement:")).map(|l| format!("{l}\n")).collect();
    std::fs::write(p.join(data), &missing).unwrap();
    for (name, c, payload) in [("after-write (Claude Code)", &post, &claude), ("after-write (Codex)", &post, &codex), ("end-of-turn", &stop, &String::new())] {
        let o = hook(c, payload);
        assert_eq!(o.status.code(), Some(2), "the {name} hook did not refuse: {}", text(&o));
        assert!(text(&o).contains("2026-11-18-x-w1-plan.yaml: missing field \"requirement\""), "the {name} hook does not name it: {}", text(&o));
    }

    std::fs::write(p.join(data), missing.replace("status: ready", "status: draft")).unwrap();
    for c in [&post, &stop] {
        let o = hook(c, &claude);
        assert!(o.status.success(), "a draft is silent: {}", text(&o));
    }

    std::fs::write(p.join(data), DATA).unwrap();
    for (name, c, payload) in [("after-write", &post, &claude), ("end-of-turn", &stop, &String::new())] {
        let o = hook(c, payload);
        assert!(o.status.success(), "the fixed data file fails the {name} hook: {}", text(&o));
    }

    std::fs::create_dir_all(p.join(".github/workflows")).unwrap();
    std::fs::write(p.join(".github/workflows/x.yaml"), "on: [push\n").unwrap();
    let o = hook(&post, r#"{"tool_input":{"file_path":".github/workflows/x.yaml"}}"#);
    assert!(o.status.success() && text(&o).is_empty(), "a YAML file outside docs/ is no doc: {}", text(&o));
}
