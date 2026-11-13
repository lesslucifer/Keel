//! E-SEED3.W2: the push and the epic walk judge an epic-close commit by the epic-range notes it
//! carries (B23, B20). Every merge and push runs through the real hooks into a bare local remote.
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
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
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

fn push(p: &Path, branch: &str) -> Output {
    run("git", p, &["push", "-q", "origin", branch], "")
}

const WU_TESTS: &str = r#"{"integration":{"files":["tests/test_a.py"],"covers":"a works"}}"#;
const EPIC_TESTS: &str = r#"{"e2e":{"waived":"no e2e harness in the fixture"}}"#;

/// main → epic/e with WU a merged through the real hook (code, a test, a T3 plan, a doc), the
/// epic pushed to a bare remote. HEAD on epic/e. The remote's TempDir is returned to keep it alive.
fn epic_with_a_merged_wu() -> (TempDir, TempDir) {
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
    write(p, "README", "x");
    commit(p, "base");
    ok(keel(p, &["hooks", "install"], ""));
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(p, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    ok(push(p, "main"));

    git(p, &["switch", "-q", "-c", "epic/e"]);
    git(p, &["switch", "-q", "-c", "wu/e/a"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    pass\n");
    write(p, "docs/work/plan.html", "<p>plan</p>");
    write(p, "docs/a.html", "<p>a</p>");
    commit(p, "wu a");
    ok(keel(p, &["notes", "write", "test"], WU_TESTS));
    ok(keel(p, &["notes", "write", "doc"], ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    ok(keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"], ""));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "merge wu a", "wu/e/a"], ""));
    ok(push(p, "epic/e"));
    (d, remote)
}

/// The close's archive commit: the T3 plan distilled and removed, the epic result committed, and
/// the epic's test and doc notes written on it while it is the tip. Returns its sha.
fn archive_commit(p: &Path) -> String {
    git(p, &["rm", "-q", "docs/work/plan.html"]);
    write(p, "docs/decisions/d.html", "<p>why</p>");
    write(p, "docs/result.html", "<p>result</p>");
    let sha = commit(p, "archive the epic's working docs");
    epic_notes(p);
    sha
}

/// The epic-range test and doc notes on HEAD, as the close writes them on the epic tip.
fn epic_notes(p: &Path) {
    ok(keel(p, &["notes", "write", "test"], EPIC_TESTS));
    epic_doc_note(p);
}

/// The epic-range doc note on HEAD, with the lineage of the plan the close archived.
fn epic_doc_note(p: &Path) {
    let archived = git(p, &["log", "--diff-filter=D", "--format=", "--name-only", "main..HEAD", "--", "docs/work"]);
    let distill = if archived.is_empty() { vec![] } else { vec!["--distill", "docs/work/plan.html=docs/decisions/d.html"] };
    ok(keel(p, &[&["notes", "write", "doc"][..], &distill].concat(), ""));
}

/// A review fix on the epic: a code change, with the epic's notes on it while it is the tip.
fn review_fix(p: &Path, n: u32) -> String {
    write(p, "src/a.py", &format!("def a():\n    return {}\n", n + 1));
    let sha = commit(p, &format!("fix: epic review round {n}"));
    epic_notes(p);
    sha
}

fn review_and_sign_epic(p: &Path) {
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    ok(keel(p, &["mr", "confirm", "epic", "--as", "t", "--report", "docs/result.html"], ""));
}

/// The real case of E-DOCSTYLE's fa355e0: after the epic merged into main, pushing its branch was
/// refused - its tip judged as a plain commit on notes written for the whole epic.
#[test]
fn pushing_an_epic_branch_after_its_merge_into_main_passes_the_real_pre_push_hook() {
    let (d, remote) = epic_with_a_merged_wu();
    let p = d.path();
    archive_commit(p);
    let tip = review_fix(p, 1);
    review_and_sign_epic(p);
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "merge epic e", "epic/e"], ""));
    ok(push(p, "main"));

    let o = push(p, "epic/e");
    assert!(o.status.success(), "a pushed epic branch whose commits are all on main is not re-judged: {}", text(&o));
    assert_eq!(git(remote.path(), &["rev-parse", "epic/e"]), tip);
}

/// Open point 8: an epic branch pushed mid-close, before its review, as a backup. Its tip is judged
/// on the epic notes it carries (checks 1, 2, 6 over the epic range), never skipped.
#[test]
fn an_epic_branch_pushed_mid_close_is_judged_on_its_tips_epic_notes() {
    let (d, remote) = epic_with_a_merged_wu();
    let p = d.path();
    let archive = archive_commit(p);
    let o = push(p, "epic/e");
    assert!(o.status.success(), "the archive commit, pushed as the tip, is judged over the epic: {}", text(&o));
    assert_eq!(git(remote.path(), &["rev-parse", "epic/e"]), archive);

    write(p, "src/a.py", "def a():\n    return 2\n");
    let fix = commit(p, "fix: epic review round 1");
    ok(keel(p, &["notes", "write", "test"], EPIC_TESTS));
    red(&push(p, "epic/e"), &["1·notes", "no doc/v1 note", &fix[..10]]);
    assert_eq!(git(remote.path(), &["rev-parse", "epic/e"]), archive, "nothing landed");

    epic_doc_note(p);
    ok(push(p, "epic/e"));
    assert_eq!(git(remote.path(), &["rev-parse", "epic/e"]), fix);
}

/// The real case of E-CRCAP's 128e3c0: a review fix after the archive commit made the archive commit
/// a walked seam, red as a standalone commit, and its notes were rewritten from a detached worktree.
#[test]
fn an_archive_commit_followed_by_a_review_fix_passes_both_walks_with_no_note_rewritten() {
    let (d, remote) = epic_with_a_merged_wu();
    let p = d.path();
    let archive = archive_commit(p);
    let notes_before = git(p, &["notes", "--ref=doc", "show", &archive]);
    let tip = review_fix(p, 1);
    review_and_sign_epic(p);

    let o = ok(keel(p, &["verify-dod", "epic", "epic/e"], ""));
    assert!(text(&o).contains("verify-dod epic green"), "{}", text(&o));
    ok(push(p, "epic/e"));
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "merge epic e", "epic/e"], ""));
    ok(push(p, "main"));
    assert_eq!(git(remote.path(), &["rev-parse", "main^2"]), tip);
    assert_eq!(git(p, &["notes", "--ref=doc", "show", &archive]), notes_before, "no note was rewritten");
}

/// The real case of E-DOCSTYLE's ad30e30, kept red by the owner's ruling: a review fix that changes
/// code stays a commit when a later fix buries it, so its epic notes are judged as a commit's.
#[test]
fn a_code_commit_carrying_epic_notes_is_still_red_as_a_commit_in_both_walks() {
    let (d, remote) = epic_with_a_merged_wu();
    let p = d.path();
    archive_commit(p);
    let first = review_fix(p, 1);
    review_fix(p, 2);
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));

    let o = keel(p, &["verify-dod", "epic", "epic/e"], "");
    red(&o, &["7·walk·2·manifest", &format!("[commit {}]", &first[..10]), "needs the unit layer"]);
    let before = git(remote.path(), &["rev-parse", "epic/e"]);
    red(&push(p, "epic/e"), &["2·manifest", &first[..10], "needs the unit layer"]);
    assert_eq!(git(remote.path(), &["rev-parse", "epic/e"]), before, "nothing landed");
}

/// What did not change: a plain commit with no notes is red as the tip and inside the range, and a
/// code commit with its own commit-level notes still passes as a commit.
#[test]
fn a_plain_commit_with_no_notes_is_still_red_and_one_with_its_own_notes_still_passes() {
    let (d, remote) = epic_with_a_merged_wu();
    let p = d.path();
    let before = git(remote.path(), &["rev-parse", "epic/e"]);
    write(p, "src/c.py", "def c():\n    return 3\n");
    let bare = commit(p, "code with no notes");
    red(&push(p, "epic/e"), &["1·notes", &bare[..10]]);

    archive_commit(p);
    red(&push(p, "epic/e"), &["1·notes", &bare[..10]]);
    review_fix(p, 1);
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    red(&keel(p, &["verify-dod", "epic", "epic/e"], ""), &["7·walk·1·notes", &format!("[commit {}]", &bare[..10])]);
    assert_eq!(git(remote.path(), &["rev-parse", "epic/e"]), before, "nothing landed");

    let (d, _remote) = epic_with_a_merged_wu();
    let p = d.path();
    write(p, "src/c.py", "def c():\n    return 3\n");
    write(p, "tests/test_c.py", "def test_c():\n    pass\n");
    let own = commit(p, "code with its own notes");
    ok(keel(p, &["notes", "write", "test", "--target", &format!("{own}^")], r#"{"unit":{"files":["tests/test_c.py"],"covers":"c returns 3"}}"#));
    archive_commit(p);
    review_fix(p, 1);
    review_and_sign_epic(p);
    ok(keel(p, &["verify-dod", "epic", "epic/e"], ""));
    ok(push(p, "epic/e"));
}
