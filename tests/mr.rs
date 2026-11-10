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
        "code_paths = [\"src/\", \"tests/\"]\nrule_bearing = [\"CLAUDE.md\", \"docs/*.html\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
    );
    let o = ok(keel(p, &["mr", "setup", "--as", "vu", "--key", key.to_str().unwrap()]));
    assert!(text(&o).contains("probe signature verified"), "{}", text(&o));
    write(p, "README", "x");
    commit(p, "base");
    ok(keel(p, &["hooks", "install"])); // after the base commit: main is merge-only from here
    assert!(!git(p, &["ls-files"]).contains(".mr-allowed"), "the roster stays out of git");
    git(p, &["switch", "-q", "-c", "epic/e"]);
    git(p, &["switch", "-q", "-c", "wu/e/a"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    pass\n");
    write(p, "docs/work/a-result.html", RESULT);
    commit(p, "wu a");
    (d, k)
}

const RESULT: &str = "<html data-artifact=\"wu-result\"><p>wu a: done</p></html>";

/// Commit `[mr] report = "<policy>"` on the current branch.
fn set_policy(p: &Path, policy: &str) {
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &format!("{cfg}[mr]\nreport = \"{policy}\"\n"));
    commit(p, &format!("report policy {policy}"));
}

/// The WU as if it never wrote a result report (amended out, so no T3 archive lineage is owed).
fn drop_fixture_report(p: &Path) {
    git(p, &["rm", "-q", "docs/work/a-result.html"]);
    git(p, &["commit", "-q", "--amend", "--no-edit"]);
}

fn refused(o: &Output, needles: &[&str]) {
    let t = text(o);
    assert!(!o.status.success() && t.contains("fix ->"), "expected a refusal with a fix line: {t}");
    for n in needles {
        assert!(t.contains(n), "expected '{n}': {t}");
    }
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
    // The walk sees "wu a" as a direct commit below the tip: commit-level notes.
    notes_with(p, &["test", "doc"], r#"{"unit":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    // Epic close (W8, UC-DOC-3): no T3 doc tracked at the tip, so the WU report is distilled + archived...
    git(p, &["rm", "-q", "docs/work/a-result.html"]);
    write(p, "docs/decisions/a.html", "<p>why a</p>");
    commit(p, "distill + archive wu a's report");
    // ...and the epic report is archived out of git by gate time: an untracked local file.
    write(p, "docs/work/e-result.html", "<p>epic e: done</p>");
    notes_with(p, &["test", "cr"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    ok(keel(p, &["notes", "write", "doc", "--distill", "docs/work/a-result.html=docs/decisions/a.html"]));
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

    // A plain listing quotes a non-ASCII path; the hook must still see it as docs/*.html.
    write(p, "docs/règle.html", "<p>a rule</p>");
    git(p, &["add", "-A"]);
    let o = run("git", p, &["commit", "-qm", "loosen a rule"]);
    assert!(!o.status.success() && text(&o).contains("docs/règle.html"), "{}", text(&o));
}

/// I6: sign-off hashes are SHA-256; a pre-W2 note (keel-mr/v1 payload, git blob-id hashes) has no
/// legacy reader, so it is red at verify-dod wu and at the WU merge hook, naming the old format as the cause.
#[test]
fn sign_off_hashes_are_sha256_and_a_pre_w2_note_is_red() {
    let (d, k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    let mut n = mr_note(p, "HEAD").unwrap();
    let sha256 = regex::Regex::new(r"^sha256:[0-9a-f]{64}$").unwrap();
    for f in ["change_hash", "report_hash"] {
        assert!(sha256.is_match(n[f].as_str().unwrap()), "{f}: {n}");
    }

    let (base, tip) = (n["base"].as_str().unwrap().to_string(), git(p, &["rev-parse", "HEAD"]));
    let diff = k.path().join("diff");
    std::fs::write(&diff, git(p, &["diff", "--no-color", "--no-ext-diff", "--no-textconv", &base, &tip, "--"])).unwrap();
    let change = git(p, &["hash-object", diff.to_str().unwrap()]);
    let report = git(p, &["rev-parse", "HEAD:docs/work/a-result.html"]);
    let v1 = k.path().join("payload");
    std::fs::write(&v1, format!("keel-mr/v1\nlevel: wu\nbase: {base}\ntip: {tip}\nchange: {change}\nreport: docs/work/a-result.html\nreport_hash: {report}\n")).unwrap();
    ok(run("ssh-keygen", k.path(), &["-Y", "sign", "-n", "keel-mr", "-f", "id", "payload"]));
    n["change_hash"] = json!(change);
    n["report_hash"] = json!(report);
    n["sig"] = json!(std::fs::read_to_string(k.path().join("payload.sig")).unwrap());
    put_note(p, "HEAD", &n);
    assert_red8(p, "signed by a keel older than the SHA-256 sign-off format");
    assert!(!text(&keel(p, &["verify-dod", "wu", "wu/e/a"])).contains("changed since the sign-off"));

    ok(keel(p, &["hooks", "install"]));
    git(p, &["switch", "-q", "epic/e"]);
    let o = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]);
    assert!(!o.status.success() && text(&o).contains("8·manual-review"), "{}", text(&o));
}

/// I7: the dod/v1 receipt of a green WU merge names the report its sign-off attested, if any.
#[test]
fn the_dod_receipt_names_the_attested_report() {
    for policy in ["required", "none"] {
        let (d, _k) = fixture();
        let p = d.path();
        if policy == "none" {
            git(p, &["switch", "-q", "epic/e"]);
            set_policy(p, policy);
            git(p, &["switch", "-q", "wu/e/a"]);
            git(p, &["merge", "-q", "--no-ff", "--no-edit", "epic/e"]);
        }
        notes(p, &["test", "doc", "cr"]);
        ok(keel(p, &["mr", "confirm", "wu"]));
        let mr = mr_note(p, "wu/e/a").unwrap();
        ok(keel(p, &["hooks", "install"]));
        git(p, &["switch", "-q", "epic/e"]);
        ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]));
        let dod: Value = serde_json::from_str(&git(p, &["notes", "--ref=dod", "show", "wu/e/a"])).unwrap();
        for f in ["report", "report_hash"] {
            assert_eq!(dod.get(f), mr.get(f), "{policy}: {f} in dod {dod} vs mr {mr}");
        }
        assert_eq!(dod.get("report").is_some(), policy == "required", "{policy}: {dod}");
    }
}

/// I1: `[mr] report` × a wu-result doc present / absent, at WU level.
#[test]
fn report_policy_matrix_at_wu_level() {
    for policy in ["required", "optional", "none"] {
        for present in [true, false] {
            let case = format!("{policy}, report {}", if present { "present" } else { "absent" });
            let (d, _k) = fixture();
            let p = d.path();
            if !present {
                drop_fixture_report(p);
            }
            set_policy(p, policy);
            notes(p, &["test", "doc", "cr"]);
            if policy == "none" && present {
                let o = keel(p, &["mr", "confirm", "wu", "--report", "docs/work/a-result.html"]);
                refused(&o, &["report = \"none\""]);
                assert!(mr_note(p, "HEAD").is_none(), "{case}: a refused confirm writes nothing");
            }
            let o = keel(p, &["mr", "confirm", "wu"]);
            if policy == "required" && !present {
                refused(&o, &["wu-result", "report = \"required\""]);
                assert!(mr_note(p, "HEAD").is_none(), "{case}: a refused confirm writes nothing");
                continue;
            }
            assert!(o.status.success(), "{case}: {}", text(&o));
            let n = mr_note(p, "HEAD").unwrap();
            let attests = present && policy != "none";
            assert_eq!(n.get("report").is_some(), attests, "{case}: {n}");
            assert_eq!(n.get("report_hash").is_some(), attests, "{case}: {n}");
            let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
            assert!(o.status.success(), "{case}: {}", text(&o));
        }
    }
}

/// I2: the report is the doc whose data-artifact carries the level's role, whatever its file name.
#[test]
fn the_wu_report_is_found_by_role_not_by_file_name() {
    let (d, _k) = fixture();
    let p = d.path();
    drop_fixture_report(p);
    write(p, "docs/work/outcome.html", RESULT);
    write(p, "docs/work/x-result.html", "<p>an ungoverned doc named like a report</p>");
    commit(p, "a report by role");
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    assert_eq!(mr_note(p, "HEAD").unwrap()["report"], "docs/work/outcome.html");

    write(p, "docs/work/second.html", RESULT);
    commit(p, "a second wu-result");
    notes(p, &["test", "doc", "cr"]);
    let o = keel(p, &["mr", "confirm", "wu"]);
    refused(&o, &["docs/work/outcome.html", "docs/work/second.html", "--report"]);
    assert!(mr_note(p, "HEAD").is_none());
    ok(keel(p, &["mr", "confirm", "wu", "--report", "docs/work/second.html"]));
    assert_eq!(mr_note(p, "HEAD").unwrap()["report"], "docs/work/second.html");

    let (d, _k) = fixture();
    let p = d.path();
    write(p, "docs/work/a-result.html", "<html data-artifact=\"epic-result wu-result\"><p>one-WU epic</p></html>");
    commit(p, "collapsed result doc");
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    assert_eq!(mr_note(p, "HEAD").unwrap()["report"], "docs/work/a-result.html");
}

/// I3: the merge hook judges the sign-off by the target's committed policy, not the WU's own.
#[test]
fn a_wu_cannot_drop_the_report_requirement_that_judges_its_merge() {
    let (d, _k) = fixture();
    let p = d.path();
    set_policy(p, "none");
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    assert!(mr_note(p, "HEAD").unwrap().get("report").is_none());
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));

    ok(keel(p, &["hooks", "install"]));
    git(p, &["switch", "-q", "epic/e"]);
    let o = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("8·manual-review") && t.contains("attests no result report"), "{t}");
    assert!(t.contains("fix ->"), "{t}");
}

/// I4: under "optional" an attested report is still bound: a forged hash is red.
#[test]
fn a_forged_report_hash_is_red_under_optional() {
    let (d, _k) = fixture();
    let p = d.path();
    set_policy(p, "optional");
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    let mut forged = mr_note(p, "HEAD").unwrap();
    forged["report_hash"] = json!(git(p, &["rev-parse", "HEAD:README"]));
    put_note(p, "HEAD", &forged);
    assert_red8(p, "does not verify");
    // An identity-tier roster takes the signature out of play: the re-hash alone catches it.
    std::fs::write(p.join(".mr-allowed"), "username: vu\n").unwrap();
    assert_red8(p, "differs from the bytes");
}

/// I5: the epic report is archived out of git (docs/.archive/ is ignored): found untracked by role.
#[test]
fn the_epic_report_is_found_untracked_in_the_ignored_archive() {
    let (d, _k) = fixture();
    let p = d.path();
    notes_with(p, &["test", "doc"], r#"{"unit":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    git(p, &["rm", "-q", "docs/work/a-result.html"]);
    write(p, "docs/decisions/a.html", "<p>why a</p>");
    write(p, ".gitignore", "docs/.archive/\n");
    commit(p, "distill + archive wu a's report");
    write(p, "docs/.archive/e-result.html", "<html data-artifact=\"epic-result\"><p>epic e: done</p></html>");
    notes_with(p, &["test", "cr"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    ok(keel(p, &["notes", "write", "doc", "--distill", "docs/work/a-result.html=docs/decisions/a.html"]));
    ok(keel(p, &["mr", "confirm", "epic"]));
    assert_eq!(mr_note(p, "HEAD").unwrap()["report"], "docs/.archive/e-result.html");
    ok(keel(p, &["verify-dod", "epic", "wu/e/a"]));

    write(p, "docs/.archive/f-result.html", "<html data-artifact=\"epic-result wu-result\"><p>epic f</p></html>");
    git(p, &["notes", "--ref=mr", "remove", "HEAD"]);
    let o = keel(p, &["mr", "confirm", "epic"]);
    refused(&o, &["docs/.archive/e-result.html", "docs/.archive/f-result.html", "--report"]);
    assert!(mr_note(p, "HEAD").is_none());
}
