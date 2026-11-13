//! E-SEED.W4 manual review: a throwaway ssh key signs a WU tip; the merge goes green; a tampered
//! report, an amended tip, an unlisted signer and a confirm before cr are each red.
//! E-ROUTER3.W4: the roster is `.keel/mr-allowed` as committed on the target, never a file on disk.
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

mod common;

fn run(prog: &str, d: &Path, a: &[&str]) -> Output {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    common::cmd(prog)
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
    assert_red8_on(p, "wu/e/a", needle);
}

fn assert_red8_on(p: &Path, tip: &str, needle: &str) {
    let o = keel(p, &["verify-dod", "wu", tip]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("8·manual-review") && t.contains("fix ->"), "{t}");
    assert!(t.contains(needle), "expected '{needle}': {t}");
}

/// A key-tier signer `vu` (throwaway ed25519, no passphrase) in the roster main commits, main →
/// epic/e → wu/e/a with code, a test and a tracked result report. HEAD on wu/e/a. The key lives
/// outside the repo; keel.signingkey names it.
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
    assert!(git(p, &["ls-files"]).lines().any(|f| f == ".keel/mr-allowed"), "the base commit carries the roster");
    git(p, &["switch", "-q", "-c", "epic/e"]);
    git(p, &["switch", "-q", "-c", "wu/e/a"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    pass\n");
    write(p, "docs/work/a-result.html", RESULT);
    commit(p, "wu a");
    (d, k)
}

/// Commit `roster` on epic/e, the target that judges wu/e/a, and come back. Its first parent stays
/// the fork point, so wu/e/a's base and every note on it are unchanged.
fn land_roster(p: &Path, roster: &str) {
    git(p, &["switch", "-q", "epic/e"]);
    write(p, ".keel/mr-allowed", roster);
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "the target's roster"]);
    git(p, &["switch", "-q", "wu/e/a"]);
}

fn keygen(dir: &Path, name: &str) -> (String, String) {
    ok(run("ssh-keygen", dir, &["-q", "-t", "ed25519", "-N", "", "-f", name]));
    let key = dir.join(name).to_str().unwrap().to_string();
    let public = std::fs::read_to_string(format!("{key}.pub")).unwrap();
    (key, public.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
}

/// An mr/v1 note on `tip` as `signer`, bound like a real one (base, change and report hashes) and
/// signed with `key` when given: what a session holding only its own key can write by hand.
fn forge(p: &Path, tip: &str, target: &str, signer: &str, tier: &str, key: Option<&str>) {
    use sha2::{Digest, Sha256};
    let hash = |b: &[u8]| format!("sha256:{}", Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect::<String>());
    let (tip, base) = (git(p, &["rev-parse", tip]), git(p, &["merge-base", target, tip]));
    let change = hash(git(p, &["diff", "--no-color", "--no-ext-diff", "--no-textconv", &base, &tip, "--"]).as_bytes());
    let report = "docs/work/a-result.html";
    let report_hash = hash(&run("git", p, &["show", &format!("{tip}:{report}")]).stdout);
    let mut n = json!({"schema": "mr/v1", "tip": tip, "base": base, "level": "wu", "signer": signer, "tier": tier,
        "committer": "forger", "timestamp": 0,
        "report": report, "report_hash": report_hash, "change_hash": change});
    if let Some(k) = key {
        let dir = TempDir::new().unwrap();
        let payload = format!("keel-mr/v2\nlevel: wu\nbase: {base}\ntip: {tip}\nchange: {change}\nreport: {report}\nreport_hash: {report_hash}\n");
        std::fs::write(dir.path().join("payload"), payload).unwrap();
        ok(run("ssh-keygen", dir.path(), &["-Y", "sign", "-n", "keel-mr", "-f", k, "payload"]));
        n["sig"] = json!(std::fs::read_to_string(dir.path().join("payload.sig")).unwrap());
    }
    put_note(p, &tip, &n);
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
        let mut c = common::cmd(env!("CARGO_BIN_EXE_keel"));
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

    // Signed by vu, then the target's committed roster stops listing vu: check 8 names that roster.
    ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"]));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    // vu listed, but with a different key: the signature is not vu's.
    let other = TempDir::new().unwrap();
    let (_, pubkey) = keygen(other.path(), "o");
    land_roster(p, &format!("key: vu {pubkey}\n"));
    assert_red8(p, "does not verify");
    assert_red8(p, "epic/e:.keel/mr-allowed");
    land_roster(p, "username: someone-else\n");
    assert_red8(p, "does not list");
    // Re-listed as an identity tier: listed is all that tier proves, so it is green again.
    land_roster(p, "username: vu\n");
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
}

#[test]
fn a_branch_cannot_weaken_the_policy_that_judges_its_own_merge() {
    let (d, _k) = fixture();
    let p = d.path();
    // vu holds a key, and the target also lists vu as a plain username.
    let roster = std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap();
    land_roster(p, &format!("{roster}username: vu\n"));
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

    // W13: from a linked worktree (absolute GIT_DIR), `commit -a` and a pathspec commit stage into a
    // temporary index (GIT_INDEX_FILE) the real index never sees - the hook must judge that one.
    let wt = TempDir::new().unwrap();
    let w = wt.path().join("w");
    git(p, &["worktree", "add", "-q", "-b", "side", w.to_str().unwrap(), "HEAD"]);
    write(&w, "docs/rules.html", "<p>a looser rule</p>");
    for a in [&["commit", "-a", "-qm", "loosen a rule"][..], &["commit", "-qm", "loosen a rule", "docs/rules.html"]] {
        let o = run("git", &w, a);
        assert!(!o.status.success() && text(&o).contains("docs/rules.html"), "{a:?}: {}", text(&o));
    }
}

/// W13: a tip its target already contains has an empty range; signing it would bind base = tip and
/// the empty diff over the note that attests the real change - refused, nothing written.
#[test]
fn confirm_refuses_a_tip_already_in_its_target_and_keeps_the_note() {
    let (d, _k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]));
    let signed = git(p, &["notes", "--ref=mr", "show", "wu/e/a"]);

    let o = keel(p, &["mr", "confirm", "wu", "wu/e/a", "--as", "vu", "--target", "epic/e", "--report", "docs/work/a-result.html"]);
    refused(&o, &["already contained in epic/e", "nothing to sign"]);
    assert_eq!(git(p, &["notes", "--ref=mr", "show", "wu/e/a"]), signed);

    git(p, &["branch", "epic/f", "main"]);
    let o = keel(p, &["mr", "confirm", "epic", "epic/f", "--as", "vu"]);
    refused(&o, &["already contained in main"]);
    assert!(mr_note(p, "epic/f").is_none());
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
    std::fs::write(&v1, format!("keel-mr/v1\nlevel: wu\nbase: {base}\ntip: {tip}\nchange: {change}\nreport: docs/work/a-result.html\nreport_hash: {report}\n"))
        .unwrap();
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
    land_roster(p, "username: vu\n");
    assert_red8(p, "differs from the bytes");
}

/// I5 (owner ruling 2026-11-10): an epic's report is `--report <path>` or a result doc committed in
/// the epic's own diff - never an untracked or archived file found by search.
#[test]
fn the_epic_report_is_named_or_committed_never_searched_for() {
    const EPIC: &str = "<html data-artifact=\"epic-result\"><p>epic e: done</p></html>";
    let (d, _k) = fixture();
    let p = d.path();
    notes_with(p, &["test", "doc"], r#"{"unit":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    git(p, &["rm", "-q", "docs/work/a-result.html"]);
    write(p, "docs/decisions/a.html", "<p>why a</p>");
    write(p, ".gitignore", "docs/.archive/\n");
    commit(p, "distill + archive wu a's report");
    write(p, "docs/.archive/e-result.html", EPIC);
    write(p, "docs/work/f-result.html", EPIC);
    notes_with(p, &["test", "cr"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    ok(keel(p, &["notes", "write", "doc", "--distill", "docs/work/a-result.html=docs/decisions/a.html"]));
    refused(&keel(p, &["mr", "confirm", "epic"]), &["required", "epic-result", "--report <path>"]);
    assert!(mr_note(p, "HEAD").is_none(), "an archived or untracked epic-result is never picked");

    ok(keel(p, &["mr", "confirm", "epic", "--report", "docs/.archive/e-result.html"]));
    assert_eq!(mr_note(p, "HEAD").unwrap()["report"], "docs/.archive/e-result.html");
    ok(keel(p, &["verify-dod", "epic", "wu/e/a"]));

    let (d, _k) = fixture();
    let p = d.path();
    // An epic tip tracks no T3 doc (check 6), so a committed epic report sits outside docs/work/.
    git(p, &["rm", "-q", "docs/work/a-result.html"]);
    write(p, "docs/e-result.html", EPIC);
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--amend", "-m", "wu a\n\nReviewed-By: key vu"]);
    write(p, "docs/work/f-result.html", EPIC);
    notes_with(p, &["test", "doc", "cr"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"a works"}}"#);
    ok(keel(p, &["mr", "confirm", "epic"]));
    assert_eq!(mr_note(p, "HEAD").unwrap()["report"], "docs/e-result.html");
}

/// E-ROUTER3.W4 (R14 as filed): a session rewrites its checkout's `.mr-allowed`, the working
/// `.keel/mr-allowed` and `[check] test`, then runs `keel check` to sign as the owner. The target's
/// committed roster judges: nothing is signed, and a hand-written note is red naming that roster.
#[test]
fn a_session_that_rewrites_its_checkouts_roster_and_check_command_cannot_sign_as_the_owner() {
    let (d, k) = fixture();
    let p = d.path();
    let owner = std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap();
    git(p, &["switch", "-q", "epic/e"]);
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &format!("{cfg}[mr]\ntiers = [\"key\"]\n"));
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "key-only policy"]);
    git(p, &["switch", "-q", "wu/e/a"]);
    notes(p, &["test", "doc", "cr"]);

    let w = TempDir::new().unwrap();
    let (wkey, wpub) = keygen(w.path(), "worker");
    std::fs::write(p.join(".mr-allowed"), format!("username: vu\nkey: vu {wpub}\n")).unwrap();
    write(p, ".keel/mr-allowed", &format!("key: vu {wpub}\nkey: worker {wpub}\n"));
    git(p, &["config", "keel.signingkey", &wkey]);
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &cfg.replace("test = \"true\"", "test = \"keel mr confirm wu --as vu\""));
    let o = keel(p, &["check"]);
    assert!(!o.status.success() && text(&o).contains("epic/e:.keel/mr-allowed"), "{}", text(&o));
    assert!(mr_note(p, "wu/e/a").is_none(), "a refused confirm writes nothing");
    let o = keel(p, &["mr", "confirm", "wu", "--as", "worker"]);
    assert!(!o.status.success() && text(&o).contains("worker is not listed in epic/e:.keel/mr-allowed"), "{}", text(&o));
    assert!(mr_note(p, "wu/e/a").is_none());

    forge(p, "wu/e/a", "epic/e", "vu", "key", Some(&wkey));
    assert_red8(p, "signature by vu does not verify");
    assert_red8(p, "epic/e:.keel/mr-allowed");
    forge(p, "wu/e/a", "epic/e", "worker", "key", Some(&wkey));
    assert_red8(p, "whom epic/e:.keel/mr-allowed does not list");
    forge(p, "wu/e/a", "epic/e", "vu", "username", None);
    assert_red8(p, "carries no signature");

    git(p, &["checkout", "--", "."]);
    git(p, &["config", "keel.signingkey", k.path().join("id").to_str().unwrap()]);
    assert_eq!(std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap(), owner);
    ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]));
}

/// A WU that commits its own signer is judged by the target's roster; the owner's confirm shows the
/// added line and signs only with --roster-change; once merged, the new signer counts.
#[test]
fn a_roster_change_inside_a_branch_is_judged_by_the_target_and_needs_the_signers_ack() {
    let (d, k) = fixture();
    let p = d.path();
    let owner_key = k.path().join("id").to_str().unwrap().to_string();
    let (two_key, two_pub) = keygen(k.path(), "two");
    let roster = std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap();
    write(p, ".keel/mr-allowed", &format!("{roster}key: two {two_pub}\n"));
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "add a signer\n\nReviewed-By: key vu"]);
    notes(p, &["test", "doc", "cr"]);

    git(p, &["config", "keel.signingkey", &two_key]);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "two", "--roster-change"]);
    assert!(!o.status.success() && text(&o).contains("two is not listed in epic/e:.keel/mr-allowed"), "{}", text(&o));
    forge(p, "wu/e/a", "epic/e", "two", "key", Some(&two_key));
    assert_red8(p, "whom epic/e:.keel/mr-allowed does not list");

    git(p, &["notes", "--ref=mr", "remove", "wu/e/a"]);
    git(p, &["config", "keel.signingkey", &owner_key]);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "vu"]);
    refused(&o, &[&format!("+ key: two {two_pub}"), "--roster-change"]);
    assert!(mr_note(p, "wu/e/a").is_none(), "a refused confirm writes nothing");
    ok(keel(p, &["mr", "confirm", "wu", "--as", "vu", "--roster-change"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]));

    git(p, &["switch", "-q", "-c", "wu/e/b"]);
    write(p, "src/b.py", "def b():\n    return 2\n");
    write(p, "tests/test_b.py", "def test_b():\n    pass\n");
    write(p, "docs/work/b-result.html", RESULT);
    commit(p, "wu b");
    notes_with(p, &["test", "doc", "cr"], r#"{"integration":{"files":["tests/test_b.py"],"covers":"b works"}}"#);
    git(p, &["config", "keel.signingkey", &two_key]);
    ok(keel(p, &["mr", "confirm", "wu", "--as", "two"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu b", "wu/e/b"]));
}

/// Bootstrap: a target with no committed roster is judged by the one the change carries, and keel
/// says so; with none on either side nobody may sign. A leftover `.mr-allowed` changes nothing.
#[test]
fn a_target_without_a_roster_is_judged_by_the_one_the_change_carries_and_says_so() {
    let (d, _k) = fixture();
    let p = d.path();
    let roster = std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap();
    git(p, &["switch", "-q", "epic/e"]);
    git(p, &["rm", "-q", ".keel/mr-allowed"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "no roster on the target"]);
    std::fs::write(p.join(".mr-allowed"), "username: ghost\nusername: vu\n").unwrap();

    git(p, &["switch", "-q", "-c", "wu/e/c"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    pass\n");
    write(p, "docs/work/a-result.html", RESULT);
    commit(p, "wu c, no roster");
    notes(p, &["test", "doc", "cr"]);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "vu"]);
    refused(&o, &["nobody is listed", "keel mr setup --as <identity>", "a local .mr-allowed is no longer read"]);
    forge(p, "wu/e/c", "epic/e", "vu", "username", None);
    assert_red8_on(p, "wu/e/c", "nobody is listed");

    write(p, ".keel/mr-allowed", &roster);
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "wu c carries the roster"]);
    notes(p, &["test", "doc", "cr"]);
    let bootstrap = "epic/e commits no .keel/mr-allowed - bootstrap";
    let o = keel(p, &["mr", "confirm", "wu", "--as", "vu"]);
    refused(&o, &[bootstrap, "+ key: vu", "--roster-change"]);
    let o = ok(keel(p, &["mr", "confirm", "wu", "--as", "vu", "--roster-change"]));
    assert!(text(&o).contains(bootstrap), "{}", text(&o));
    git(p, &["switch", "-q", "epic/e"]);
    let o = ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu c", "wu/e/c"]));
    assert!(text(&o).contains("HEAD commits no .keel/mr-allowed - bootstrap"), "{}", text(&o));
}

/// The commit level: HEAD's committed roster judges a rule-bearing commit, so a commit cannot name
/// itself into the roster it edits; only with no roster at HEAD does the staged one count.
#[test]
fn the_commit_hook_reads_the_roster_from_head_never_from_the_working_tree() {
    let (d, _k) = fixture();
    let p = d.path();
    let roster = std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap();
    write(p, ".keel/mr-allowed", &format!("{roster}username: ghost\n"));
    write(p, "docs/rules.html", "<p>a rule</p>");
    git(p, &["add", "docs/rules.html"]);
    let o = run("git", p, &["commit", "-qm", "loosen a rule\n\nReviewed-By: username ghost"]);
    assert!(!o.status.success() && text(&o).contains("docs/rules.html"), "a working-copy roster admits nobody: {}", text(&o));

    git(p, &["reset", "-q"]);
    git(p, &["add", ".keel/mr-allowed"]);
    let o = run("git", p, &["commit", "-qm", "add ghost"]);
    assert!(!o.status.success() && text(&o).contains(".keel/mr-allowed"), "the roster is rule-bearing whatever rule_bearing lists: {}", text(&o));
    let o = run("git", p, &["commit", "-qm", "add ghost\n\nReviewed-By: username ghost"]);
    assert!(!o.status.success(), "a staged roster edit is judged by HEAD's roster: {}", text(&o));
    ok(run("git", p, &["commit", "-qm", "add ghost\n\nReviewed-By: key vu"]));

    git(p, &["checkout", "-q", "--orphan", "fresh"]);
    git(p, &["rm", "-q", "--cached", ".keel/mr-allowed"]);
    write(p, ".keel/mr-allowed", "username: newbie\n");
    git(p, &["add", "-A"]);
    let o = run("git", p, &["commit", "-qm", "first\n\nReviewed-By: key vu"]);
    assert!(!o.status.success(), "with no roster at HEAD the staged one judges, and it lists no vu: {}", text(&o));
    let o = ok(run("git", p, &["commit", "-qm", "first\n\nReviewed-By: username newbie"]));
    assert!(text(&o).contains("bootstrap"), "{}", text(&o));
}

/// pre-push re-judges each pushed merge by the roster its own first parent committed, so a merge
/// forced in before its signer was listed stays red after the roster later lists them.
#[test]
fn pre_push_judges_each_merge_by_the_roster_of_its_own_first_parent() {
    let (d, k) = fixture();
    let p = d.path();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(p, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    ok(run("git", p, &["push", "-q", "origin", "main", "epic/e"]));

    let (two_key, two_pub) = keygen(k.path(), "two");
    notes(p, &["test", "doc", "cr"]);
    forge(p, "wu/e/a", "epic/e", "two", "key", Some(&two_key));
    git(p, &["switch", "-q", "epic/e"]);
    git(p, &["merge", "-q", "--no-ff", "--no-verify", "-m", "merge wu a, signed by a signer not yet listed", "wu/e/a"]);
    let forced = git(p, &["rev-parse", "--short=10", "wu/e/a"]);

    let roster = std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap();
    write(p, ".keel/mr-allowed", &format!("{roster}key: two {two_pub}\n"));
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "add two\n\nReviewed-By: key vu"]);

    git(p, &["switch", "-q", "-c", "wu/e/b"]);
    write(p, "src/b.py", "def b():\n    return 2\n");
    write(p, "tests/test_b.py", "def test_b():\n    pass\n");
    write(p, "docs/work/b-result.html", RESULT);
    commit(p, "wu b");
    notes_with(p, &["test", "doc", "cr"], r#"{"integration":{"files":["tests/test_b.py"],"covers":"b works"}}"#);
    git(p, &["config", "keel.signingkey", &two_key]);
    ok(keel(p, &["mr", "confirm", "wu", "--as", "two"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu b", "wu/e/b"]));
    let signed = git(p, &["rev-parse", "--short=10", "wu/e/b"]);

    let o = run("git", p, &["push", "origin", "epic/e"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("8·manual-review") && t.contains("two, whom"), "{t}");
    assert!(t.contains(&format!("red on {forced}")) && !t.contains(&format!("red on {signed}")), "{t}");
}

/// setup writes the line into the tracked roster file and names the commit that lands it; it
/// writes no local roster and no exclude entry.
#[test]
fn setup_writes_the_tracked_roster_and_touches_no_local_file() {
    let (d, k) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let p = d.path();
    let (key, public) = keygen(k.path(), "id");
    git(p, &["init", "-q", "-b", "main"]);
    write(
        p,
        ".keel/config.toml",
        "code_paths = [\"src/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n[mr]\ntiers = [\"key\"]\n",
    );
    let exclude = std::fs::read_to_string(p.join(".git/info/exclude")).unwrap_or_default();

    let o = keel(p, &["mr", "setup", "--as", "vu"]);
    assert!(!o.status.success() && text(&o).contains("tier 'username' is not accepted"), "{}", text(&o));
    let o = ok(keel(p, &["mr", "setup", "--as", "vu", "--key", &key]));
    assert!(text(&o).contains("probe signature verified"), "{}", text(&o));
    assert!(text(&o).contains("git add .keel/mr-allowed && keel mr commit"), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap(), format!("key: vu {public}\n"));
    assert!(!p.join(".mr-allowed").exists());
    assert_eq!(std::fs::read_to_string(p.join(".git/info/exclude")).unwrap_or_default(), exclude);
    assert!(git(p, &["status", "--porcelain"]).contains(".keel/"), "the roster is a file git sees, to be committed");
}

/// W4 review round 1 blocker: a roster setup cannot read is refused, never rewritten as if empty.
#[test]
fn setup_refuses_an_unreadable_roster_and_leaves_its_signers_in_place() {
    let d = TempDir::new().unwrap();
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    write(
        p,
        ".keel/config.toml",
        "code_paths = [\"src/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
    );
    let bytes = b"username: owner\n\xff\n";
    std::fs::write(p.join(".keel/mr-allowed"), bytes).unwrap();

    let o = keel(p, &["mr", "setup", "--as", "vu"]);
    assert!(!o.status.success() && text(&o).contains("cannot read .keel/mr-allowed"), "{}", text(&o));
    assert_eq!(std::fs::read(p.join(".keel/mr-allowed")).unwrap(), bytes);
}

// E-CRCAP.W1: a waived review at sign-off (UC-REV-2, UC-MR-1).

fn accept_code(o: &Output) -> Option<String> {
    let re = regex::Regex::new(r"--accept-waiver ([0-9a-f]{12})").unwrap();
    re.captures(&text(o)).map(|c| c[1].to_string())
}

/// Accept the tip's tool-written waiver: the refused run shows it with a code, the re-run gives it back.
fn accept(p: &Path, extra: &[&str]) -> Output {
    let args = [&["mr", "confirm", "wu"][..], extra].concat();
    let preview = keel(p, &args);
    let code = accept_code(&preview).unwrap_or_else(|| panic!("no accept code: {}", text(&preview)));
    ok(keel(p, &[&args[..], &["--accept-waiver", &code]].concat()))
}

const WAIVER: &str = "owner ruling 2026-11-12: review waived, the open blocker is R31";

fn cr(p: &Path, args: &[&str]) -> Output {
    keel(p, &[&["notes", "write", "cr"][..], args].concat())
}

/// A tool's waiver on HEAD: a failed round made WAIVED by hand. The writer has no reasoned form
/// (B18) and the real cap needs five rounds (tests/verify_dod.rs runs them); the sign-off reads the
/// same note either way.
fn plant_waiver(p: &Path, reason: &str) {
    ok(cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    edit_cr(p, |n| (n["verdict"], n["waiver"]) = (json!("WAIVED"), json!(reason)));
}

fn edit_cr(p: &Path, f: impl FnOnce(&mut Value)) {
    let mut n: Value = serde_json::from_str(&git(p, &["notes", "--ref=cr", "show", "HEAD"])).unwrap();
    f(&mut n);
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &n.to_string(), "HEAD"]);
}

fn sha256_of(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{}", Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect::<String>())
}

#[test]
fn a_waived_tip_is_signed_only_with_explicit_acceptance_after_the_history_is_shown() {
    let (d, _k) = fixture();
    let p = d.path();
    // Round 1 fails on the first commit; the owner waives the review of the fix.
    notes(p, &["test", "doc"]);
    ok(cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    let round1 = git(p, &["rev-parse", "HEAD"]);
    write(p, "src/a.py", "def a():\n    return 2\n");
    commit(p, "fix round 1");
    notes(p, &["test", "doc"]);
    plant_waiver(p, WAIVER);
    let tip = git(p, &["rev-parse", "HEAD"]);
    edit_cr(p, |n| {
        n["findings"] = json!([{"location": "src/a.py:2", "claim": "a() returns 2 where the contract says 1", "severity": "blocker", "evidence": "e"}])
    });

    let o = keel(p, &["mr", "confirm", "wu"]);
    refused(&o, &["WAIVED", WAIVER, "--accept-waiver", "blocker src/a.py:2: a() returns 2 where the contract says 1", &round1[..10], "FAIL", &tip[..10]]);
    assert!(mr_note(p, "HEAD").is_none(), "a refused confirm writes nothing");

    let o = accept(p, &["--as", "vu"]);
    assert!(text(&o).contains(WAIVER), "{}", text(&o));
    let m = mr_note(p, "HEAD").unwrap();
    assert_eq!(m["waiver_hash"].as_str().unwrap(), sha256_of(WAIVER.as_bytes()));
    assert!(m["sig"].as_str().unwrap().contains("BEGIN SSH SIGNATURE"));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));

    ok(keel(p, &["hooks", "install"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]));
    let dod: Value = serde_json::from_str(&git(p, &["notes", "--ref=dod", "show", "wu/e/a"])).unwrap();
    assert_eq!((dod["review"].as_str(), dod["waiver"].as_str()), (Some("WAIVED"), Some(WAIVER)), "{dod}");
}

#[test]
fn the_sign_off_covers_the_waiver_it_accepted_and_only_that() {
    let (d, _k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc"]);
    plant_waiver(p, WAIVER);
    accept(p, &[]);
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    // Reworded after signing: red. The signed wording again: green - the line is rebuilt from the note.
    edit_cr(p, |n| n["waiver"] = json!("owner ruling: waived (softer wording)"));
    assert_red8(p, "waiver");
    edit_cr(p, |n| n["waiver"] = json!(WAIVER));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    // A signed waiver whose note later says PASS: the owner accepted something no longer there.
    ok(cr(p, &["--verdict", "PASS"]));
    assert_red8(p, "waiver");
    // A signed PASS turned into a waiver by hand (the writer never waives over a PASS): the owner never saw it.
    ok(keel(p, &["mr", "confirm", "wu"]));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    let mut n: Value = serde_json::from_str(&git(p, &["notes", "--ref=cr", "show", "HEAD"])).unwrap();
    (n["verdict"], n["waiver"]) = (json!("WAIVED"), json!(WAIVER));
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &n.to_string(), "HEAD"]);
    assert_red8(p, "waiver");
    // Forged: the waiver hash copied into the PASS sign-off by hand still fails the signature.
    let mut m = mr_note(p, "HEAD").unwrap();
    m["waiver_hash"] = json!(sha256_of(WAIVER.as_bytes()));
    put_note(p, "HEAD", &m);
    assert_red8(p, "does not verify");
}

#[test]
fn a_tip_without_a_waiver_signs_byte_for_byte_as_today() {
    let (d, k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc", "cr"]);
    ok(keel(p, &["mr", "confirm", "wu", "--accept-waiver"])); // nothing to accept: ignored
    let m = mr_note(p, "HEAD").unwrap();
    assert!(m.get("waiver_hash").is_none(), "{m}");
    // The signature verifies over today's exact keel-mr/v2 payload, with no review line.
    let (base, tip) = (m["base"].as_str().unwrap(), m["tip"].as_str().unwrap());
    let payload = format!(
        "keel-mr/v2\nlevel: wu\nbase: {base}\ntip: {tip}\nchange: {}\nreport: {}\nreport_hash: {}\n",
        m["change_hash"].as_str().unwrap(),
        m["report"].as_str().unwrap(),
        m["report_hash"].as_str().unwrap()
    );
    let pubkey = std::fs::read_to_string(k.path().join("id.pub")).unwrap();
    let v = TempDir::new().unwrap();
    std::fs::write(v.path().join("signers"), format!("vu {}\n", pubkey.split_whitespace().take(2).collect::<Vec<_>>().join(" "))).unwrap();
    std::fs::write(v.path().join("sig"), m["sig"].as_str().unwrap()).unwrap();
    std::fs::write(v.path().join("payload"), &payload).unwrap();
    let sh = "ssh-keygen -Y verify -f signers -I vu -n keel-mr -s sig < payload";
    ok(run("sh", v.path(), &["-c", sh]));
    // And a note signed over today's payload by the key holder is accepted by check 8.
    forge(p, "wu/e/a", "epic/e", "vu", "key", Some(k.path().join("id").to_str().unwrap()));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
}

// E-CRCAP.W1 round 2: the owner's waiver at sign-off, key tier.

#[test]
fn an_owner_waiver_at_sign_off_is_key_signed_and_a_failed_signing_writes_neither_note() {
    let (d, _k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc"]);
    // No signing key configured: the sign-off fails before anything is written.
    let key = git(p, &["config", "keel.signingkey"]);
    git(p, &["config", "--unset", "keel.signingkey"]);
    let preview = keel(p, &["mr", "confirm", "wu", "--waive-review", WAIVER]);
    let re = regex::Regex::new(r"--confirm-waiver ([0-9a-f]{12})").unwrap();
    let code = re.captures(&text(&preview)).unwrap_or_else(|| panic!("no code: {}", text(&preview)))[1].to_string();
    let o = keel(p, &["mr", "confirm", "wu", "--waive-review", WAIVER, "--confirm-waiver", &code]);
    assert!(!o.status.success() && text(&o).contains("no signing key"), "{}", text(&o));
    assert!(mr_note(p, "HEAD").is_none() && !run("git", p, &["notes", "--ref=cr", "show", "HEAD"]).status.success(), "nothing written");
    git(p, &["config", "keel.signingkey", &key]);

    let o = ok(keel(p, &["mr", "confirm", "wu", "--waive-review", WAIVER, "--confirm-waiver", &code]));
    assert!(text(&o).contains(WAIVER), "{}", text(&o));
    let m = mr_note(p, "HEAD").unwrap();
    assert_eq!(m["waiver_hash"].as_str().unwrap(), sha256_of(WAIVER.as_bytes()));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    // The signature covers the owner's words: a session rewording them is caught.
    edit_cr(p, |n| n["waiver"] = json!("owner ruling: waived (softer wording)"));
    assert_red8(p, "waiver");
    edit_cr(p, |n| n["waiver"] = json!(WAIVER));
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    ok(keel(p, &["hooks", "install"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"]));
}

// E-CRCAP.W1 code review round 1, I1: --accept-waiver signs the waiver it showed.

#[test]
fn accept_waiver_signs_the_waiver_it_showed_and_only_with_its_code() {
    let (d, _k) = fixture();
    let p = d.path();
    notes(p, &["test", "doc"]);
    plant_waiver(p, WAIVER);
    let preview = keel(p, &["mr", "confirm", "wu"]);
    let code = accept_code(&preview).unwrap_or_else(|| panic!("no accept code: {}", text(&preview)));
    assert!(mr_note(p, "HEAD").is_none());
    for bad in [&["--accept-waiver"][..], &["--accept-waiver", "000000000000"]] {
        let o = keel(p, &[&["mr", "confirm", "wu"][..], bad].concat());
        assert!(!o.status.success() && text(&o).contains("fix ->"), "{bad:?}: {}", text(&o));
        assert!(mr_note(p, "HEAD").is_none(), "{bad:?} signed");
    }
    // The note changes between the refused run and the accepting one: the old code no longer signs.
    let reworded = "review cap: reworded after the preview";
    edit_cr(p, |n| n["waiver"] = json!(reworded));
    let stale = keel(p, &["mr", "confirm", "wu", "--accept-waiver", &code]);
    assert!(!stale.status.success() && text(&stale).contains("changed since"), "{}", text(&stale));
    assert!(mr_note(p, "HEAD").is_none());
    accept(p, &[]);
    assert_eq!(mr_note(p, "HEAD").unwrap()["waiver_hash"].as_str().unwrap(), sha256_of(reworded.as_bytes()), "signs the waiver it showed");
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
}

// E-CRCAP.W1 code review round 3: the review history is read the same whatever the user's log config.

#[test]
fn the_review_history_keeps_every_row_when_git_log_shows_signatures() {
    let (d, k) = fixture();
    let p = d.path();
    let (key, public) = keygen(k.path(), "commit-signer");
    std::fs::write(k.path().join("signers"), format!("t@t {public}\n")).unwrap();
    // The repo's own config (never the user's): signed commits, and git log showing their signatures.
    for (k_, v) in [("gpg.format", "ssh"), ("user.signingkey", key.as_str()), ("gpg.ssh.allowedSignersFile", k.path().join("signers").to_str().unwrap())] {
        git(p, &["config", k_, v]);
    }
    // Round 1 fails on a signed commit; the owner's waiver sits on the unsigned fix above it.
    write(p, "src/a.py", "def a():\n    return 3\n");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-S", "-m", "signed round 1"]);
    notes(p, &["test", "doc"]);
    ok(cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    let round1 = git(p, &["rev-parse", "HEAD"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    commit(p, "fix");
    notes(p, &["test", "doc"]);
    plant_waiver(p, WAIVER);
    let tip = git(p, &["rev-parse", "HEAD"]);
    git(p, &["config", "log.showSignature", "true"]);
    let shown = git(p, &["log", "-1", &round1]);
    assert!(shown.to_lowercase().contains("signature"), "the setting makes git log print signature lines: {shown}");
    let o = keel(p, &["mr", "confirm", "wu"]);
    let t = text(&o);
    for (sha, verdict) in [(&tip, "WAIVED"), (&round1, "FAIL")] {
        assert!(t.lines().any(|l| l.trim_start().starts_with(&format!("{} {verdict}", &sha[..10]))), "history row {} {verdict} missing: {t}", &sha[..10]);
    }
}
