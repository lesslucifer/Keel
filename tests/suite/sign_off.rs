//! E-GHFLOW2.W3: a pull-request approval is the sign-off. `keel ci sign-off`, the writer's core, on a
//! scratch repo with recorded review lists as GitHub's API returns them; check 8 on the note it
//! writes; the github tier refused on every typed path; the epic report carried as a note; and R50,
//! `keel mr` checking its checkout's git links before any git call.
compiled!();

use serde_json::{json, Value};
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common;

const KEEL: &str = env!("CARGO_BIN_EXE_keel");
/// The owner's GitHub id and login, as the roster line names them.
const OWNER: u64 = 5499546;
const REPO_URL: &str = "https://github.com/own/Keel/pull/7";

fn run_with(prog: &str, d: &Path, a: &[&str], path: &str) -> Output {
    common::as_tester(common::cmd(prog).args(a).current_dir(d).env("PATH", path).env_remove("SSH_AUTH_SOCK").stdin(Stdio::null())).output().unwrap()
}

fn path() -> String {
    common::path_with(Path::new(KEEL).parent().unwrap())
}

fn run(prog: &str, d: &Path, a: &[&str]) -> Output {
    run_with(prog, d, a, &path())
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
    run(KEEL, d, a)
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

fn commit(d: &Path, msg: &str) {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "--no-verify", "-m", msg]);
}

fn note(d: &Path, kind: &str, rev: &str) -> Option<Value> {
    let o = run("git", d, &["notes", &format!("--ref={kind}"), "show", rev]);
    o.status.success().then(|| serde_json::from_slice(&o.stdout).unwrap())
}

const RESULT: &str = "<html data-artifact=\"wu-result\"><p>wu a: done</p></html>";
const EPIC_RESULT: &str = "<html data-artifact=\"epic-result\"><p>epic e: done</p></html>";
const ROSTER: &str = "username: vu\ngithub: 5499546 lesslucifer\n";

/// main (config accepting the username and github tiers, the roster above) → epic/e → wu/e/a with
/// code, a test and a result report; hooks installed; HEAD on wu/e/a, no notes yet.
fn fixture() -> TempDir {
    common::fixture("sign-off by approval", |p| {
        git(p, &["init", "-q", "-b", "main"]);
        write(
            p,
            ".keel/config.toml",
            "code_paths = [\"src/\", \"tests/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n[mr]\ntiers = [\"username\", \"github\"]\n",
        );
        write(p, ".keel/mr-allowed", ROSTER);
        write(p, "README", "x");
        commit(p, "base");
        ok(keel(p, &["hooks", "install"]));
        git(p, &["switch", "-q", "-c", "epic/e"]);
        git(p, &["switch", "-q", "-c", "wu/e/a"]);
        write(p, "src/a.py", "def a():\n    return 1\n");
        write(p, "tests/test_a.py", "def test_a():\n    pass\n");
        write(p, "docs/work/a-result.html", RESULT);
        commit(p, "wu a");
    })
}

fn notes_with(p: &Path, kinds: &[&str], judgment: &str) {
    for k in kinds {
        let mut c = common::cmd(KEEL);
        common::as_tester(c.current_dir(p).args(["notes", "write", k]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()));
        if *k == "cr" {
            c.args(["--verdict", "PASS"]);
        }
        let mut ch = c.spawn().unwrap();
        use std::io::Write;
        ch.stdin.take().unwrap().write_all(if *k == "test" { judgment.as_bytes() } else { b"" }).unwrap();
        let o = ch.wait_with_output().unwrap();
        assert!(o.status.success(), "notes write {k}: {}", text(&o));
    }
}

fn notes(p: &Path) {
    notes_with(p, &["test", "doc", "cr"], r#"{"integration":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
}

/// One review as GitHub's API lists it.
fn review(id: u64, user: u64, state: &str, commit: &str) -> Value {
    json!({"id": id, "user": {"login": "someone", "id": user, "type": "User"}, "state": state, "commit_id": commit,
        "html_url": format!("{REPO_URL}#pullrequestreview-{id}"), "body": "", "submitted_at": "2026-11-15T10:00:00Z"})
}

/// `keel ci sign-off` with `reviews` as the recorded list.
fn sign_off(p: &Path, level: &str, target: &str, reviews: &Value, extra: &[&str]) -> Output {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("reviews.json");
    std::fs::write(&f, reviews.to_string()).unwrap();
    let mut a = vec!["ci", "sign-off", level, "wu/e/a", "--target", target, "--reviews", f.to_str().unwrap()];
    a.extend_from_slice(extra);
    keel(p, &a)
}

fn red8(p: &Path, level: &str, needle: &str) {
    let o = keel(p, &["verify-dod", level, "wu/e/a"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("8·manual-review"), "{t}");
    assert!(t.contains(needle), "expected '{needle}': {t}");
}

#[test]
fn an_approval_by_a_listed_id_on_the_head_signs_and_the_gate_goes_green() {
    let d = fixture();
    let p = d.path();
    notes(p);
    let tip = git(p, &["rev-parse", "wu/e/a"]);
    let reviews = json!([review(11, 42, "APPROVED", &tip), review(12, OWNER, "COMMENTED", &tip), review(13, OWNER, "APPROVED", &tip)]);
    let o = ok(sign_off(p, "wu", "epic/e", &reviews, &[]));
    assert!(text(&o).contains("github 5499546 approved it (review 13)"), "{}", text(&o));
    let n = note(p, "mr", &tip).unwrap();
    assert_eq!((n["tier"].as_str(), n["signer"].as_str(), n["level"].as_str()), (Some("github"), Some("5499546"), Some("wu")));
    assert_eq!(n["review"], json!({"id": 13, "url": format!("{REPO_URL}#pullrequestreview-13"), "commit": tip}));
    assert_eq!(n["report"], "docs/work/a-result.html");
    assert!(n.get("sig").is_none());
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
}

#[test]
fn a_comment_an_older_commit_an_unlisted_id_or_a_foreign_page_writes_nothing() {
    let d = fixture();
    let p = d.path();
    notes(p);
    let tip = git(p, &["rev-parse", "wu/e/a"]);
    let older = git(p, &["rev-parse", "wu/e/a^"]);
    let mut foreign = review(20, OWNER, "APPROVED", &tip);
    foreign["html_url"] = json!("https://github.com/own/Keel/pull/7#pullrequestreview-21");
    let mut no_page = review(22, OWNER, "APPROVED", &tip);
    no_page["html_url"] = json!("https://example.com/own/Keel/pull/7#pullrequestreview-22");
    let mut string_id = review(23, OWNER, "APPROVED", &tip);
    string_id["user"]["id"] = json!("5499546");
    for (why, list) in [
        ("none", json!([])),
        ("a comment", json!([review(1, OWNER, "COMMENTED", &tip)])),
        ("changes requested", json!([review(2, OWNER, "CHANGES_REQUESTED", &tip)])),
        ("dismissed", json!([review(3, OWNER, "DISMISSED", &tip)])),
        ("an older commit", json!([review(4, OWNER, "APPROVED", &older)])),
        ("an unlisted id", json!([review(5, 42, "APPROVED", &tip)])),
        ("a page of another review", json!([foreign])),
        ("a page off GitHub", json!([no_page])),
        ("an id as a string", json!([string_id])),
        ("the lowercase state", json!([review(6, OWNER, "approved", &tip)])),
    ] {
        let o = sign_off(p, "wu", "epic/e", &list, &[]);
        assert_eq!(o.status.code(), Some(3), "{why}: {}", text(&o));
        assert!(text(&o).contains("no approval to sign"), "{why}: {}", text(&o));
        assert!(note(p, "mr", &tip).is_none(), "{why}: a note was written");
    }
    let o = sign_off(p, "wu", "epic/e", &json!({"message": "Not Found"}), &[]);
    assert!(!o.status.success() && text(&o).contains("not GitHub's list of reviews"), "{}", text(&o));
}

#[test]
fn the_github_tier_counts_only_when_the_targets_policy_accepts_it() {
    let d = fixture();
    let p = d.path();
    git(p, &["switch", "-q", "epic/e"]);
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap().replace("\"username\", \"github\"", "\"username\"");
    write(p, ".keel/config.toml", &cfg);
    commit(p, "the epic accepts no github tier");
    git(p, &["switch", "-q", "wu/e/a"]);
    git(p, &["merge", "-q", "--no-ff", "--no-verify", "-m", "sync", "epic/e"]);
    notes(p);
    let tip = git(p, &["rev-parse", "wu/e/a"]);
    let o = sign_off(p, "wu", "epic/e", &json!([review(13, OWNER, "APPROVED", &tip)]), &[]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(note(p, "mr", &tip).is_none());
}

#[test]
fn nothing_is_written_while_a_check_before_the_sign_off_is_red() {
    let d = fixture();
    let p = d.path();
    notes_with(p, &["test", "doc"], r#"{"integration":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    let tip = git(p, &["rev-parse", "wu/e/a"]);
    let o = sign_off(p, "wu", "epic/e", &json!([review(13, OWNER, "APPROVED", &tip)]), &[]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("sign last"), "{}", text(&o));
    assert!(note(p, "mr", &tip).is_none());
}

#[test]
fn a_roster_change_in_the_range_is_never_signed_by_approval() {
    let d = fixture();
    let p = d.path();
    write(p, ".keel/mr-allowed", &format!("{ROSTER}github: 42 intruder\n"));
    commit(p, "list another approver");
    notes(p);
    let tip = git(p, &["rev-parse", "wu/e/a"]);
    let o = sign_off(p, "wu", "epic/e", &json!([review(13, OWNER, "APPROVED", &tip)]), &[]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("changes who may sign") && text(&o).contains("--roster-change"), "{}", text(&o));
    assert!(note(p, "mr", &tip).is_none());
    let o = sign_off(p, "wu", "epic/e", &json!([review(14, 42, "APPROVED", &tip)]), &[]);
    assert_eq!(o.status.code(), Some(3), "the branch's own roster line authorises nothing: {}", text(&o));
}

#[test]
fn a_commit_after_the_approval_or_an_edited_review_turns_check_8_red() {
    let d = fixture();
    let p = d.path();
    notes(p);
    let tip = git(p, &["rev-parse", "wu/e/a"]);
    ok(sign_off(p, "wu", "epic/e", &json!([review(13, OWNER, "APPROVED", &tip)]), &[]));
    let signed = note(p, "mr", &tip).unwrap();
    let put = |n: &Value| git(p, &["notes", "--ref=mr", "add", "-f", "-m", &n.to_string(), &tip]);
    for (why, edit, needle) in [
        ("another commit", json!({"commit": git(p, &["rev-parse", "wu/e/a^"])}), "not this tip"),
        ("another review's page", json!({"url": format!("{REPO_URL}#pullrequestreview-99")}), "is not review 13's page"),
        ("a page off GitHub", json!({"url": "https://example.com/own/Keel/pull/7#pullrequestreview-13"}), "is not review 13's page"),
    ] {
        let mut n = signed.clone();
        for (k, v) in edit.as_object().unwrap() {
            n["review"][k] = v.clone();
        }
        put(&n);
        red8(p, "wu", needle);
        let _ = why;
    }
    let mut n = signed.clone();
    n.as_object_mut().unwrap().remove("review");
    put(&n);
    red8(p, "wu", "github tier only");
    let mut n = signed.clone();
    n["signer"] = json!("42");
    put(&n);
    red8(p, "wu", "does not list");
    put(&signed);
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));

    write(p, "src/a.py", "def a():\n    return 2\n");
    commit(p, "a change after the approval");
    red8(p, "wu", "has no mr/v1 note");
    let new = git(p, &["rev-parse", "wu/e/a"]);
    let mut moved = signed.clone();
    moved["tip"] = json!(new);
    git(p, &["notes", "--ref=mr", "add", "-f", "-m", &moved.to_string(), &new]);
    red8(p, "wu", "changed since the sign-off");
}

#[test]
fn a_github_only_id_cannot_sign_by_typing() {
    let d = fixture();
    let p = d.path();
    notes(p);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "5499546"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("github tier only") && t.contains("fix ->"), "{t}");
    assert!(note(p, "mr", "wu/e/a").is_none());
    write(p, ".keel/mr-allowed", &format!("{ROSTER}# touched\n"));
    git(p, &["add", "-A"]);
    let o = keel(p, &["mr", "commit", "-m", "x", "--as", "5499546"]);
    assert!(!o.status.success() && text(&o).contains("github tier only"), "{}", text(&o));
    let o = run("git", p, &["commit", "-q", "-m", "x\n\nReviewed-By: github 5499546"]);
    assert!(!o.status.success() && text(&o).contains("Reviewed-By"), "a typed trailer naming a github-only id: {}", text(&o));
    ok(keel(p, &["mr", "commit", "-m", "x", "--as", "vu"]));
}

/// The epic report rides as refs/notes/report on the epic tip: the approval attests its bytes and
/// an edit of the note afterwards turns check 8 red.
#[test]
fn an_epic_approval_binds_the_report_note_and_an_edited_note_is_red() {
    let d = fixture();
    let p = d.path();
    notes_with(p, &["test", "doc"], r#"{"unit":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    git(p, &["rm", "-q", "docs/work/a-result.html"]);
    write(p, "docs/decisions/a.html", "<p>why a</p>");
    commit(p, "distill + archive wu a's report");
    let tip = git(p, &["rev-parse", "wu/e/a"]);
    notes_with(p, &["test", "cr"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    ok(keel(p, &["notes", "write", "doc", "--distill", "docs/work/a-result.html=docs/decisions/a.html"]));
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("wu.html"), RESULT).unwrap();
    let o = keel(p, &["notes", "write", "report", dir.path().join("wu.html").to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("epic-result"), "only an epic report rides as the note: {}", text(&o));
    std::fs::write(dir.path().join("e.html"), format!("{EPIC_RESULT}\n\n  trailing  \n")).unwrap();
    ok(keel(p, &["notes", "write", "report", dir.path().join("e.html").to_str().unwrap()]));
    let stored = git(p, &["notes", "--ref=report", "list", &tip]);
    assert_eq!(run("git", p, &["cat-file", "blob", &stored]).stdout, format!("{EPIC_RESULT}\n\n  trailing  \n").as_bytes(), "the exact bytes");

    let reviews = json!([review(30, OWNER, "APPROVED", &tip)]);
    let o = sign_off(p, "epic", "main", &reviews, &[]);
    assert_eq!(o.status.code(), Some(1), "an epic with no committed report names the note: {}", text(&o));
    ok(sign_off(p, "epic", "main", &reviews, &["--report", "refs/notes/report"]));
    let n = note(p, "mr", &tip).unwrap();
    assert_eq!((n["level"].as_str(), n["report"].as_str()), (Some("epic"), Some("refs/notes/report")));
    ok(keel(p, &["verify-dod", "epic", "wu/e/a"]));

    git(p, &["notes", "--ref=report", "add", "-f", "-m", "<html data-artifact=\"epic-result\"><p>something else</p></html>", &tip]);
    red8(p, "epic", "differs from the bytes");
    git(p, &["notes", "--ref=report", "remove", &tip]);
    red8(p, "epic", "carries no refs/notes/report note");
}

/// A git on PATH that records every call and fails: `keel mr` must make none on a refused checkout.
fn recording_git(dir: &Path) -> (String, std::path::PathBuf) {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join("git-calls");
    std::fs::write(bin.join("git"), format!("#!/bin/sh\necho \"$*\" >> {}\nexit 1\n", common::sh_quote(&log.to_string_lossy()))).unwrap();
    std::fs::set_permissions(bin.join("git"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    (common::path_with(&bin), log)
}

/// R50: a linked checkout of the fixture, its admin dir tampered as a worker could, refused by
/// every `keel mr` command before any git runs; a clean one signs as before.
#[test]
fn keel_mr_refuses_a_checkout_whose_git_links_git_did_not_make_and_runs_no_git() {
    let d = fixture();
    let p = d.path();
    notes(p);
    git(p, &["switch", "-q", "main"]);
    let wt = d.path().join("wt");
    git(p, &["worktree", "add", "-q", wt.to_str().unwrap(), "wu/e/a"]);
    let admin = std::path::PathBuf::from(git(&wt, &["rev-parse", "--absolute-git-dir"]));
    let common_dir = p.join(".git");
    let sub = wt.join("src");
    let scratch = TempDir::new().unwrap();
    let (fake, log) = recording_git(scratch.path());
    type Tamper = Box<dyn Fn()>;
    type Undo = Box<dyn Fn()>;
    let file = |f: std::path::PathBuf, body: &'static str| -> (Tamper, Undo) {
        let g = f.clone();
        (Box::new(move || std::fs::write(&f, body).unwrap()), Box::new(move || std::fs::remove_file(&g).unwrap()))
    };
    let swap = |f: std::path::PathBuf, body: String| -> (Tamper, Undo) {
        let old = std::fs::read_to_string(&f).unwrap();
        let g = f.clone();
        (Box::new(move || std::fs::write(&f, &body).unwrap()), Box::new(move || std::fs::write(&g, &old).unwrap()))
    };
    let cases: Vec<(&str, (Tamper, Undo), &str)> = vec![
        ("config.worktree in the admin dir", file(admin.join("config.worktree"), "[core]\n\thooksPath = /tmp/evil\n"), "config.worktree exists"),
        ("config.worktree in the git dir", file(common_dir.join("config.worktree"), "[core]\n"), "config.worktree exists"),
        ("a commondir in the main git dir", file(common_dir.join("commondir"), "../elsewhere\n"), "commondir exists"),
        ("a wrong commondir", swap(admin.join("commondir"), "../../../other/.git\n".into()), "commondir is not ../.."),
        ("a gitdir naming another checkout", swap(admin.join("gitdir"), format!("{}\n", p.join("x/.git").display())), "gitdir does not name"),
        ("a .git naming a dir outside worktrees/", swap(wt.join(".git"), format!("gitdir: {}\n", common_dir.display())), "not under <git dir>/worktrees/"),
        ("a .git that is no link", swap(wt.join(".git"), "nothing\n".into()), "not a gitdir link"),
    ];
    for (why, (tamper, undo), needle) in cases {
        tamper();
        for (dir, a) in [(&wt, &["mr", "confirm", "wu"][..]), (&sub, &["mr", "commit", "-m", "x", "--as", "vu"][..]), (&wt, &["mr", "setup", "--as", "x"][..])]
        {
            let _ = std::fs::remove_file(&log);
            let o = run_with(KEEL, dir, a, &fake);
            let t = text(&o);
            assert!(!o.status.success() && t.contains("keel mr: refused") && t.contains(needle), "{why} {a:?}: {t}");
            assert!(!log.exists(), "{why} {a:?}: git ran: {}", std::fs::read_to_string(&log).unwrap_or_default());
        }
        undo();
    }
    // the main checkout's own links, tampered the same way, are refused too
    let (tamper, undo) = file(common_dir.join("config.worktree"), "[core]\n");
    tamper();
    let o = run_with(KEEL, p, &["mr", "commit", "-m", "x"], &fake);
    assert!(!o.status.success() && text(&o).contains("config.worktree exists") && !log.exists(), "{}", text(&o));
    undo();
    // untouched, the linked checkout signs as before (with the real git)
    let o = run(KEEL, &wt, &["mr", "confirm", "wu", "--as", "vu"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(note(p, "mr", "wu/e/a").unwrap()["tier"], "username");
}
