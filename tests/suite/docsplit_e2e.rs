//! E-DOCSPLIT epic test (DEC-69), through the built binary in a scratch repo. W2 starts it: the hooks
//! `keel init` installs check a plan's data file at the write and at turn end - one missing a field
//! is refused, naming it; fixed, it passes. W3 extends it to the sign-off and the archive: a WU
//! signed through its result's data file, merged, and the epic closed with each pair archived
//! under one lineage line.
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

fn git(d: &Path, a: &[&str]) -> String {
    let o = run("git", d, a, "");
    assert!(o.status.success(), "git {a:?}: {}", text(&o));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn keel(d: &Path, a: &[&str], input: &str) -> Output {
    run(env!("CARGO_BIN_EXE_keel"), d, a, input)
}

fn ok(o: Output) -> Output {
    assert!(o.status.success(), "{}", text(&o));
    o
}

fn put(d: &Path, rel: &str, body: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, body).unwrap();
}

fn commit(d: &Path, msg: &str) {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", msg]);
}

/// A short page, as DEC-69 has it: no checklist tags.
fn page(title: &str) -> String {
    format!("<!DOCTYPE html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><title>{title}</title></head>\n<body><main><h2 id=\"what\">What happened</h2><p>{title}: done.</p></main></body>\n</html>\n")
}

const WU_RESULT: &str = r#"artifact: wu-result
status: ready
id: E-X.W1
epic: E-X
title: What this part changed
date: "2026-11-18"
page: 2026-11-18-x-w1-result.html
outcome: Delivered.
deviations: none
evidence:
  tests: tests/test_a.py
distillation: docs/decisions/x.html
follow-ups: none
"#;

const EPIC_RESULT: &str = r#"artifact: epic-result
status: ready
id: E-X
title: The epic, closed
date: "2026-11-18"
page: 2026-11-18-x-result.html
outcome: Delivered.
wu-rollup:
  - E-X.W1 delivered
deviations: none
evidence:
  e2e: tests/test_a.py
distillation: docs/decisions/x.html
follow-ups: none
"#;

/// The rest of the epic flow (E-DOCSPLIT.W3), through the built binary and the hooks `keel init`
/// installs: a WU commits its plan pair and its result pair; the sign-off finds the result page
/// through its data file, with no --report, and the WU merges into the epic through the merge hook.
/// At the close, the result page archived without its data file is refused, naming the file left;
/// archived whole, the plan pair and the result pair take one --distill each, the epic result pair
/// sits in docs/.archive, the epic sign-off names its page, and the epic gate is green. No waiver.
#[test]
fn epic_a_wu_signs_by_its_result_s_data_file_and_the_epic_closes_with_one_lineage_line_per_pair() {
    let d = TempDir::new().unwrap();
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    put(
        p,
        ".keel/config.toml",
        "code_paths = [\"src/\", \"tests/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
    );
    ok(keel(p, &["mr", "setup", "--as", "t"], ""));
    put(p, "README", "a project keel adopted\n");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "base: the policy and the roster"]);
    ok(keel(p, &["init"], ""));
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "keel init"]);
    git(p, &["switch", "-q", "-c", "epic/E-X"]);

    // ── the WU: code, its test, its plan pair and its result pair ────────────────────────────────
    git(p, &["switch", "-q", "-c", "wu/E-X/w1"]);
    let (plan, plan_data) = ("docs/work/2026-11-18-x-w1-plan.html", "docs/work/2026-11-18-x-w1-plan.yaml");
    let (result, result_data) = ("docs/work/2026-11-18-x-w1-result.html", "docs/work/2026-11-18-x-w1-result.yaml");
    put(p, plan, PAGE);
    put(p, plan_data, DATA);
    put(p, result, &page("E-X.W1 result"));
    put(p, result_data, WU_RESULT);
    put(p, "src/a.py", "def a():\n    return 1\n");
    put(p, "tests/test_a.py", "def test_a():\n    assert 1\n");
    ok(keel(p, &["verify-doc", plan, plan_data, result, result_data], ""));
    commit(p, "feat(E-X.W1): a returns one");
    ok(keel(p, &["notes", "write", "test"], r#"{"integration":{"files":["tests/test_a.py"],"covers":"a returns one"}}"#));
    ok(keel(p, &["notes", "write", "doc"], ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    let o = ok(keel(p, &["mr", "confirm", "wu", "--as", "t"], ""));
    assert!(text(&o).contains(result), "the sign-off names the result page: {}", text(&o));
    let mr: serde_json::Value = serde_json::from_str(&git(p, &["notes", "--ref=mr", "show", "HEAD"])).unwrap();
    assert_eq!(mr["report"], result, "the page is attested, found through its data file");
    git(p, &["switch", "-q", "epic/E-X"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge E-X.W1: a returns one", "wu/E-X/w1"], ""));

    // ── the close: the result page archived without its data file is refused ──────────────────────
    put(p, "docs/decisions/x.html", "<p>what the epic taught</p>");
    put(p, "docs/.archive/2026-11-18-x-result.html", &page("E-X result"));
    put(p, "docs/.archive/2026-11-18-x-result.yaml", EPIC_RESULT);
    ok(keel(p, &["verify-doc", "docs/.archive/2026-11-18-x-result.html", "docs/.archive/2026-11-18-x-result.yaml"], ""));
    git(p, &["rm", "-q", plan, plan_data, result]);
    commit(p, "docs(E-X close): archive the working docs");
    assert!(git(p, &["status", "--porcelain", "--ignored"]).contains("docs/.archive/"), "the epic result pair stays local");
    ok(keel(p, &["notes", "write", "test"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"the WU, its sign-off and the close"}}"#));
    let distill =
        ["--distill", "docs/work/2026-11-18-x-w1-plan.yaml=docs/decisions/x.html", "--distill", "docs/work/2026-11-18-x-w1-result.html=docs/decisions/x.html"];
    let mut doc = vec!["notes", "write", "doc"];
    doc.extend(distill);
    let o = keel(p, &doc, "");
    assert!(!o.status.success(), "a pair archived by half: {}", text(&o));
    assert!(
        text(&o).contains(&format!("{result} is archived but {result_data}, the other half of its doc, is still at the tip"))
            && text(&o).contains(&format!("git rm {result_data} too, in the archive commit itself")),
        "{}",
        text(&o)
    );

    // ── archived whole: one line per pair, and the epic gate is green ─────────────────────────────
    git(p, &["rm", "-q", result_data]);
    git(p, &["commit", "-q", "--amend", "--no-edit"]);
    ok(keel(p, &["notes", "write", "test"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"the WU, its sign-off and the close"}}"#));
    ok(keel(p, &doc, ""));
    let note: serde_json::Value = serde_json::from_str(&git(p, &["notes", "--ref=doc", "show", "HEAD"])).unwrap();
    for f in [plan, plan_data, result, result_data] {
        let c = note["changes"].as_array().unwrap().iter().find(|c| c["path"] == f).unwrap_or_else(|| panic!("{f} not in {note}"));
        assert_eq!(c["distilled_to"], serde_json::json!(["docs/decisions/x.html"]), "{f}");
    }
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    let o = keel(p, &["mr", "confirm", "epic", "--as", "t", "--report", "docs/.archive/2026-11-18-x-result.yaml"], "");
    assert!(!o.status.success() && text(&o).contains("--report docs/.archive/2026-11-18-x-result.html"), "a data file is not the report: {}", text(&o));
    ok(keel(p, &["mr", "confirm", "epic", "--as", "t", "--report", "docs/.archive/2026-11-18-x-result.html"], ""));
    let gate = ok(keel(p, &["verify-dod", "epic", "epic/E-X"], ""));
    assert!(text(&gate).contains("verify-dod epic green"), "{}", text(&gate));
    assert!(!text(&gate).contains("WAIVED"), "no waiver: {}", text(&gate));
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge epic E-X", "epic/E-X"], ""));
}
