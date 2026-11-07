//! E-SEED.W4 manual review: a throwaway ssh key signs a WU tip; the merge goes green; a tampered
//! report, an amended tip, an unlisted signer and a confirm before cr are each red.
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn run(prog: &str, d: &Path, a: &[&str]) -> Output {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    Command::new(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
        .env_remove("SSH_AUTH_SOCK") // never reach a real agent
        .stdin(Stdio::null())
        .output()
        .unwrap()
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
    git(d, &["commit", "-qm", msg]);
}

fn mr_note(p: &Path, rev: &str) -> Option<Value> {
    let o = run("git", p, &["notes", "--ref=mr", "show", rev]);
    o.status.success().then(|| serde_json::from_slice(&o.stdout).unwrap())
}

fn put_note(p: &Path, rev: &str, n: &Value) {
    git(p, &["notes", "--ref=mr", "add", "-f", "-m", &n.to_string(), rev]);
}

fn assert_red8(p: &Path, needle: &str) {
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("8·manual-review") && t.contains("fix ->"), "{t}");
    assert!(t.contains(needle), "expected '{needle}': {t}");
}

/// A key-tier signer `vu` (throwaway ed25519, no passphrase), main → epic/e → wu/e/a with code,
/// a test and a tracked result report. HEAD on wu/e/a. The key lives outside the repo.
fn fixture() -> (TempDir, TempDir) {
    let (d, k) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let p = d.path();
    let key = k.path().join("id");
    ok(run("ssh-keygen", k.path(), &["-q", "-t", "ed25519", "-N", "", "-C", "keel-test", "-f", key.to_str().unwrap()]));
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    write(
        p,
        ".keel/config.toml",
        "code_paths = [\"src/\", \"tests/\"]\nrule_bearing = [\"CLAUDE.md\", \"docs/*.html\"]\n[gate0]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
    );
    let o = ok(keel(p, &["mr", "setup", "--as", "vu", "--key", key.to_str().unwrap()]));
    assert!(text(&o).contains("probe signature verified"), "{}", text(&o));
    write(p, "README", "x");
    commit(p, "base");
    assert!(!git(p, &["ls-files"]).contains(".mr-allowed"), "the roster stays out of git");
    git(p, &["switch", "-q", "-c", "epic/e"]);
    git(p, &["switch", "-q", "-c", "wu/e/a"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    pass\n");
    write(p, "docs/work/a-result.html", "<p>wu a: done</p>");
    commit(p, "wu a");
    (d, k)
}

fn notes(p: &Path, kinds: &[&str]) {
    notes_with(p, kinds, r#"{"integration":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
}

fn notes_with(p: &Path, kinds: &[&str], judgment: &str) {
    for k in kinds {
        let mut c = Command::new(env!("CARGO_BIN_EXE_keel"));
        c.current_dir(p).args(["notes", "write", k]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if *k == "cr" {
            c.args(["--verdict", "PASS"]);
        }
        let mut ch = c.spawn().unwrap();
        use std::io::Write;
        ch.stdin.take().unwrap().write_all(if *k == "test" { judgment.as_bytes() } else { b"" }).unwrap();
        assert!(ch.wait_with_output().unwrap().status.success(), "notes write {k}");
    }
}

#[test]
fn key_signed_confirm_lets_the_merge_through() {
    let (d, _k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    let n = mr_note(p, "wu/e/a").unwrap();
    assert_eq!((n["schema"].as_str(), n["tier"].as_str(), n["signer"].as_str()), (Some("mr/v1"), Some("key"), Some("vu")));
    assert_eq!(n["report"], "docs/work/a-result.html");
    assert!(n["sig"].as_str().unwrap().contains("BEGIN SSH SIGNATURE"));

    ok(keel(p, &["hooks", "install"]));
    git(p, &["switch", "-q", "epic/e"]);
    // H3: git pull hides the merge head from the hook (GIT_REFLOG_ACTION="pull ..."): refused, not waved through.
    let o = run("git", p, &["pull", "--no-ff", "--no-rebase", "--no-edit", ".", "wu/e/a"]);
    assert!(!o.status.success() && text(&o).contains("cannot determine what is being merged"), "{}", text(&o));
    git(p, &["merge", "--abort"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]));
    let dod = git(p, &["notes", "--ref=dod", "show", "wu/e/a"]);
    assert!(dod.contains("manual-review"), "{dod}");
}

#[test]
fn confirm_refuses_before_cr_and_for_an_unlisted_signer() {
    let (d, _k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc"]);
    let o = keel(p, &["mr", "confirm", "wu"]);
    assert!(!o.status.success() && text(&o).contains("sign last") && text(&o).contains("1·notes"), "{}", text(&o));
    assert!(mr_note(p, "HEAD").is_none(), "a refused confirm writes nothing");

    notes(p, &["cr"]);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "ghost"]);
    assert!(!o.status.success() && text(&o).contains("ghost is not listed"), "{}", text(&o));
    assert!(mr_note(p, "HEAD").is_none());

    // Signed by vu, then this checkout's roster stops listing vu: check 8 names the roster.
    ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"]));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    // vu listed, but with a different key: the signature is not vu's.
    let other = TempDir::new().unwrap();
    ok(run("ssh-keygen", other.path(), &["-q", "-t", "ed25519", "-N", "", "-f", "o"]));
    let pubkey = std::fs::read_to_string(other.path().join("o.pub")).unwrap();
    std::fs::write(p.join(".mr-allowed"), format!("key: vu {pubkey}")).unwrap();
    assert_red8(p, "does not verify");
    std::fs::write(p.join(".mr-allowed"), "username: someone-else\n").unwrap();
    assert_red8(p, "does not list");
    // Re-listed as an identity tier: listed is all that tier proves, so it is green again.
    std::fs::write(p.join(".mr-allowed"), "username: vu\n").unwrap();
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    // A tracked roster is refused, and the fix says to delete it and rebuild, not merely untrack it.
    git(p, &["add", "-f", ".mr-allowed"]);
    assert_red8(p, "rm .mr-allowed");
    assert_red8(p, "rebuild your own roster from scratch with keel mr setup");
}

#[test]
fn a_branch_cannot_weaken_the_policy_that_judges_its_own_merge() {
    let (d, _k) = fixture();
    let p = d.path();
    // vu holds a key, and is also listed as a plain username.
    let roster = std::fs::read_to_string(p.join(".mr-allowed")).unwrap();
    std::fs::write(p.join(".mr-allowed"), format!("{roster}username: vu\n")).unwrap();
    // The WU drops `key` from the accepted tiers, so its own working tree lets vu sign off unsigned.
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &format!("{cfg}[mr]\ntiers = [\"username\"]\n"));
    commit(p, "weaken the mr policy");
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    assert!(mr_note(p, "wu/e/a").unwrap().get("sig").is_none(), "signed off on the username tier");
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"])); // green under the branch's own policy

    // The merge hook judges by the target's committed policy, where vu is key-backed.
    ok(keel(p, &["hooks", "install"]));
    git(p, &["switch", "-q", "epic/e"]);
    let o = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]);
    assert!(!o.status.success() && text(&o).contains("carries no signature"), "{}", text(&o));
}

#[test]
fn an_epic_report_edited_after_signing_is_rehashed_and_red() {
    let (d, _k) = fixture();
    let p = d.path();
    // The epic report is archived out of git by gate time: an untracked local file.
    write(p, "docs/work/e-result.html", "<p>epic e: done</p>");
    notes_with(p, &["test", "doc", "cr"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    let epic = ["verify-dod", "epic", "wu/e/a"];
    ok(keel(p, &["mr", "confirm", "epic", "--report", "docs/work/e-result.html"]));
    ok(keel(p, &epic));

    write(p, "docs/work/e-result.html", "<p>epic e: done (and something the signer never read)</p>");
    let o = keel(p, &epic);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("8·manual-review") && t.contains("differs from the bytes"), "{t}");
}

#[test]
fn tampered_report_or_amended_tip_turns_check_8_red() {
    let (d, _k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    let signed = mr_note(p, "HEAD").unwrap();

    // The note's attested report hash is forged: the signature no longer verifies.
    let mut forged = signed.clone();
    forged["report_hash"] = json!(git(p, &["rev-parse", "HEAD:README"]));
    put_note(p, "HEAD", &forged);
    assert_red8(p, "does not verify");
    // The signature is stripped: a key-backed signer's note must be signed.
    let mut bare = signed.clone();
    bare.as_object_mut().unwrap().remove("sig");
    put_note(p, "HEAD", &bare);
    assert_red8(p, "carries no signature");
    put_note(p, "HEAD", &signed);
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));

    // The report is edited and the tip amended: the new tip carries no sign-off...
    let old = git(p, &["rev-parse", "HEAD"]);
    write(p, "docs/work/a-result.html", "<p>wu a: done (and something the signer never read)</p>");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--amend", "--no-edit"]);
    notes(p, &["test", "doc", "cr"]);
    assert_red8(p, "has no mr/v1 note");
    // ...and copying the old sign-off over does not help.
    git(p, &["notes", "--ref=mr", "copy", &old, "HEAD"]);
    assert_red8(p, "copied onto an amended");
}

#[test]
fn rule_bearing_commit_needs_a_listed_reviewed_by_trailer() {
    let (d, _k) = fixture();
    let p = d.path();
    ok(keel(p, &["hooks", "install"]));
    write(p, "src/b.py", "def b():\n    return 2\n");
    git(p, &["add", "-A"]);
    ok(run("git", p, &["commit", "-qm", "plain code, no trailer needed"]));

    write(p, "docs/rules.html", "<p>a rule</p>");
    git(p, &["add", "-A"]);
    let o = run("git", p, &["commit", "-qm", "loosen a rule"]);
    assert!(!o.status.success() && text(&o).contains("docs/rules.html"), "{}", text(&o));
    let o = run("git", p, &["commit", "-qm", "loosen a rule\n\nReviewed-By: key ghost"]);
    assert!(!o.status.success(), "an unlisted trailer is refused: {}", text(&o));
    ok(keel(p, &["mr", "commit", "-m", "tighten a rule", "--as", "vu"]));
    assert!(git(p, &["log", "-1", "--format=%B"]).contains("Reviewed-By: key vu"));
}
