//! E-GHFLOW2.W6: work that need not wait goes in its own pull request. A piece of a WU that waits
//! is a task, `wu/<e>/<wu>_<task>`, merged into its WU branch (by the owner, on GitHub); the WU keeps
//! one range, gated and signed on its own last commit. On a task branch the commit check takes a
//! rule-bearing commit with no code and no roster without a trailer; a range with no code needs no
//! result report under `[mr] report = "required"`.
compiled!();

use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common;

fn run(prog: &str, d: &Path, a: &[&str]) -> Output {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    common::cmd(prog).args(a).current_dir(d).env("PATH", common::path_with(bin)).env_remove("SSH_AUTH_SOCK").stdin(Stdio::null()).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn git(d: &Path, a: &[&str]) -> String {
    let o = run("git", d, a);
    assert!(o.status.success(), "git {a:?}: {}", text(&o));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn keel(d: &Path, a: &[&str]) -> Output {
    run(env!("CARGO_BIN_EXE_keel"), d, a)
}

fn keel_in(d: &Path, a: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    let mut c = common::cmd(env!("CARGO_BIN_EXE_keel"))
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

fn ok(o: Output) -> Output {
    assert!(o.status.success(), "{}", text(&o));
    o
}

fn write(d: &Path, rel: &str, body: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, body).unwrap();
}

/// A commit with every hook, so the commit check judges it; returns the output.
fn try_commit(d: &Path, paths: &[&str], msg: &str) -> Output {
    let mut a = vec!["add", "--"];
    a.extend(paths);
    git(d, &a);
    run("git", d, &["commit", "-qm", msg])
}

const FIXTURE: &str = "split prs";
const CONFIG: &str = "code_paths = [\"src/\", \"tests/\"]\nrule_bearing = [\"skills/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n";

/// main (the roster, signer `t` by username) → epic/e → wu/e/x with code, a test and a result
/// report; hooks installed. HEAD on wu/e/x. A copy of a template built once per run.
fn fixture() -> TempDir {
    common::fixture(FIXTURE, |p| {
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@t"]);
        git(p, &["config", "user.name", "t"]);
        write(p, ".keel/config.toml", CONFIG);
        ok(keel(p, &["mr", "setup", "--as", "t"]));
        write(p, "README", "x");
        write(p, "skills/s/SKILL.md", "a rule\n");
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", "base\n\nReviewed-By: username t"]);
        ok(keel(p, &["hooks", "install"]));
        git(p, &["switch", "-q", "-c", "epic/e"]);
        git(p, &["switch", "-q", "-c", "wu/e/x"]);
        write(p, "src/a.py", "def a():\n    return 1\n");
        write(p, "tests/test_a.py", "def test_a():\n    pass\n");
        common::doc_pair(p, "docs/work/x-result.html", "wu-result", RESULT);
        git(p, &["add", "-A"]);
        git(p, &["commit", "-qm", "wu x"]);
    })
}

const RESULT: &str = "<html><p>wu x: done</p></html>";

/// Item 3: an untrailered skill edit is refused on a WU branch and taken on a task branch; with a
/// code path or the roster staged too it is refused on the task branch as anywhere.
#[test]
fn a_task_commit_with_rule_text_only_needs_no_trailer() {
    let d = fixture();
    let p = d.path();
    write(p, "skills/s/SKILL.md", "a rule\none more line\n");
    let o = try_commit(p, &["skills/s/SKILL.md"], "skill line");
    assert!(!o.status.success() && text(&o).contains("skills/s/SKILL.md") && text(&o).contains("Reviewed-By"), "on the WU branch: {}", text(&o));

    git(p, &["switch", "-q", "-c", "wu/e/x_skill-line"]);
    let o = ok(try_commit(p, &["skills/s/SKILL.md"], "skill line"));
    let t = text(&o);
    assert!(t.contains("skills/s/SKILL.md") && t.contains("task") && t.contains("covers"), "the hook says why it lets it through: {t}");

    write(p, "skills/s/SKILL.md", "a rule\none more line\nand another\n");
    write(p, "src/b.py", "def b():\n    return 2\n");
    let o = try_commit(p, &["skills/s/SKILL.md", "src/b.py"], "skill and code");
    assert!(!o.status.success() && text(&o).contains("Reviewed-By"), "a code path staged too: {}", text(&o));
    git(p, &["reset", "-q", "src/b.py"]);

    let roster = std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap();
    write(p, ".keel/mr-allowed", &format!("{roster}username: ghost\n"));
    let o = try_commit(p, &["skills/s/SKILL.md", ".keel/mr-allowed"], "skill and roster");
    assert!(!o.status.success() && text(&o).contains("Reviewed-By"), "the roster staged too: {}", text(&o));
    git(p, &["reset", "-q", ".keel/mr-allowed"]);
    std::fs::write(p.join(".keel/mr-allowed"), &roster).unwrap();

    // W6 security review: any .keel/ path staged gets no relaxation, so a commit cannot redefine
    // code_paths for itself; and the relaxation reads code_paths from HEAD's committed config, so an
    // unstaged edit of the working-tree config does not either
    let config = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &config.replace("code_paths = [\"src/\", \"tests/\"]", "code_paths = [\"nothing/\"]"));
    let o = try_commit(p, &["skills/s/SKILL.md", ".keel/config.toml"], "skill and config");
    assert!(!o.status.success() && text(&o).contains("Reviewed-By"), "a .keel/ path staged too: {}", text(&o));
    git(p, &["reset", "-q", ".keel/config.toml"]);
    write(p, "src/b.py", "def b():\n    return 2\n");
    let o = try_commit(p, &["skills/s/SKILL.md", "src/b.py"], "skill and code, the working-tree config redefining code");
    assert!(!o.status.success() && text(&o).contains("Reviewed-By"), "HEAD's code_paths judge: {}", text(&o));
    git(p, &["reset", "-q", "src/b.py"]);
    std::fs::remove_file(p.join("src/b.py")).unwrap();
    write(p, ".keel/config.toml", &config);
    ok(try_commit(p, &["skills/s/SKILL.md"], "the skill line alone, again"));
}

/// Item 3: only an exact task name relaxes the check: not an epic branch, a detached HEAD, a fourth
/// segment, a second `_` or an empty side.
#[test]
fn only_an_exact_task_branch_relaxes_the_commit_check() {
    let d = fixture();
    let p = d.path();
    let wu = git(p, &["rev-parse", "HEAD"]);
    for (i, how) in ["epic/e", "detached", "wu/e/y_b/c", "wu/e/y_b_c", "wu/e/y_", "wu/e/_b"].iter().enumerate() {
        match *how {
            "epic/e" => git(p, &["switch", "-q", "epic/e"]),
            "detached" => git(p, &["switch", "-q", "--detach", &wu]),
            name => git(p, &["switch", "-q", "-c", name, &wu]),
        };
        write(p, "skills/s/SKILL.md", &format!("a rule\nline {i}\n"));
        let o = try_commit(p, &["skills/s/SKILL.md"], "skill line");
        assert!(!o.status.success() && text(&o).contains("Reviewed-By"), "{how}: {}", text(&o));
        git(p, &["reset", "-q", "--hard"]);
    }
}

/// The test, doc and cr notes a green WU tip carries (the cr note as the review skill writes it).
fn notes(p: &Path, code: bool) {
    if code {
        ok(keel_in(p, &["notes", "write", "test"], r#"{"integration": {"files": ["tests/test_a.py"], "covers": "a returns 1"}}"#));
    }
    ok(keel(p, &["notes", "write", "doc"]));
    if code {
        ok(keel_in(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    }
}

/// Item 1: two tasks of one WU, each off the WU branch, merged into it as GitHub merges a pull
/// request (`--no-ff`); the WU is gated, reviewed and signed on its own last commit, which covers
/// both; the epic is green after the WU merges.
#[test]
fn two_tasks_merged_into_a_wu_are_gated_and_signed_with_it_on_its_last_commit() {
    let d = fixture();
    let p = d.path();
    git(p, &["switch", "-q", "-c", "wu/e/x_code"]);
    write(p, "src/c.py", "def c():\n    return 3\n");
    ok(try_commit(p, &["src/c.py"], "task: code"));
    git(p, &["switch", "-q", "-c", "wu/e/x_skill", "wu/e/x"]);
    write(p, "skills/s/SKILL.md", "a rule\none more line\n");
    ok(try_commit(p, &["skills/s/SKILL.md"], "task: skill line"));
    git(p, &["switch", "-q", "wu/e/x"]);
    for task in ["wu/e/x_code", "wu/e/x_skill"] {
        ok(run("git", p, &["merge", "-q", "--no-ff", "-m", &format!("Merge pull request from {task}"), task]));
    }
    let base = git(p, &["merge-base", "epic/e", "HEAD"]);
    let files = git(p, &["diff", "--name-only", &base, "HEAD"]);
    for f in ["src/a.py", "src/c.py", "skills/s/SKILL.md"] {
        assert!(files.lines().any(|l| l == f), "the WU's range holds {f}: {files}");
    }
    notes(p, true);
    let o = keel(p, &["verify-dod", "wu", "wu/e/x"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("8·manual-review") && !t.contains("FAIL 1·") && !t.contains("FAIL 3·"), "only the sign-off is missing: {t}");
    ok(keel(p, &["mr", "confirm", "wu", "--as", "t"]));
    ok(keel(p, &["verify-dod", "wu", "wu/e/x"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "merge wu x", "wu/e/x"]));
}

/// Item 4: under `report = "required"` a WU with no code signs without a report, typed and at check
/// 8; a WU with code and no report is still refused and red. `optional` and `none` are unchanged.
#[test]
fn a_range_without_code_needs_no_report_and_a_range_with_code_still_does() {
    for policy in ["required", "optional", "none"] {
        for code in [false, true] {
            let case = format!("{policy}, {}", if code { "code" } else { "docs only" });
            let d = fixture();
            let p = d.path();
            if policy != "required" {
                git(p, &["switch", "-q", "main"]);
                write(p, ".keel/config.toml", &format!("{CONFIG}[mr]\nreport = \"{policy}\"\n"));
                git(p, &["add", "-A"]);
                git(p, &["commit", "-q", "--no-verify", "-m", "policy"]);
                git(p, &["switch", "-q", "epic/e"]);
                ok(run("git", p, &["merge", "-q", "--no-ff", "--no-verify", "-m", "policy", "main"]));
            }
            git(p, &["switch", "-q", "-c", "wu/e/d", "epic/e"]);
            if code {
                write(p, "src/d.py", "def d():\n    return 4\n");
                write(p, "tests/test_a.py", "def test_a():\n    pass\n\ndef test_d():\n    pass\n");
            } else {
                write(p, "docs/guide.md", "how to\n");
            }
            git(p, &["add", "-A"]);
            git(p, &["commit", "-qm", "wu d"]);
            if code {
                ok(keel_in(p, &["notes", "write", "test"], r#"{"integration": {"files": ["tests/test_a.py"], "covers": "d"}}"#));
                ok(keel(p, &["notes", "write", "doc"]));
                ok(keel_in(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
            }
            let o = keel(p, &["mr", "confirm", "wu", "--as", "t"]);
            if policy == "required" && code {
                let t = text(&o);
                assert!(!o.status.success() && t.contains("report = \"required\"") && t.contains("fix ->"), "{case}: {t}");
                continue;
            }
            assert!(o.status.success(), "{case}: {}", text(&o));
            let o = keel(p, &["verify-dod", "wu", "wu/e/d"]);
            assert!(o.status.success(), "{case}: {}", text(&o));
        }
    }
}
