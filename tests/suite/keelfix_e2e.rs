//! E-KEELFIX epic e2e: a commit follows the committed rules, a gate run or a help request writes
//! nothing unless asked, and a roster line of a tier keel does not know is refused by its line
//! number. One scratch project, Keel's hooks installed, no network: a worker's uncommitted edits to
//! its folder's rules (lint skipped, signer tiers widened) change nothing; the WU is noted,
//! reviewed, dry-run, asked for help, signed by typing and merged into its epic through the real
//! merge hook; the epic is closed and merged into main through the same hook; then a GitHub line
//! in the roster is refused by every reader, naming its line.
compiled!();

use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    common::path_with(bin)
}

fn run(prog: &str, d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut c = common::cmd(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", path_with_keel())
        .env_remove("SSH_AUTH_SOCK")
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

fn write(d: &Path, rel: &str, body: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, body).unwrap();
}

fn has_note(p: &Path, kind: &str, rev: &str) -> bool {
    run("git", p, &["notes", &format!("--ref={kind}"), "show", rev], "").status.success()
}

/// What a by-hand run could change: every ref (notes included), `HEAD`, the index and the folder.
fn state(p: &Path) -> String {
    [git(p, &["for-each-ref"]), git(p, &["rev-parse", "HEAD"]), git(p, &["ls-files", "-s"]), git(p, &["status", "--porcelain", "-uall"])].join("\n--\n")
}

/// The lint main commits: red while any file under `src/` holds the marker.
const LINT: &str = "! grep -rqs KEELFIX_MARK src/";
/// The committed rules: that lint, one rule file, the username and key signer tiers.
const RULES: &str = "code_paths = [\"src/\", \"tests/\"]\nrule_bearing = [\"docs/rules.html\"]\n[check]\nlint = \"! grep -rqs KEELFIX_MARK src/\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n[mr]\ntiers = [\"username\", \"key\"]\n";
/// The committed roster: `t` under the username tier, `t@t` under the email tier the rules leave out.
const ROSTER: &str = "username: t\nemail: t@t\n";
const WU_PLAN: &str = "docs/work/2026-11-20-e2e-wu-a-plan.html";
const WU_RESULT: &str = "docs/work/2026-11-20-e2e-wu-a-result.html";
const EPIC_RESULT: &str = "docs/.archive/2026-11-20-e2e-epic-result.html";

/// The rules and roster committed on main, Keel's hooks installed after, so main stays merge-only.
fn project() -> TempDir {
    let d = TempDir::new().unwrap();
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    write(p, ".keel/config.toml", RULES);
    write(p, ".keel/mr-allowed", ROSTER);
    write(p, ".gitignore", "docs/.archive/\n");
    write(p, "docs/rules.html", "<p>the rules</p>");
    write(p, "README", "a project\n");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "base: the rules, the roster, the rule file"]);
    ok(keel(p, &["hooks", "install"], ""));
    d
}

/// The folder's `.keel/config.toml` with one exact `from` -> `to`, never staged.
fn edit_folder_rules(p: &Path, from: &str, to: &str) {
    let f = p.join(".keel/config.toml");
    let body = std::fs::read_to_string(&f).unwrap();
    assert!(body.contains(from), "{from} not in {body}");
    std::fs::write(f, body.replace(from, to)).unwrap();
}

#[test]
fn keelfix_commits_follow_the_committed_rules_and_nothing_is_written_unless_asked() {
    let d = project();
    let p = d.path();
    assert!(RULES.contains(LINT));
    git(p, &["switch", "-q", "-c", "epic/e"]);
    git(p, &["switch", "-q", "-c", "wu/e/a"]);

    // ── 1. the folder's rules skip lint, not committed: the committed lint still refuses ───────
    edit_folder_rules(p, &format!("lint = \"{LINT}\""), "lint = \"true\"");
    write(p, "src/a.py", "def a():\n    return 1  # KEELFIX_MARK\n");
    write(p, "tests/test_a.py", "def test_a():\n    assert 1\n");
    git(p, &["add", "src", "tests"]);
    let base = git(p, &["rev-parse", "HEAD"]);
    let o = run("git", p, &["commit", "-qm", "feat(e2e.W1): a returns one"], "");
    assert!(!o.status.success() && text(&o).contains("keel lint: red"), "the committed lint judges the commit: {}", text(&o));
    assert_eq!(git(p, &["rev-parse", "HEAD"]), base, "the refused commit records nothing");
    write(p, "src/a.py", "def a():\n    return 1\n");
    git(p, &["add", "src"]);
    ok(run("git", p, &["commit", "-qm", "feat(e2e.W1): a returns one"], ""));

    // ── 2. the folder's tiers widened, not committed: only a committed signer signs ───────────
    edit_folder_rules(p, "tiers = [\"username\", \"key\"]", "tiers = [\"email\", \"username\", \"key\"]");
    write(p, "docs/rules.html", "<p>the rules, amended</p>");
    git(p, &["add", "docs/rules.html"]);
    let head = git(p, &["rev-parse", "HEAD"]);
    // keel mr commit's own check refuses, by HEAD's tiers, before any git commit runs.
    let o = keel(p, &["mr", "commit", "-m", "docs: amend the rules", "--as", "t@t"], "");
    let (out, refusal) = (text(&o), "keel mr: refused - t@t is not listed in HEAD:.keel/mr-allowed");
    assert!(!o.status.success() && out.contains(refusal) && !out.contains("commit refused"), "only the folder's tiers accept email t@t: {out}");
    assert_eq!(git(p, &["rev-parse", "HEAD"]), head, "nothing committed");
    // The commit-msg hook judges a typed trailer by HEAD's tiers too, the folder's still widened.
    let o = run("git", p, &["commit", "-qm", "docs: amend the rules\n\nReviewed-By: email t@t"], "");
    let (out, refusal) = (text(&o), "keel: commit refused - it touches rule-bearing docs/rules.html with no Reviewed-By trailer from a listed signer");
    assert!(!o.status.success() && out.contains(refusal), "the commit-msg hook: only the folder's tiers accept email t@t: {out}");
    assert_eq!(git(p, &["rev-parse", "HEAD"]), head, "nothing committed");
    ok(keel(p, &["mr", "commit", "-m", "docs: amend the rules", "--as", "t"], ""));
    assert!(git(p, &["log", "-1", "--format=%B"]).contains("Reviewed-By: username t"));
    write(p, ".keel/config.toml", RULES);
    assert_eq!(git(p, &["status", "--porcelain"]), "", "the folder's rules back to the committed ones");

    // ── 3. notes, review; a dry gate run and help write nothing ──────────────────────────────
    common::doc_pair(p, WU_PLAN, "wu-plan", "<html><p>plan</p></html>");
    common::doc_pair(p, WU_RESULT, "wu-result", "<html><p>wu a: done</p></html>");
    git(p, &["add", "docs"]);
    ok(run("git", p, &["commit", "-qm", "docs(e2e.W1): plan and result"], ""));
    let wu = git(p, &["rev-parse", "HEAD"]);
    ok(keel(p, &["notes", "write", "test"], r#"{"integration":{"files":["tests/test_a.py"],"covers":"a returns one"}}"#));
    ok(keel(p, &["notes", "write", "doc"], ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    let before = state(p);
    let o = keel(p, &["verify-dod", "wu", "wu/e/a", "--no-receipt"], "");
    let out = text(&o);
    assert!(!o.status.success() && out.contains("FAIL  8·manual-review"), "red on the sign-off: {out}");
    assert!(!out.contains("FAIL  1·") && !out.contains("FAIL  6·"), "red only on the sign-off: {out}");
    for (args, usage) in
        [(&["notes", "write", "doc", "--help"][..], "usage: keel notes write doc"), (&["mr", "confirm", "wu", "--as", "t", "--help"], "usage: keel mr confirm")]
    {
        let o = keel(p, args, "");
        assert!(o.status.success() && String::from_utf8_lossy(&o.stdout).starts_with(usage), "{args:?}: {}", text(&o));
    }
    // The red run stops before its receipt branch; this guards the two help requests (the green
    // --no-receipt run is step 4's).
    assert_eq!(state(p), before, "a red gate run and two help requests wrote nothing");
    assert!(!has_note(p, "mr", &wu) && !has_note(p, "dod", &wu));

    // ── 4. the owner signs; the receipt only when asked; the merges go through the hooks ──────
    ok(keel(p, &["mr", "confirm", "wu", "--as", "t"], ""));
    assert!(git(p, &["notes", "--ref=mr", "show", &wu]).contains("\"tier\":\"username\""));
    let notes = git(p, &["for-each-ref", "refs/notes"]);
    let o = ok(keel(p, &["verify-dod", "wu", "wu/e/a", "--no-receipt"], ""));
    assert!(String::from_utf8_lossy(&o.stdout).ends_with("no receipt written (--no-receipt)\n"), "{}", text(&o));
    assert_eq!(git(p, &["for-each-ref", "refs/notes"]), notes, "--no-receipt writes no note");
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"], ""));
    assert!(has_note(p, "dod", &wu), "the gate without the flag writes its receipt");
    git(p, &["switch", "-q", "epic/e"]);
    let o = ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge E2E.W1: a returns one", "wu/e/a"], ""));
    assert!(text(&o).contains(&format!("verify-dod wu green on {}", &wu[..10])), "the merge hook ran the wu gate: {}", text(&o));

    let data = |page: &str| page.replace(".html", ".yaml");
    git(p, &["rm", "-q", WU_PLAN, &data(WU_PLAN), WU_RESULT, &data(WU_RESULT)]);
    write(p, "docs/decisions/close.html", "<p>what the epic taught</p>");
    common::doc_pair(p, EPIC_RESULT, "epic-result", "<html><p>epic e: done</p></html>");
    git(p, &["add", "docs/decisions/close.html"]);
    ok(run("git", p, &["commit", "-qm", "docs(E2E close): archive the working docs"], ""));
    ok(keel(
        p,
        &["notes", "write", "test"],
        r#"{"e2e":{"files":["tests/test_a.py"],"covers":"the committed rules, the dry run, help, the sign-off and both merges"}}"#,
    ));
    let distill: Vec<String> = [WU_PLAN, WU_RESULT].iter().map(|t| format!("{t}=docs/decisions/close.html")).collect();
    ok(keel(p, &["notes", "write", "doc", "--distill", &distill[0], "--distill", &distill[1]], ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    ok(keel(p, &["mr", "confirm", "epic", "--as", "t", "--report", EPIC_RESULT], ""));
    let tip = git(p, &["rev-parse", "HEAD"]);
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge epic E2E", "epic/e"], ""));
    assert_eq!(git(p, &["rev-parse", "main^2"]), tip, "main's second parent is the signed epic tip");
    assert!(has_note(p, "dod", &tip), "the merge hook's epic gate wrote the receipt");

    // ── 5. a GitHub line in the roster is refused by its line number ──────────────────────────
    git(p, &["switch", "-q", "-c", "epic/f"]);
    let roster = "username: t\ngithub: 5499546 lesslucifer\n";
    write(p, ".keel/mr-allowed", roster);
    let o = keel(p, &["mr", "setup", "--as", "other"], "");
    assert!(!o.status.success() && text(&o).contains(".keel/mr-allowed line 2"), "setup reads the working roster: {}", text(&o));
    assert_eq!(std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap(), roster, "setup wrote nothing");
    git(p, &["add", ".keel/mr-allowed"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "a hand edit of the roster, past the hooks"]);
    let head = git(p, &["rev-parse", "HEAD"]);
    write(p, "docs/rules.html", "<p>the rules, amended again</p>");
    git(p, &["add", "docs/rules.html"]);
    // mr commit's own roster check refuses before git commit runs: no hook's refusal in its output.
    let o = keel(p, &["mr", "commit", "-m", "docs: amend the rules", "--as", "t"], "");
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(!o.status.success() && err.starts_with("keel: HEAD:.keel/mr-allowed line 2") && !err.contains("commit refused"), "keel mr commit: {}", text(&o));
    let o = run("git", p, &["commit", "-qm", "docs: amend the rules\n\nReviewed-By: username t"], "");
    assert!(!o.status.success() && text(&o).contains("HEAD:.keel/mr-allowed line 2"), "the commit-msg hook: {}", text(&o));
    assert_eq!(git(p, &["rev-parse", "HEAD"]), head, "no commit written");
}
