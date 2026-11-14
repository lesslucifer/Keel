//! E-SEED3 epic e2e: one epic, planned, worked in two WUs, gated at every merge, closed, reviewed,
//! signed and merged into main - through the real hooks, in a scratch repo with a bare local
//! remote. It runs `keel init` for the setup and every gate the workflow names.
//!
//! The epic plan's fourth step (a fresh clone fetches the evidence with a plain fetch) is not run:
//! the notes fetch is deferred to backlog B24 by owner ruling 2026-11-13, so there is no fetch
//! setup to exercise. The close records that deviation; this test never waives a layer.
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

mod common;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

fn run(prog: &str, d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut c = common::cmd(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", path_with_keel())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
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

fn red(o: &Output, needles: &[&str]) {
    let t = text(o);
    assert!(!o.status.success(), "expected red: {t}");
    for n in needles {
        assert!(t.contains(n), "expected {n:?} in: {t}");
    }
}

fn write(d: &Path, rel: &str, body: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, body).unwrap();
}

fn commit(d: &Path, msg: &str) -> String {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", msg]);
    git(d, &["rev-parse", "HEAD"])
}

/// A project set up by `keel init` on `main`, with the base commit pushed to a bare local remote.
/// The remote's TempDir is returned to keep it alive.
fn init_project() -> (TempDir, TempDir) {
    let d = TempDir::new().unwrap();
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    write(
        p,
        ".keel/config.toml",
        "code_paths = [\"src/\", \"tests/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
    );
    ok(keel(p, &["mr", "setup", "--as", "t"], ""));
    write(p, "README", "a project keel adopted\n");
    // DEC-8: the policy and the roster land once, ungated, before the hooks exist.
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "base: the policy and the roster"]);
    let init = ok(keel(p, &["init"], ""));
    assert!(text(&init).contains("hooks"), "init installs the hooks: {}", text(&init));
    // init prints this bootstrap commit itself: the first ungated landing on main.
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "keel init"]);

    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(p, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    ok(run("git", p, &["push", "-q", "origin", "main"], ""));
    (d, remote)
}

fn push(p: &Path, branch: &str) -> Output {
    run("git", p, &["push", "-q", "origin", branch], "")
}

/// WU `a`: code, its integration test, its plan and result, one T2 doc. Signed and merged into the
/// epic through the real pre-merge-commit hook.
fn wu_a(p: &Path) -> String {
    git(p, &["switch", "-q", "-c", "wu/e/a", "epic/e"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    assert 1\n");
    write(p, "docs/work/2026-11-14-e-seed3-e2e-wu-a-plan.html", "<p data-artifact=\"wu-plan\" data-status=\"ready\">plan</p>");
    write(p, "docs/work/2026-11-14-e-seed3-e2e-wu-a-result.html", "<p data-artifact=\"wu-result\" data-status=\"ready\">result</p>");
    write(p, "docs/use-cases/e2e.html", "<p>what the project must do</p>");
    let sha = commit(p, "feat(e2e.W1): a returns one");
    ok(keel(p, &["notes", "write", "test"], r#"{"integration":{"files":["tests/test_a.py"],"covers":"a returns one"}}"#));
    ok(keel(p, &["notes", "write", "doc"], ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    ok(keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/work/2026-11-14-e-seed3-e2e-wu-a-result.html"], ""));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge E2E.W1: a returns one", "wu/e/a"], ""));
    sha
}

/// The first-parent line since the epic's base, as the close must walk it.
fn seams(p: &Path) -> Vec<String> {
    git(p, &["rev-list", "--first-parent", "--reverse", "main..epic/e"]).lines().map(String::from).collect()
}

#[test]
fn seed3_epic_close_merges_pushes_and_keeps_the_evidence() {
    let (d, remote) = init_project();
    let p = d.path();

    // ── 1. the WU level: a clean merge, then a conflicted one finished by hand ────────────────
    git(p, &["switch", "-q", "-c", "epic/e"]);
    // WU b branches before a merges, so its change conflicts with a's on the same line.
    git(p, &["switch", "-q", "-c", "wu/e/b", "epic/e"]);
    write(p, "src/a.py", "def a():\n    return 2\n");
    commit(p, "feat(e2e.W2): a returns two");
    git(p, &["switch", "-q", "epic/e"]);
    let a_sha = wu_a(p);
    let epic_tip = git(p, &["rev-parse", "HEAD"]);

    let o = run("git", p, &["merge", "-q", "--no-ff", "wu/e/b"], "");
    assert!(!o.status.success(), "the two WUs conflict: {}", text(&o));
    write(p, "src/a.py", "def a():\n    return 3\n");
    git(p, &["add", "src/a.py"]);
    // A merge finished with `git commit` runs the gate pre-merge-commit would have run; b has no
    // evidence yet, so it must be refused and nothing lands.
    let hand = run("git", p, &["commit", "-qm", "merge E2E.W2 by hand"], "");
    red(&hand, &["1·notes", "no test/v1 note"]);
    assert_eq!(git(p, &["rev-parse", "HEAD"]), epic_tip, "nothing landed");
    git(p, &["merge", "--abort"]);

    // b gets its evidence at its own tip, then the same hand-finished merge lands.
    git(p, &["switch", "-q", "wu/e/b"]);
    write(p, "tests/test_b.py", "def test_b():\n    assert 2\n");
    write(p, "docs/work/2026-11-14-e-seed3-e2e-wu-b-plan.html", "<p data-artifact=\"wu-plan\" data-status=\"ready\">plan</p>");
    write(p, "docs/work/2026-11-14-e-seed3-e2e-wu-b-result.html", "<p data-artifact=\"wu-result\" data-status=\"ready\">result</p>");
    commit(p, "test(e2e.W2): b returns two");
    ok(keel(p, &["notes", "write", "test"], r#"{"integration":{"files":["tests/test_b.py"],"covers":"a returns two"}}"#));
    ok(keel(p, &["notes", "write", "doc"], ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    ok(keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/work/2026-11-14-e-seed3-e2e-wu-b-result.html"], ""));
    git(p, &["switch", "-q", "epic/e"]);
    let o = run("git", p, &["merge", "-q", "--no-ff", "wu/e/b"], "");
    assert!(!o.status.success(), "the conflict is still there: {}", text(&o));
    write(p, "src/a.py", "def a():\n    return 3\n");
    git(p, &["add", "src/a.py"]);
    let hand = run("git", p, &["commit", "-qm", "Merge E2E.W2: a returns two"], "");
    assert!(hand.status.success(), "with evidence the hand-finished merge lands: {}", text(&hand));
    let epic_wus = git(p, &["rev-list", "--count", "--first-parent", "main..epic/e"]);
    assert_eq!(epic_wus, "2", "two WU merges on the epic's first-parent line");
    assert!(run("git", p, &["merge-base", "--is-ancestor", &a_sha, "epic/e"], "").status.success(), "the first WU is in the epic");

    // ── 2. the close: archive the working docs, then a review fix ─────────────────────────────
    let archived = [
        "docs/work/2026-11-14-e-seed3-e2e-wu-a-plan.html",
        "docs/work/2026-11-14-e-seed3-e2e-wu-a-result.html",
        "docs/work/2026-11-14-e-seed3-e2e-wu-b-plan.html",
        "docs/work/2026-11-14-e-seed3-e2e-wu-b-result.html",
    ];
    let mut rm = vec!["rm", "-q"];
    rm.extend(archived.iter().copied());
    git(p, &rm);
    write(p, "docs/decisions/close.html", "<p>what the close taught</p>");
    write(p, "docs/.archive/2026-11-14-e2e-epic-result.html", "<p data-artifact=\"epic-result\" data-status=\"ready\">result</p>");
    git(p, &["add", "docs/decisions/close.html"]);
    git(p, &["commit", "-qm", "docs(E2E close): archive the working docs, distilled to the T2 set"]);
    let archive = git(p, &["rev-parse", "HEAD"]);
    let distill: Vec<String> = archived.iter().map(|t| format!("{t}=docs/decisions/close.html")).collect();
    let epic_doc_note = |p: &Path| {
        let mut a = vec!["notes", "write", "doc"];
        for d in &distill {
            a.extend(["--distill", d.as_str()]);
        }
        ok(keel(p, &a, ""));
    };
    let epic_notes = |p: &Path| {
        ok(keel(
            p,
            &["notes", "write", "test"],
            r#"{"e2e":{"files":["tests/test_a.py","tests/test_b.py"],"covers":"the WU flow, the close and the merge into main"}}"#,
        ));
        epic_doc_note(p);
    };
    epic_notes(p);
    let doc_before = git(p, &["notes", "--ref=doc", "show", &archive]);
    let test_before = git(p, &["notes", "--ref=test", "show", &archive]);

    // A review fix after the archive: the archive commit becomes a walked seam, judged at close
    // level on the epic's notes - no note is rewritten.
    write(p, "src/a.py", "def a():\n    return 4\n");
    let fix = commit(p, "fix(e2e close): review round 1");
    epic_notes(p);
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    ok(keel(p, &["mr", "confirm", "epic", "--as", "t", "--report", "docs/.archive/2026-11-14-e2e-epic-result.html"], ""));
    let gate = ok(keel(p, &["verify-dod", "epic", "epic/e"], ""));
    assert!(text(&gate).contains("verify-dod epic green"), "{}", text(&gate));
    assert_eq!(git(p, &["notes", "--ref=doc", "show", &archive]), doc_before, "the archive commit's doc note was not rewritten");
    assert_eq!(git(p, &["notes", "--ref=test", "show", &archive]), test_before, "the archive commit's test note was not rewritten");
    assert!(seams(p).contains(&archive), "the archive commit is on the epic's first-parent line and was walked");

    // ── 3. the epic merges into main, and both pushes pass the real pre-push hook ─────────────
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge epic E2E", "epic/e"], ""));
    ok(push(p, "main"));
    ok(push(p, "epic/e"));
    assert_eq!(git(remote.path(), &["rev-parse", "main^2"]), fix, "main's second parent is the signed epic tip");
    assert_eq!(git(remote.path(), &["rev-parse", "epic/e"]), fix, "the epic branch is pushed and still pointed at the signed tip");
    assert_eq!(git(remote.path(), &["rev-parse", "refs/notes/cr"]), git(p, &["rev-parse", "refs/notes/cr"]), "the evidence rode the push");
}
