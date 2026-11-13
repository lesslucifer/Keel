//! E-SEED.W3 merge gate: one red fixture per check, a green path, the epic walk, the hooks.
use serde_json::{json, Value};
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
        // No global or system git config reaches a fixture (a signing key, log.showSignature, hooks...).
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

fn keel(d: &Path, a: &[&str]) -> Output {
    run(env!("CARGO_BIN_EXE_keel"), d, a, "")
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

const WU_TESTS: &str = r#"{"integration":{"files":["tests/test_a.py"],"covers":"a works"}}"#;
const EPIC_TESTS: &str = r#"{"e2e":{"waived":"no e2e harness in the fixture"}}"#;

/// main (one commit) → epic/e → wu/e/a adding code, a python test and a doc. HEAD on wu/e/a.
fn fixture() -> TempDir {
    fixture_with("", "", "epic/e", "wu/e/a")
}

/// `fixture` with extra top-level and `[branches]` config lines and its own branch names.
fn fixture_with(top: &str, branches: &str, epic: &str, wu: &str) -> TempDir {
    let d = TempDir::new().unwrap();
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    write(
        p,
        ".keel/config.toml",
        &format!("{top}code_paths = [\"src/\", \"tests/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n{branches}[docs]\nroot = \"docs/\"\n"),
    );
    // An identity-tier signer in the roster the base commit carries, so green paths can sign off (check 8).
    assert!(keel(p, &["mr", "setup", "--as", "t"]).status.success());
    write(p, "README", "x");
    commit(p, "base");
    assert!(keel(p, &["hooks", "install"]).status.success()); // after the base commit: main is merge-only from here
    git(p, &["switch", "-q", "-c", epic]);
    git(p, &["switch", "-q", "-c", wu]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    pass\n");
    write(p, "docs/a.html", "<p>a</p>");
    commit(p, "wu a");
    d
}

/// Write the three notes on HEAD, as the workflow does before a merge.
fn bless(p: &Path, tests: &str, verdict: &str) {
    for (args, input) in [
        (&["notes", "write", "test"][..], tests),
        (&["notes", "write", "doc"][..], ""),
        (&["notes", "write", "cr", "--verdict", verdict][..], ""),
    ] {
        let o = run(env!("CARGO_BIN_EXE_keel"), p, args, input);
        assert!(o.status.success(), "{args:?}: {}", text(&o));
    }
}

/// The human sign-off on HEAD, signed last (after bless).
fn sign(p: &Path, level: &str, report: &str) {
    let o = keel(p, &["mr", "confirm", level, "--as", "t", "--report", report]);
    assert!(o.status.success(), "mr confirm: {}", text(&o));
}

fn note(p: &Path, kind: &str, rev: &str) -> Value {
    serde_json::from_str(&git(p, &["notes", &format!("--ref={kind}"), "show", rev])).unwrap()
}

fn edit_note(p: &Path, kind: &str, rev: &str, f: impl FnOnce(&mut Value)) {
    let mut n = note(p, kind, rev);
    f(&mut n);
    git(p, &["notes", &format!("--ref={kind}"), "add", "-f", "-m", &n.to_string(), rev]);
}

fn assert_red(o: &Output, check: &str) {
    let t = text(o);
    assert!(!o.status.success(), "expected red on {check}: {t}");
    assert!(t.contains(check) && t.contains("fix ->"), "expected {check} with a fix line: {t}");
}

#[test]
fn green_wu_writes_a_parsable_receipt_and_preflight_is_green() {
    let d = fixture();
    let p = d.path();
    bless(p, WU_TESTS, "PASS");
    let pf = keel(p, &["preflight", "wu", "wu/e/a"]);
    assert!(pf.status.success(), "{}", text(&pf));
    sign(p, "wu", "docs/a.html");
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    assert!(o.status.success(), "{}", text(&o));
    let dod = note(p, "dod", "wu/e/a");
    assert_eq!(dod["schema"], "dod/v1");
    assert_eq!(dod["level"], "wu");
    assert_eq!(dod["tip"].as_str().unwrap(), git(p, &["rev-parse", "wu/e/a"]));
    assert!(dod["checks"].as_array().unwrap().contains(&json!("diff-binding")));
    assert!(dod["checks"].as_array().unwrap().contains(&json!("manual-review")));
}

type Tamper = fn(&Path);

#[test]
fn each_missing_or_tampered_piece_of_evidence_is_red_with_its_check_id() {
    let cases: &[(&str, Tamper)] = &[
        ("1·notes", |p| {
            // the preflight already refuses a tip with no test note
            assert_red(&keel(p, &["preflight", "wu", "wu/e/a"]), "1·notes");
            bless(p, WU_TESTS, "PASS");
            git(p, &["notes", "--ref=cr", "remove", "HEAD"]);
        }),
        ("2·manifest", |p| bless(p, r#"{"unit":{"files":["tests/test_a.py"],"covers":"a"}}"#, "PASS")),
        ("3·review", |p| bless(p, WU_TESTS, "FAIL")),
        ("4·review-base", |p| {
            bless(p, WU_TESTS, "PASS");
            edit_note(p, "cr", "HEAD", |n| n["base"] = json!("0123456789012345678901234567890123456789"));
        }),
        ("5·diff-binding", |p| {
            bless(p, WU_TESTS, "PASS");
            edit_note(p, "cr", "HEAD", |n| n["code_diff_hash"] = json!("deadbeef"));
        }),
        ("6·doc-note", |p| {
            bless(p, WU_TESTS, "PASS");
            edit_note(p, "doc", "HEAD", |n| n["changes"] = json!([]));
        }),
        ("6·doc-note", |p| {
            // misclassified: docs/a.html is T1 by the default tiers
            bless(p, WU_TESTS, "PASS");
            edit_note(p, "doc", "HEAD", |n| n["changes"][0]["tier"] = json!("T3"));
        }),
    ];
    for (check, tamper) in cases {
        let d = fixture();
        tamper(d.path());
        let o = keel(d.path(), &["verify-dod", "wu", "wu/e/a"]);
        assert_red(&o, check);
        assert!(run("git", d.path(), &["notes", "--ref=dod", "show", "HEAD"], "").stdout.is_empty(), "no receipt on red");
    }
}

#[test]
fn docs_only_wu_is_exempt_but_still_needs_its_sign_off() {
    let d = fixture();
    let p = d.path();
    git(p, &["switch", "-q", "-c", "wu/e/docs", "epic/e"]);
    write(p, "docs/only.html", "<p>docs</p>");
    commit(p, "docs only");
    // UC-GATE-5: exempt from test/cr/doc evidence, never from the human sign-off.
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/docs"]), "8·manual-review");
    sign(p, "wu", "docs/only.html");
    let o = keel(p, &["verify-dod", "wu", "wu/e/docs"]);
    assert!(o.status.success() && text(&o).contains("exempt"), "{}", text(&o));
    assert!(!run("git", p, &["notes", "--ref=dod", "show", "HEAD"], "").status.success());
}

/// Branch `branch` off epic/e changing one `file`; the unsigned wu gate's output.
fn exempt_run(p: &Path, branch: &str, file: &str) -> String {
    git(p, &["switch", "-q", "-c", branch, "epic/e"]);
    write(p, file, "x");
    commit(p, file);
    let o = keel(p, &["verify-dod", "wu", branch]);
    assert!(!o.status.success(), "no sign-off yet: {}", text(&o));
    text(&o)
}

#[test]
fn infra_paths_narrows_the_exemption_and_unset_keeps_everything_outside_code_paths() {
    let d = fixture();
    let t = exempt_run(d.path(), "wu/e/mk", "Makefile");
    assert!(t.contains("8·manual-review") && !t.contains("1·notes"), "default: Makefile is infra: {t}");

    let d = fixture_with("infra_paths = [\"docs/**\"]\n", "", "epic/e", "wu/e/a");
    let t = exempt_run(d.path(), "wu/e/mk", "Makefile");
    assert!(t.contains("1·notes"), "narrowed: Makefile now needs evidence: {t}");
    let t = exempt_run(d.path(), "wu/e/docs", "docs/only.html");
    assert!(t.contains("8·manual-review") && !t.contains("1·notes"), "docs/** still exempt: {t}");
}

#[test]
fn custom_branch_prefixes_get_the_same_gating() {
    let d = fixture_with("", "epic_prefix = \"release/\"\nwu_prefix = \"task/\"\n", "release/e", "task/e/a");
    let p = d.path();
    // Move the epic ahead so a wrongly inferred target (main) would give a different base.
    git(p, &["switch", "-q", "release/e"]);
    write(p, "NOTES", "epic work");
    commit(p, "epic moves");
    git(p, &["switch", "-q", "task/e/a"]);
    git(p, &["rebase", "-q", "release/e"]);

    assert!(keel(p, &["hooks", "install"]).status.success());
    git(p, &["switch", "-q", "release/e"]);
    let m = run("git", p, &["merge", "--no-ff", "-m", "merge task", "task/e/a"], "");
    assert_red(&m, "1·notes"); // release/* is detected as an epic: the merge is wu-gated
    let _ = run("git", p, &["merge", "--abort"], "");

    git(p, &["switch", "-q", "task/e/a"]);
    bless(p, WU_TESTS, "PASS");
    sign(p, "wu", "docs/a.html");
    let o = keel(p, &["verify-dod", "wu", "task/e/a"]);
    assert!(o.status.success(), "{}", text(&o));
    let base = note(p, "dod", "task/e/a")["base"].as_str().unwrap().to_string();
    assert_eq!(base, git(p, &["rev-parse", "release/e"]), "task/e/* is measured against release/e");
    git(p, &["switch", "-q", "release/e"]);
    let m = run("git", p, &["merge", "--no-ff", "-m", "merge task", "task/e/a"], "");
    assert!(m.status.success(), "{}", text(&m));
}

#[test]
fn epic_walk_catches_a_fast_forward_smuggled_seam_unless_waived() {
    let d = fixture();
    let p = d.path();
    bless(p, WU_TESTS, "PASS");
    sign(p, "wu", "docs/a.html");
    git(p, &["switch", "-q", "epic/e"]);
    git(p, &["merge", "-q", "--no-ff", "-m", "merge wu a", "wu/e/a"]);
    write(p, "docs/result.html", "<p>result</p>");
    commit(p, "close epic");
    bless(p, EPIC_TESTS, "PASS");
    sign(p, "epic", "docs/result.html");
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    assert!(o.status.success(), "a --no-ff, blessed seam walks green: {}", text(&o));

    // WU b lands by fast-forward: its code commit is a first-parent seam with no notes.
    git(p, &["switch", "-q", "-c", "wu/e/b"]);
    write(p, "src/b.py", "def b():\n    return 2\n");
    commit(p, "wu b, never gated");
    let smuggled = git(p, &["rev-parse", "HEAD"]);
    git(p, &["switch", "-q", "epic/e"]);
    git(p, &["merge", "-q", "--ff-only", "wu/e/b"]);
    write(p, "docs/result.html", "<p>result v2</p>");
    commit(p, "close epic again");
    bless(p, EPIC_TESTS, "PASS");
    assert_red(&keel(p, &["verify-dod", "epic", "epic/e"]), "7·walk·1·notes");

    edit_note(p, "cr", "HEAD", |n| n["walk_waivers"] = json!([{"seam": &smuggled[..12], "why": "   "}]));
    assert_red(&keel(p, &["verify-dod", "epic", "epic/e"]), "7·walk·1·notes"); // a blank why never waives
    edit_note(p, "cr", "HEAD", |n| n["walk_waivers"] = json!([{"seam": &smuggled[..12], "why": "hotfix, reviewed live"}]));
    sign(p, "epic", "docs/result.html");
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    assert!(o.status.success() && text(&o).contains("WAIVED"), "{}", text(&o));
}

/// The W4-review hole: a branch shipping no-op hooks at the old tracked path must not
/// neuter the gate judging its own merge - the installed hooks live in the git dir.
#[test]
fn a_branch_shipping_neutered_hooks_cannot_skip_the_merge_gate() {
    let d = fixture();
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    let hooks = git(p, &["config", "core.hooksPath"]);
    let common = git(p, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let common = std::fs::canonicalize(common).unwrap();
    assert!(Path::new(&hooks).is_absolute() && Path::new(&hooks).starts_with(&common), "{hooks}");
    assert!(git(p, &["ls-files", "--others", "--cached", ".keel/hooks"]).is_empty());

    for h in ["pre-merge-commit", "pre-commit", "pre-push"] {
        write(p, &format!(".keel/hooks/{h}"), "#!/bin/sh\nexit 0\n");
        std::fs::set_permissions(p.join(".keel/hooks").join(h), std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }
    commit(p, "neuter the gate");
    git(p, &["switch", "-q", "epic/e"]);
    let m = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], "");
    assert_red(&m, "1·notes");
    assert!(p.join(".keel/hooks/pre-merge-commit").exists(), "the neutered hook was in the merged tree");
    let _ = run("git", p, &["merge", "--abort"], "");
    assert_eq!(git(p, &["rev-list", "--count", "epic/e"]), "1", "nothing landed");
}

#[test]
fn hooks_refuse_an_unblessed_merge_then_land_it_and_push_syncs_notes() {
    let d = fixture();
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    git(p, &["switch", "-q", "epic/e"]);
    let m = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], "");
    assert_red(&m, "1·notes");
    let _ = run("git", p, &["merge", "--abort"], "");
    assert_eq!(git(p, &["rev-list", "--count", "epic/e"]), "1", "nothing landed");

    git(p, &["switch", "-q", "wu/e/a"]);
    bless(p, WU_TESTS, "PASS");
    sign(p, "wu", "docs/a.html");
    git(p, &["switch", "-q", "epic/e"]);
    let m = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], "");
    assert!(m.status.success(), "{}", text(&m));
    assert_eq!(note(p, "dod", "wu/e/a")["level"], "wu");

    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(p, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    let push = run("git", p, &["push", "-q", "origin", "epic/e"], "");
    assert!(push.status.success(), "{}", text(&push));
    let synced = git(remote.path(), &["for-each-ref", "--format=%(refname)", "refs/notes"]);
    assert!(synced.contains("refs/notes/dod") && synced.contains("refs/notes/cr"), "{synced}");

    write(p, "src/c.py", "def c():\n    return 3\n");
    commit(p, "noteless code straight on the epic");
    let push = run("git", p, &["push", "-q", "origin", "epic/e"], "");
    assert_red(&push, "1·notes");
}

#[test]
fn doc_note_records_tiers_from_config_globs_and_refuses_an_unclassified_doc() {
    let d = fixture();
    let p = d.path();
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &format!("{cfg}[docs.tiers]\nT1 = [\"docs/*\"]\nT2 = [\"docs/kb/**\"]\nT3 = [\"docs/scratch/**\"]\n"));
    write(p, "docs/kb/x.html", "x");
    write(p, "docs/scratch/y.html", "y");
    commit(p, "tiered docs");
    let o = keel(p, &["notes", "write", "doc"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        note(p, "doc", "HEAD")["changes"],
        json!([
            {"path": "docs/a.html", "tier": "T1"},
            {"path": "docs/kb/x.html", "tier": "T2"},
            {"path": "docs/scratch/y.html", "tier": "T3"}
        ])
    );
    write(p, "docs/img/z.png", "z");
    commit(p, "an untiered doc");
    let o = keel(p, &["notes", "write", "doc"]);
    assert!(!o.status.success() && text(&o).contains("docs/img/z.png matches no docs.tiers"), "{}", text(&o));
}

#[test]
fn epic_tip_rejects_a_tracked_t3_doc_and_an_archive_without_lineage() {
    let d = fixture();
    let p = d.path();
    write(p, "docs/work/plan.html", "<p>plan</p>");
    commit(p, "wu plan");
    bless(p, WU_TESTS, "PASS");
    sign(p, "wu", "docs/a.html");
    git(p, &["switch", "-q", "epic/e"]);
    git(p, &["merge", "-q", "--no-ff", "-m", "merge wu a", "wu/e/a"]);
    write(p, "docs/result.html", "<p>result</p>");
    commit(p, "close epic");
    bless(p, EPIC_TESTS, "PASS");
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    assert_red(&o, "6·doc-note");
    assert!(text(&o).contains("docs/work/plan.html is a T3 working doc still tracked"), "{}", text(&o));

    // Archive at the epic tip: created and deleted inside the range, so it nets out of the diff.
    git(p, &["rm", "-q", "docs/work/plan.html"]);
    write(p, "docs/decisions/d.html", "<p>why</p>");
    commit(p, "distill + archive the plan");
    for (args, input) in [(&["notes", "write", "test"][..], EPIC_TESTS), (&["notes", "write", "cr", "--verdict", "PASS"][..], "")] {
        assert!(run(env!("CARGO_BIN_EXE_keel"), p, args, input).status.success());
    }
    let o = keel(p, &["notes", "write", "doc"]);
    assert!(!o.status.success() && text(&o).contains("without distillation lineage"), "{}", text(&o));
    let o = keel(p, &["notes", "write", "doc", "--distill", "docs/work/plan.html=docs/decisions/d.html"]);
    assert!(o.status.success(), "{}", text(&o));
    sign(p, "epic", "docs/result.html");
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    assert!(o.status.success(), "{}", text(&o));

    edit_note(p, "doc", "HEAD", |n| {
        let plan = n["changes"].as_array_mut().unwrap().iter_mut().find(|c| c["path"] == "docs/work/plan.html").unwrap();
        plan.as_object_mut().unwrap().remove("distilled_to");
    });
    assert_red(&keel(p, &["verify-dod", "epic", "epic/e"]), "6·doc-note");
}

/// cr blocker 1: `git merge --no-commit` skips pre-merge-commit, and `--ff-only` makes no merge
/// commit, so the push range is the only gate left - it must judge merge seams, not skip them.
#[test]
fn push_of_a_fast_forwarded_no_commit_wu_merge_is_gated() {
    let d = fixture();
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(p, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    git(p, &["push", "-q", "origin", "main"]);

    git(p, &["switch", "-q", "epic/e"]);
    git(p, &["merge", "-q", "--no-ff", "--no-commit", "wu/e/a"]);
    git(p, &["commit", "-qm", "merge wu a, never gated"]);
    git(p, &["switch", "-q", "main"]);
    git(p, &["merge", "-q", "--ff-only", "epic/e"]);
    let push = run("git", p, &["push", "-q", "origin", "main"], "");
    assert_red(&push, "1·notes");
    assert_ne!(git(remote.path(), &["rev-parse", "main"]), git(p, &["rev-parse", "main"]), "nothing landed");
}

/// `keel verify-dod commit HEAD` after one commit on wu/e/a; `cfg` replaces the fixture config.
fn commit_gate(p: &Path, cfg: Option<&str>, change: impl FnOnce(&Path)) -> Output {
    if let Some(c) = cfg {
        write(p, ".keel/config.toml", c);
    }
    change(p);
    git(p, &["add", "-A", "src", "tests", "docs"]);
    git(p, &["commit", "-qm", "change"]);
    keel(p, &["verify-dod", "commit", "HEAD"])
}

/// cr blocker 3: a rename out of code_paths and a quoted non-ASCII path both still count as code.
#[test]
fn renamed_and_non_ascii_code_paths_are_not_docs_only() {
    let (d, e) = (fixture(), fixture());
    let moved = commit_gate(d.path(), None, |p| {
        git(p, &["mv", "src/a.py", "docs/a.py"]);
    });
    let quoted = commit_gate(e.path(), None, |p| write(p, "src/é.py", "def e():\n    pass\n"));
    let red = |o: &Output| !o.status.success() && text(o).contains("1·notes");
    assert!(red(&moved) && red(&quoted), "rename: {}\nnon-ASCII: {}", text(&moved), text(&quoted));
}

/// cr blocker 4: code_paths are git pathspecs for both the exemption and the cr binding.
#[test]
fn code_paths_pathspec_globs_gate_and_bind_the_same_files() {
    let cfg = "code_paths = [\"*.py\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n";
    let d = fixture();
    let o = commit_gate(d.path(), Some(cfg), |p| write(p, "src/b.py", "def b():\n    return 2\n"));
    assert_red(&o, "1·notes");
}

/// cr blocker 5a: a non-ASCII T3 doc created and archived inside the range still owes its lineage.
#[test]
fn a_non_ascii_archived_t3_doc_needs_distillation_lineage() {
    let d = fixture();
    let p = d.path();
    write(p, "docs/work/plán.html", "<p>plan</p>");
    commit(p, "plan");
    git(p, &["rm", "-q", "docs/work/plán.html"]);
    write(p, "docs/decisions/d.html", "<p>why</p>");
    commit(p, "distill + archive the plan");
    for (args, input) in [(&["notes", "write", "test"][..], WU_TESTS), (&["notes", "write", "cr", "--verdict", "PASS"][..], "")] {
        assert!(run(env!("CARGO_BIN_EXE_keel"), p, args, input).status.success());
    }
    let o = keel(p, &["notes", "write", "doc"]);
    assert!(!o.status.success() && text(&o).contains("docs/work/plán.html is archived without distillation lineage"), "{}", text(&o));
    let o = keel(p, &["notes", "write", "doc", "--distill", "docs/work/plán.html=docs/decisions/d.html"]);
    assert!(o.status.success(), "{}", text(&o));
    edit_note(p, "doc", "HEAD", |n| {
        let plan = n["changes"].as_array_mut().unwrap().iter_mut().find(|c| c["path"] == "docs/work/plán.html").unwrap();
        plan.as_object_mut().unwrap().remove("distilled_to");
    });
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/a"]), "6·doc-note");
}

/// cr blocker 5b: a non-ASCII T1 doc tracked at the epic tip is a T1 doc, not an untiered one.
#[test]
fn a_non_ascii_t1_doc_at_the_epic_tip_is_green() {
    let d = fixture();
    let p = d.path();
    bless(p, WU_TESTS, "PASS");
    sign(p, "wu", "docs/a.html");
    git(p, &["switch", "-q", "epic/e"]);
    git(p, &["merge", "-q", "--no-ff", "-m", "merge wu a", "wu/e/a"]);
    write(p, "docs/résultat.html", "<p>result</p>");
    commit(p, "close epic");
    bless(p, EPIC_TESTS, "PASS");
    sign(p, "epic", "docs/résultat.html");
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    assert!(o.status.success(), "{}", text(&o));
}

/// I9: once another tool takes core.hooksPath (or it is unset), the gate is silently off: preflight
/// and verify-dod wu refuse with 0·hooks, and keel check warns but keeps its exit code.
#[test]
fn a_foreign_or_unset_hooks_path_turns_preflight_and_verify_dod_red() {
    let d = fixture();
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    bless(p, WU_TESTS, "PASS");
    sign(p, "wu", "docs/a.html");
    assert!(keel(p, &["preflight", "wu", "wu/e/a"]).status.success());
    assert!(keel(p, &["verify-dod", "wu", "wu/e/a"]).status.success());
    git(p, &["notes", "--ref=dod", "remove", "wu/e/a"]);

    for (what, args) in [(".husky", &["config", "core.hooksPath", ".husky"][..]), ("unset", &["config", "--unset", "core.hooksPath"][..])] {
        git(p, args);
        for cmd in [&["preflight", "wu", "wu/e/a"][..], &["verify-dod", "wu", "wu/e/a"][..]] {
            let o = keel(p, cmd);
            assert_red(&o, "0·hooks");
            assert!(text(&o).contains("keel hooks install"), "{what} {cmd:?}: {}", text(&o));
        }
        assert!(!run("git", p, &["notes", "--ref=dod", "show", "wu/e/a"], "").status.success(), "{what}: no receipt on red");
        let c = keel(p, &["check"]);
        assert!(c.status.success(), "{what}: {}", text(&c));
        assert!(String::from_utf8_lossy(&c.stderr).contains("warning") && String::from_utf8_lossy(&c.stderr).contains("keel hooks install"), "{what}: {}", text(&c));
    }
}

/// cr important: the cr binding is the same bytes whatever the local diff config.
#[test]
fn diff_binding_ignores_local_diff_color_and_external_diff() {
    let d = fixture();
    let p = d.path();
    bless(p, WU_TESTS, "PASS");
    sign(p, "wu", "docs/a.html");
    git(p, &["config", "color.diff", "always"]);
    git(p, &["config", "diff.external", "false"]);
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    assert!(o.status.success(), "{}", text(&o));
}

// E-CRCAP.W1: the WAIVED verdict (UC-REV-2).

const RULING: &str = "owner ruling 2026-11-12: review waived, open findings tracked in R31";

fn write_cr(p: &Path, args: &[&str]) -> Output {
    keel(p, &[&["notes", "write", "cr"][..], args].concat())
}

/// The test and doc notes, then the owner's waiver in place of a review.
fn bless_waived(p: &Path, tests: &str, reason: &str) {
    for (args, input) in [(&["notes", "write", "test"][..], tests), (&["notes", "write", "doc"][..], "")] {
        let o = run(env!("CARGO_BIN_EXE_keel"), p, args, input);
        assert!(o.status.success(), "{args:?}: {}", text(&o));
    }
    let o = write_cr(p, &["--verdict", "WAIVED", "--reason", reason]);
    assert!(o.status.success(), "waive: {}", text(&o));
}

/// Accept a tool-written waiver in two steps: the refused run prints the code, the re-run gives it back.
fn sign_waived(p: &Path, level: &str, report: &str) {
    let args = ["mr", "confirm", level, "--as", "t", "--report", report];
    let preview = keel(p, &args);
    let re = regex::Regex::new(r"--accept-waiver ([0-9a-f]{12})").unwrap();
    let code = re.captures(&text(&preview)).unwrap_or_else(|| panic!("no accept code: {}", text(&preview)))[1].to_string();
    let o = keel(p, &[&args[..], &["--accept-waiver", &code]].concat());
    assert!(o.status.success(), "mr confirm --accept-waiver: {}", text(&o));
}

fn has_note(p: &Path, kind: &str, rev: &str) -> bool {
    run("git", p, &["notes", &format!("--ref={kind}"), "show", rev], "").status.success()
}

#[test]
fn a_waiver_needs_a_reason_is_bound_like_a_review_and_prints_at_the_gate() {
    let d = fixture();
    let p = d.path();
    bless(p, WU_TESTS, "PASS");
    let pass = note(p, "cr", "HEAD");
    git(p, &["notes", "--ref=cr", "remove", "HEAD"]);
    for args in [
        &["--verdict", "WAIVED"][..],
        &["--verdict", "WAIVED", "--reason", "   "],
        &["--verdict", "WAIVED", "--reason"],
        &["--verdict", "PASS", "--reason", "x"],
        &["--verdict", "FAIL", "--blockers", "1", "--reason", "x"],
    ] {
        let o = write_cr(p, args);
        assert!(!o.status.success(), "{args:?} was accepted: {}", text(&o));
        assert!(!has_note(p, "cr", "HEAD"), "{args:?} wrote a note");
    }
    let o = write_cr(p, &["--verdict", "WAIVED", "--reason", &format!("  {RULING}\n"), "--blockers", "2"]);
    assert!(o.status.success(), "{}", text(&o));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["verdict"].as_str(), n["waiver"].as_str(), n["blockers"].as_u64()), (Some("WAIVED"), Some(RULING), Some(2)));
    for f in ["base", "tip", "code_diff_hash"] {
        assert_eq!(n[f], pass[f], "{f}: a waiver is bound like a review");
    }

    // The gate accepts and prints it; only the sign-off is missing. Preflight never reads cr.
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    let t = text(&o);
    assert!(t.contains(&format!("WAIVED 3·review: {RULING}")), "{t}");
    assert_red(&o, "8·manual-review");
    assert!(t.contains("- 1 failed") && !t.contains("FAIL  3·review"), "{t}");
    assert!(keel(p, &["preflight", "wu", "wu/e/a"]).status.success());

    // A waiver with its reason blanked by hand waives nothing.
    edit_note(p, "cr", "HEAD", |n| n["waiver"] = json!("  "));
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/a"]), "3·review");
    edit_note(p, "cr", "HEAD", |n| {
        n.as_object_mut().unwrap().remove("waiver");
    });
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/a"]), "3·review");
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", RULING]).status.success());

    // Sign-off needs the explicit acceptance (identity tier); then green, and the receipt says so.
    let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"]);
    assert!(!o.status.success() && text(&o).contains(RULING) && text(&o).contains("--accept-waiver"), "{}", text(&o));
    assert!(!has_note(p, "mr", "HEAD"), "a refused confirm writes nothing");
    sign_waived(p, "wu", "docs/a.html");
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    assert!(o.status.success() && text(&o).contains(&format!("WAIVED 3·review: {RULING}")), "{}", text(&o));
    let dod = note(p, "dod", "wu/e/a");
    assert_eq!((dod["review"].as_str(), dod["waiver"].as_str()), (Some("WAIVED"), Some(RULING)), "{dod}");

    // An identity-tier sign-off has no signature: the note's waiver hash still covers the reason.
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", "reworded after the owner signed"]).status.success());
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    assert_red(&o, "8·manual-review");
    assert!(text(&o).contains("waiver"), "{}", text(&o));
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", RULING]).status.success());
    assert!(keel(p, &["verify-dod", "wu", "wu/e/a"]).status.success(), "rebuilt from the current note, never stored");

    // A code commit after the waiver: the new tip has no review, and the old waiver carried onto it binds other code.
    let waived = note(p, "cr", "HEAD");
    write(p, "src/a.py", "def a():\n    return 2\n");
    commit(p, "change after the waiver");
    for (args, input) in [(&["notes", "write", "test"][..], WU_TESTS), (&["notes", "write", "doc"][..], "")] {
        assert!(run(env!("CARGO_BIN_EXE_keel"), p, args, input).status.success());
    }
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/a"]), "1·notes");
    let mut carried = waived;
    carried["tip"] = json!(git(p, &["rev-parse", "HEAD"]));
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &carried.to_string(), "HEAD"]);
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/a"]), "5·diff-binding");
}

#[test]
fn the_epic_walk_passes_a_waived_wu_and_an_epic_review_can_be_waived() {
    let d = fixture();
    let p = d.path();
    bless_waived(p, WU_TESTS, RULING);
    sign_waived(p, "wu", "docs/a.html");
    git(p, &["switch", "-q", "epic/e"]);
    let o = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], "");
    assert!(o.status.success(), "the merge hook passes a waived, accepted WU: {}", text(&o));
    write(p, "docs/result.html", "<p>result</p>");
    commit(p, "close epic");
    bless(p, EPIC_TESTS, "PASS");
    sign(p, "epic", "docs/result.html");
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("WAIVED 7·walk") && t.contains(RULING), "the walk prints the waived seam: {t}");
    assert!(note(p, "dod", "epic/e").get("review").is_none(), "the epic's own review passed");

    // The E-SEED2 close case: the epic's own review is waived.
    let close = "owner ruling: epic review waived at close";
    git(p, &["notes", "--ref=mr", "remove", "HEAD"]);
    git(p, &["notes", "--ref=dod", "remove", "HEAD"]);
    // The tool never waives over a PASS: this epic's review stopped before one (no cr note).
    git(p, &["notes", "--ref=cr", "remove", "HEAD"]);
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", close]).status.success());
    let o = keel(p, &["mr", "confirm", "epic", "--as", "t", "--report", "docs/result.html"]);
    assert!(!o.status.success() && text(&o).contains(close), "{}", text(&o));
    sign_waived(p, "epic", "docs/result.html");
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    assert!(o.status.success() && text(&o).contains(&format!("WAIVED 3·review: {close}")), "{}", text(&o));
    assert_eq!(note(p, "dod", "epic/e")["review"], "WAIVED");
}

// E-CRCAP.W1 round 2: the owner records a waiver at sign-off; no session writes it (router decision 2026-11-12).

fn bless_unreviewed(p: &Path, tests: &str) {
    for (args, input) in [(&["notes", "write", "test"][..], tests), (&["notes", "write", "doc"][..], "")] {
        let o = run(env!("CARGO_BIN_EXE_keel"), p, args, input);
        assert!(o.status.success(), "{args:?}: {}", text(&o));
    }
}

/// The code a `--waive-review` preview prints for its confirming re-run.
fn confirm_code(t: &str) -> Option<String> {
    let re = regex::Regex::new(r"--confirm-waiver ([0-9a-f]{12})").unwrap();
    re.captures(t).map(|c| c[1].to_string())
}

/// The owner's two steps: the preview prints the listing and refuses; the re-run with its code signs.
fn waive_at_sign_off(p: &Path, level: &str, report: &str, reason: &str) -> Output {
    let args = ["mr", "confirm", level, "--as", "t", "--report", report, "--waive-review", reason];
    let preview = keel(p, &args);
    match confirm_code(&text(&preview)) {
        Some(code) if !preview.status.success() => keel(p, &[&args[..], &["--confirm-waiver", &code]].concat()),
        _ => preview,
    }
}

#[test]
fn the_owner_sees_the_waiver_before_it_is_written_and_confirms_what_was_shown() {
    let d = fixture();
    let p = d.path();
    bless_unreviewed(p, WU_TESTS);
    let args = ["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING];
    let preview = keel(p, &args);
    let t = text(&preview);
    assert!(!preview.status.success() && t.contains(RULING) && t.contains("not reviewed") && !t.contains("0 blockers"), "{t}");
    let code = confirm_code(&t).unwrap_or_else(|| panic!("no confirm code: {t}"));
    assert!(!has_note(p, "cr", "HEAD") && !has_note(p, "mr", "HEAD"), "a preview writes nothing");
    for bad in [&["--confirm-waiver", "000000000000"][..], &["--confirm-waiver", &code, "--accept-waiver"], &["--accept-waiver"]] {
        let o = keel(p, &[&args[..], bad].concat());
        assert!(!o.status.success() && text(&o).contains("fix ->"), "{bad:?}: {}", text(&o));
        assert!(!has_note(p, "cr", "HEAD") && !has_note(p, "mr", "HEAD"), "{bad:?} wrote a note");
    }
    // The code binds what was shown: a review recorded since, or other words, need a new preview.
    let reworded = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", "other words", "--confirm-waiver", &code]);
    assert!(!reworded.status.success() && !has_note(p, "mr", "HEAD"), "{}", text(&reworded));
    assert!(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]).status.success());
    let stale = keel(p, &[&args[..], &["--confirm-waiver", &code]].concat());
    assert!(!stale.status.success() && text(&stale).contains("changed since"), "{}", text(&stale));
    assert!(!has_note(p, "mr", "HEAD"));
    assert_eq!(note(p, "cr", "HEAD")["verdict"], "FAIL");
    let o = waive_at_sign_off(p, "wu", "docs/a.html", RULING);
    assert!(o.status.success() && text(&o).contains(RULING), "{}", text(&o));
    assert!(keel(p, &["verify-dod", "wu", "wu/e/a"]).status.success());
}

#[test]
fn the_owner_waives_an_unreviewed_tip_at_sign_off_and_the_gate_reads_it_as_a_waiver() {
    let d = fixture();
    let p = d.path();
    bless_unreviewed(p, WU_TESTS);
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/a"]), "1·notes");
    // Without the waiver, sign-last still refuses a tip with no review.
    let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"]);
    assert!(!o.status.success() && text(&o).contains("sign last"), "{}", text(&o));
    for blank in ["", "   ", "--as"] {
        let o = keel(p, &["mr", "confirm", "wu", "--report", "docs/a.html", "--waive-review", blank, "--as", "t"]);
        assert!(!o.status.success() && text(&o).contains("fix ->"), "{blank:?}: {}", text(&o));
        assert!(!has_note(p, "cr", "HEAD") && !has_note(p, "mr", "HEAD"), "{blank:?}: a refused waiver writes nothing");
    }

    let o = waive_at_sign_off(p, "wu", "docs/a.html", &format!(" {RULING} "));
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains(RULING), "{}", text(&o));
    let cr = note(p, "cr", "HEAD");
    assert_eq!((cr["verdict"].as_str(), cr["waiver"].as_str()), (Some("WAIVED"), Some(RULING)), "{cr}");
    assert_eq!(cr["tip"].as_str().unwrap(), git(p, &["rev-parse", "HEAD"]));
    assert!(note(p, "mr", "HEAD")["waiver_hash"].as_str().unwrap().starts_with("sha256:"));
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    assert!(o.status.success() && text(&o).contains(&format!("WAIVED 3·review: {RULING}")), "checks 3-5 and 8 read the owner's waiver: {}", text(&o));
    assert_eq!(note(p, "dod", "wu/e/a")["review"], "WAIVED");

    // The real merge hook passes it; the epic walk prints it.
    git(p, &["notes", "--ref=dod", "remove", "wu/e/a"]);
    git(p, &["switch", "-q", "epic/e"]);
    let o = run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], "");
    assert!(o.status.success(), "{}", text(&o));
    write(p, "docs/result.html", "<p>result</p>");
    commit(p, "close epic");
    bless_unreviewed(p, EPIC_TESTS);
    // An epic's own review waived by the owner at its sign-off (the E-SEED2 close case).
    let close = "owner ruling: epic review waived at close";
    assert!(waive_at_sign_off(p, "epic", "docs/result.html", close).status.success());
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("WAIVED 7·walk") && t.contains(RULING) && t.contains(&format!("WAIVED 3·review: {close}")), "{t}");
}

#[test]
fn an_owner_waiver_replaces_a_failed_round_carrying_its_findings_and_never_a_pass() {
    let d = fixture();
    let p = d.path();
    bless(p, WU_TESTS, "PASS");
    // A review that passed is never waived over; a stale one needs a new review.
    let o = waive_at_sign_off(p, "wu", "docs/a.html", RULING);
    assert!(!o.status.success() && text(&o).contains("PASS") && text(&o).contains("new review") && text(&o).contains("fix ->"), "{}", text(&o));
    assert_eq!(note(p, "cr", "HEAD")["verdict"], "PASS");
    assert!(!has_note(p, "mr", "HEAD"));

    // A failed round on the tip: its counts, its unwaived findings and walk waivers are shown before
    // the waiver is written, and every field the review recorded stays on the note.
    assert!(write_cr(p, &["--verdict", "FAIL", "--blockers", "2", "--importants", "1"]).status.success());
    let recorded = json!({
        "findings": [
            {"location": "src/a.py:2", "claim": "a() is wrong on 0", "severity": "blocker", "evidence": "a(0) returns 1"},
            {"location": "src/a.py:1", "claim": "a() is slow", "severity": "important", "evidence": "e", "waived": "owner: fine"}
        ],
        "refuted": [{"location": "src/a.py:1", "claim": "c", "why": "w"}],
        "nits": [{"location": "src/a.py:1", "finding": "n", "fixed": "f"}],
        "fleet": {"finders": 2},
        "walk_waivers": [{"seam": "abc", "why": "seam kept"}]
    });
    edit_note(p, "cr", "HEAD", |n| n.as_object_mut().unwrap().extend(recorded.as_object().unwrap().clone()));
    let preview = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING]);
    let t = text(&preview);
    for needle in [RULING, "open: 2 blockers, 1 importants", "blocker src/a.py:2: a() is wrong on 0", "FAIL blockers 2 importants 1", "walk waiver abc: seam kept"] {
        assert!(t.contains(needle), "missing '{needle}': {t}");
    }
    assert!(!t.contains("a() is slow"), "a waived finding is not open: {t}");
    let o = waive_at_sign_off(p, "wu", "docs/a.html", RULING);
    assert!(o.status.success(), "{}", text(&o));
    let cr = note(p, "cr", "HEAD");
    assert_eq!((cr["verdict"].as_str(), cr["blockers"].as_u64(), cr["importants"].as_u64()), (Some("WAIVED"), Some(2), Some(1)), "{cr}");
    for f in ["findings", "refuted", "nits", "fleet", "walk_waivers"] {
        assert_eq!(cr[f], recorded[f], "{f} kept on the waiver");
    }
    assert!(keel(p, &["verify-dod", "wu", "wu/e/a"]).status.success());

    // Sign-last holds for everything else: a missing doc note refuses the waiver and writes nothing.
    let d = fixture();
    let p = d.path();
    let o = run(env!("CARGO_BIN_EXE_keel"), p, &["notes", "write", "test"], WU_TESTS);
    assert!(o.status.success());
    let o = waive_at_sign_off(p, "wu", "docs/a.html", RULING);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("1·notes") && t.contains("doc/v1") && t.contains("waive") && !t.contains("cr PASS"), "{t}");
    assert!(!has_note(p, "cr", "HEAD") && !has_note(p, "mr", "HEAD"));
}

// E-CRCAP.W1 review round 2 leads: the listing is safe to read, complete, and bound to the note written.

#[test]
fn the_waiver_preview_is_inert_text_bound_to_the_note_and_refused_on_a_broken_note_or_report() {
    let d = fixture();
    let p = d.path();
    bless_unreviewed(p, WU_TESTS);
    let args = ["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING];

    // A cr note that does not parse is never taken for "no review" and overwritten.
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", "{not json", "HEAD"]);
    let o = keel(p, &args);
    assert!(!o.status.success() && text(&o).contains("does not parse") && text(&o).contains("fix ->"), "{}", text(&o));
    assert_eq!(git(p, &["notes", "--ref=cr", "show", "HEAD"]), "{not json");

    // A failed round whose recorded text carries terminal control sequences.
    assert!(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]).status.success());
    edit_note(p, "cr", "HEAD", |n| {
        n["findings"] = json!([{"location": "src/a.py:2", "claim": "hidden\u{1b}[2K\u{1b}[1Aclaim", "severity": "blocker", "evidence": "e"}]);
        n["walk_waivers"] = json!([{"seam": "abc", "why": "bell\u{7}why"}]);
        n["committer"] = json!("reviewer <r@r>");
        n["timestamp"] = json!(1);
    });
    // The report is settled before the waiver is shown: a bad report refuses with no waiver printed.
    let bad = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/nope.html", "--waive-review", RULING]);
    assert!(!bad.status.success() && !text(&bad).contains("is WAIVED:"), "{}", text(&bad));

    let preview = keel(p, &args);
    let t = text(&preview);
    assert!(!t.contains('\u{1b}') && !t.contains('\u{7}'), "control characters reach the terminal: {t:?}");
    assert!(t.contains(r"hidden\u{1b}[2K\u{1b}[1Aclaim") && t.contains(r"bell\u{7}why"), "{t}");
    let code = confirm_code(&t).unwrap();

    // The code binds the note to be written, not only the lines shown: a changed refuted list needs a new preview.
    edit_note(p, "cr", "HEAD", |n| n["refuted"] = json!([{"location": "src/a.py:1", "claim": "c", "why": "added after the preview"}]));
    let stale = keel(p, &[&args[..], &["--confirm-waiver", &code]].concat());
    assert!(!stale.status.success() && text(&stale).contains("changed since"), "{}", text(&stale));
    assert!(!has_note(p, "mr", "HEAD"));

    let o = waive_at_sign_off(p, "wu", "docs/a.html", RULING);
    assert!(o.status.success(), "{}", text(&o));
    let raw = git(p, &["notes", "--ref=cr", "show", "HEAD"]);
    for key in ["\"tip\"", "\"base\"", "\"committer\"", "\"timestamp\"", "\"verdict\""] {
        assert_eq!(raw.matches(key).count(), 1, "{key} once: {raw}");
    }
    let cr = note(p, "cr", "HEAD");
    assert_eq!(cr["committer"], "t <t@t>", "the signer writes the waiver: {cr}");
    assert!(cr["timestamp"].as_u64().unwrap() > 1, "{cr}");
    assert_eq!(cr["refuted"][0]["why"], "added after the preview");
}

// E-CRCAP.W1 code review round 1: one cr/v1 waiver writer.

#[test]
fn a_waiver_reason_is_never_the_next_flag() {
    let d = fixture();
    let p = d.path();
    bless_unreviewed(p, WU_TESTS);
    for args in [&["--verdict", "WAIVED", "--reason", "--blockers", "2"][..], &["--verdict", "WAIVED", "--blockers", "2", "--reason", "--importants"]] {
        let o = write_cr(p, args);
        assert!(!o.status.success() && text(&o).contains("fix ->"), "{args:?}: {}", text(&o));
        assert!(!has_note(p, "cr", "HEAD"), "{args:?} wrote a note");
    }
}

#[test]
fn a_tool_written_waiver_with_no_review_on_the_tip_says_not_reviewed() {
    let d = fixture();
    let p = d.path();
    bless_unreviewed(p, WU_TESTS);
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", "review cap: 5 rounds, 0 blockers open"]).status.success());
    assert_eq!(note(p, "cr", "HEAD")["unreviewed"], true);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"]);
    assert!(!o.status.success() && text(&o).contains("not reviewed") && !text(&o).contains("0 blockers, 0 importants"), "{}", text(&o));
    // B3: the history row says so too, never "blockers 0 importants 0".
    let tip = git(p, &["rev-parse", "HEAD"]);
    let row = text(&o).lines().find(|l| l.trim_start().starts_with(&tip[..10])).map(String::from).unwrap_or_else(|| panic!("no history row: {}", text(&o)));
    assert!(row.contains("WAIVED not reviewed") && !row.contains("blockers 0"), "{row}");

    // Over a reviewed round the tool's waiver is a reviewed one, and keeps what the round recorded.
    assert!(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]).status.success());
    edit_note(p, "cr", "HEAD", |n| n["findings"] = json!([{"location": "src/a.py:2", "claim": "kept", "severity": "blocker", "evidence": "e"}]));
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", "review cap: 5 rounds, 1 blockers open", "--blockers", "1"]).status.success());
    let n = note(p, "cr", "HEAD");
    assert!(n.get("unreviewed").is_none(), "{n}");
    assert_eq!((n["blockers"].as_u64(), n["findings"][0]["claim"].as_str()), (Some(1), Some("kept")), "{n}");
    // A PASS is never waived over, by the tool either.
    assert!(write_cr(p, &["--verdict", "PASS"]).status.success());
    let o = write_cr(p, &["--verdict", "WAIVED", "--reason", "cap"]);
    assert!(!o.status.success() && text(&o).contains("PASS"), "{}", text(&o));
    assert_eq!(note(p, "cr", "HEAD")["verdict"], "PASS");
}

// E-CRCAP.W1 code review round 2.

/// B1: a count flag the writer was not given keeps the count the replaced round recorded.
#[test]
fn a_waiver_keeps_the_open_count_it_was_not_given() {
    let d = fixture();
    let p = d.path();
    bless_unreviewed(p, WU_TESTS);
    assert!(write_cr(p, &["--verdict", "FAIL", "--blockers", "2", "--importants", "3"]).status.success());
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", "review cap: 5 rounds, 2 blockers open", "--blockers", "2"]).status.success());
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["blockers"].as_u64(), n["importants"].as_u64()), (Some(2), Some(3)), "{n}");
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", "review cap: 5 rounds, 1 blockers open", "--importants", "1"]).status.success());
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["blockers"].as_u64(), n["importants"].as_u64()), (Some(2), Some(1)), "{n}");
    let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"]);
    assert!(text(&o).contains("open: 2 blockers, 1 importants"), "{}", text(&o));
}

/// B4: a round recorded against another range is never shown or recorded as this tip's review.
#[test]
fn a_review_of_another_range_is_never_this_tips_review() {
    for owner in [true, false] {
        let d = fixture();
        let p = d.path();
        bless_unreviewed(p, WU_TESTS);
        assert!(write_cr(p, &["--verdict", "FAIL", "--blockers", "2", "--importants", "1"]).status.success());
        edit_note(p, "cr", "HEAD", |n| {
            n["code_diff_hash"] = json!("0000000000000000000000000000000000000000");
            n["findings"] = json!([{"location": "src/a.py:9", "claim": "about other code", "severity": "blocker", "evidence": "e"}]);
        });
        let n = if owner {
            let preview = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING]);
            let t = text(&preview);
            assert!(t.contains("not reviewed") && t.contains("another range") && !t.contains("about other code") && !t.contains("open: 2 blockers"), "{t}");
            assert!(waive_at_sign_off(p, "wu", "docs/a.html", RULING).status.success());
            note(p, "cr", "HEAD")
        } else {
            assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", "review cap"]).status.success());
            note(p, "cr", "HEAD")
        };
        assert_eq!((n["unreviewed"].as_bool(), n["blockers"].as_u64(), n["importants"].as_u64()), (Some(true), Some(0), Some(0)), "{owner}: {n}");
        assert!(n.get("findings").is_none(), "{owner}: the stale round's findings are not this tip's: {n}");
        assert_eq!(n["replaced"]["findings"][0]["claim"], "about other code", "{owner}: kept as history: {n}");
        assert_eq!(n["replaced"]["code_diff_hash"], "0000000000000000000000000000000000000000");
    }
}

/// I-c: the one writer refuses a note it cannot parse, whoever calls it, with one fix text.
#[test]
fn every_waiver_writer_refuses_a_cr_note_it_cannot_parse() {
    let d = fixture();
    let p = d.path();
    bless_unreviewed(p, WU_TESTS);
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", "{not json", "HEAD"]);
    let tool = write_cr(p, &["--verdict", "WAIVED", "--reason", "cap"]);
    let owner = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING]);
    let fix = |o: &Output| text(o).lines().find(|l| l.contains("fix ->")).map(|l| l.split("fix ->").nth(1).unwrap().trim().to_string());
    for o in [&tool, &owner] {
        assert!(!o.status.success() && text(o).contains("does not parse"), "{}", text(o));
    }
    assert_eq!(fix(&tool), fix(&owner), "one fix text");
    assert_eq!(git(p, &["notes", "--ref=cr", "show", "HEAD"]), "{not json");
}

// E-CRCAP.W1 code review round 4: the review history never depends on the user's display config.

#[test]
fn the_review_history_lists_every_round_intact_whatever_the_log_encoding_and_shows_an_unreadable_one() {
    let d = fixture();
    let p = d.path();
    // An older round whose note carries non-ASCII text (a tool waiver), then one whose note does not parse.
    bless_unreviewed(p, WU_TESTS);
    let cap = "review cap: Größe übrig";
    assert!(write_cr(p, &["--verdict", "WAIVED", "--reason", cap]).status.success());
    let older = git(p, &["rev-parse", "HEAD"]);
    write(p, "src/a.py", "def a():\n    return 2\n");
    commit(p, "round 2");
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", "{not json", "HEAD"]);
    let broken = git(p, &["rev-parse", "HEAD"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    commit(p, "fix");
    bless_unreviewed(p, WU_TESTS);
    let mut wrong = vec![];
    for encoding in [None, Some("ISO-8859-1"), Some("UTF-16")] {
        if let Some(e) = encoding {
            git(p, &["config", "i18n.logOutputEncoding", e]);
            git(p, &["config", "i18n.commitEncoding", e]);
        }
        let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING]);
        let t = text(&o);
        let row = |sha: &str| t.lines().find(|l| l.trim_start().starts_with(&sha[..10])).map(String::from);
        match row(&older) {
            Some(r) if r.contains(&format!("WAIVED not reviewed - {cap}")) => {}
            other => wrong.push(format!("{encoding:?}: older round row {other:?}")),
        }
        match row(&broken) {
            Some(r) if r.contains("unreadable") => {}
            other => wrong.push(format!("{encoding:?}: unreadable round row {other:?}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// E-CRCAP.W2: the 5-round review schedule (UC-REV-3).

fn ok(o: Output) -> Output {
    assert!(o.status.success(), "{}", text(&o));
    o
}

fn refused(o: &Output, needles: &[&str]) {
    let t = text(o);
    assert!(!o.status.success() && t.contains("fix ->"), "expected a refusal with a fix line: {t}");
    for n in needles {
        assert!(t.contains(n), "expected '{n}': {t}");
    }
}

fn test_note(p: &Path) {
    ok(run(env!("CARGO_BIN_EXE_keel"), p, &["notes", "write", "test"], WU_TESTS));
}

fn doc_note(p: &Path) {
    ok(keel(p, &["notes", "write", "doc"]));
}

fn head(p: &Path) -> String {
    git(p, &["rev-parse", "HEAD"])
}

/// A fix of the last round: a new code commit on HEAD with its test note, ready for preflight.
fn step(p: &Path, n: u32) -> String {
    write(p, "src/a.py", &format!("def a():\n    return {n}\n"));
    commit(p, &format!("fix {n}"));
    test_note(p);
    head(p)
}

fn preflight(p: &Path) -> Output {
    keel(p, &["preflight", "wu", "wu/e/a"])
}

fn base(p: &Path) -> String {
    git(p, &["merge-base", "epic/e", "HEAD"])
}

fn whole(n: u32, of: u32, base: &str, tip: &str) -> String {
    format!("review round {n} of {of} - scope: the whole range, {base}..{tip}")
}

fn last_fix(n: u32, of: u32, since: &str, tip: &str) -> String {
    format!("review round {n} of {of} - scope: the last fix, {since}..{tip}")
}

/// The history row of `sha` in a sign-off's output.
fn row(t: &str, sha: &str) -> String {
    t.lines().find(|l| l.trim_start().starts_with(&sha[..10])).map(String::from).unwrap_or_else(|| panic!("no history row for {}: {t}", &sha[..10]))
}

#[test]
fn preflight_names_the_round_and_its_scope_the_whole_range_then_the_last_fix() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    let (b, c1) = (base(p), head(p));
    assert!(text(&ok(preflight(p))).contains(&whole(1, 5, &b, &c1)), "round 1");
    ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    // A re-run on the same commit is the same round.
    assert!(text(&ok(preflight(p))).contains(&whole(1, 5, &b, &c1)), "a re-run is round 1 again");
    ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["round"].as_u64(), n["scope"].as_str()), (Some(1), Some("full")), "{n}");

    let c2 = step(p, 2);
    assert!(text(&ok(preflight(p))).contains(&whole(2, 5, &b, &c2)), "round 2 still reviews the whole range");
    ok(write_cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    assert_eq!((note(p, "cr", "HEAD")["round"].as_u64(), note(p, "cr", "HEAD")["scope"].as_str()), (Some(2), Some("full")));

    let c3 = step(p, 3);
    let t = text(&ok(preflight(p)));
    assert!(t.contains(&last_fix(3, 5, &c2, &c3)) && !t.contains("whole range"), "round 3 reviews only the last fix: {t}");
    ok(write_cr(p, &["--verdict", "PASS"]));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["round"].as_u64(), n["scope"].as_str()), (Some(3), Some(format!("since {c2}").as_str())), "{n}");
    let full = |a: &str, b: &str| git(p, &["diff", "--no-color", "--no-ext-diff", "--no-textconv", a, b, "--", "src/", "tests/"]);
    let hash = run("git", p, &["hash-object", "--stdin"], &full(&b, &c3));
    assert_eq!(n["code_diff_hash"].as_str(), Some(String::from_utf8_lossy(&hash.stdout).trim()), "a narrow round still binds the whole range");
}

#[test]
fn a_fix_pass_after_a_nits_only_round_is_accepted_by_the_merge_check_and_listed_at_sign_off() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    doc_note(p);
    let nits = r#"[{"location": "src/a.py:1", "finding": "a() could be named one()", "fixed": "renamed, in a fix-PASS"}]"#;
    ok(write_cr(p, &["--verdict", "PASS", "--nits", nits]));
    let c1 = head(p);
    assert_eq!(note(p, "cr", "HEAD")["nits"][0]["finding"], "a() could be named one()");
    write(p, "src/a.py", "def one():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_one():\n    pass\n");
    commit(p, "fix the nit");
    test_note(p);
    doc_note(p);
    let c2 = head(p);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", "HEAD"]), &["fixes"]);
    let o = ok(write_cr(p, &["--verdict", "PASS", "--fixes", &c1[..12]]));
    assert!(!text(&o).contains("review round"), "a fix-PASS is no round: {}", text(&o));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["verdict"].as_str(), n["fixes"].as_str(), n["round"].as_u64()), (Some("PASS"), Some(c1.as_str()), Some(1)), "{n}");
    assert_eq!((n["answers"]["importants"].as_u64(), n["answers"]["nits"].as_u64()), (Some(0), Some(1)), "{n}");
    let fix = git(p, &["diff", "--no-color", "--no-ext-diff", "--no-textconv", &c1, &c2, "--", "src/", "tests/"]);
    let hash = run("git", p, &["hash-object", "--stdin"], &fix);
    assert_eq!(n["fix_diff_hash"].as_str(), Some(String::from_utf8_lossy(&hash.stdout).trim()), "the fix's own code hash");

    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    assert_red(&o, "8·manual-review");
    assert!(text(&o).contains("- 1 failed"), "the merge check reads a fix-PASS as a PASS: {}", text(&o));
    let o = ok(keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"]));
    let t = text(&o);
    assert!(row(&t, &c2).contains(&format!("[fix-PASS of round 1 on {}; no reviewers ran; changed: src/a.py, tests/test_a.py]", &c1[..10])), "{t}");
    assert!(row(&t, &c1).contains("[round 1, full]"), "{t}");
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], ""));
}

#[test]
fn a_fix_pass_answers_only_findings_that_need_no_new_round() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    ok(write_cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    let c1 = head(p);
    step(p, 2);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c1]), &["round 1", "importants"]);
    assert!(!has_note(p, "cr", "HEAD"), "a refused fix-PASS writes nothing");
    ok(write_cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    let c2 = head(p);
    step(p, 3);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c2]), &["round 2", "importants"]);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c1]), &["latest"]);
    ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1", "--importants", "1"]));
    let c3 = head(p);
    step(p, 4);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c3]), &["blocker"]);
    ok(write_cr(p, &["--verdict", "FAIL", "--importants", "2"]));
    let c4 = head(p);
    let c5 = step(p, 5);

    // Round 4 found no blocker: there is no round 5, and its importants are fixed under a fix-PASS.
    let o = preflight(p);
    assert_red(&o, "0·review-cap");
    assert!(text(&o).contains("round 5") && text(&o).contains("round 4"), "{}", text(&o));
    refused(&write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]), &["round 5"]);
    assert!(!has_note(p, "cr", "HEAD"));
    ok(write_cr(p, &["--verdict", "PASS", "--fixes", &c4]));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["fixes"].as_str(), n["round"].as_u64(), n["answers"]["importants"].as_u64()), (Some(c4.as_str()), Some(4), Some(2)), "{n}");

    // A second fix needs a round.
    step(p, 6);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c4]), &["fix-PASS", &c5[..10]]);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c5]), &["latest"]);
    assert!(!has_note(p, "cr", "HEAD"));
}

#[test]
fn round_5_follows_only_a_round_4_blocker_round_6_never_runs_and_a_round_5_blocker_ends_in_the_cap_waiver() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    doc_note(p);
    let mut tips = vec![head(p)];
    for n in 1..=4 {
        ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
        if n == 4 {
            let o = write_cr(p, &["--verdict", "WAIVED"]);
            refused(&o, &["review cap", "round 4 of 5"]);
            assert_eq!(note(p, "cr", "HEAD")["verdict"], "FAIL", "below the cap no waiver is written");
        }
        tips.push(step(p, n + 1));
        doc_note(p);
    }
    assert!(text(&ok(preflight(p))).contains(&last_fix(5, 5, &tips[3], &tips[4])));
    let findings = r#"[{"location": "src/a.py:2", "claim": "a() returns 5", "severity": "blocker", "evidence": "e"},
        {"location": "src/a.py:1", "claim": "a() has no contract", "severity": "blocker", "evidence": "e"},
        {"location": "src/a.py:1", "claim": "named badly", "severity": "important", "evidence": "e", "waived": "owner: fine"}]"#;
    let o = ok(write_cr(p, &["--verdict", "FAIL", "--findings", findings]));
    assert!(text(&o).contains("review cap reached") && text(&o).contains("keel notes write cr --verdict WAIVED"), "{}", text(&o));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["blockers"].as_u64(), n["importants"].as_u64(), n["round"].as_u64()), (Some(2), Some(0), Some(5)), "counts come from the findings: {n}");
    ok(write_cr(p, &["--verdict", "WAIVED"]));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["verdict"].as_str(), n["waiver"].as_str(), n["blockers"].as_u64(), n["round"].as_u64()), (Some("WAIVED"), Some("review cap: 5 rounds, 2 blockers open"), Some(2), Some(5)), "{n}");
    assert_eq!(n["findings"][0]["claim"], "a() returns 5", "the round's findings stay on the waiver: {n}");

    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    assert_red(&o, "8·manual-review");
    assert!(text(&o).contains("WAIVED 3·review: review cap: 5 rounds, 2 blockers open") && text(&o).contains("- 1 failed"), "{}", text(&o));
    let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"]);
    refused(&o, &["review cap: 5 rounds, 2 blockers open", "--accept-waiver", "blocker src/a.py:2: a() returns 5"]);
    let t = text(&o);
    assert!(row(&t, &tips[4]).contains(&format!("[round 5, since {}]", &tips[3][..10])), "{t}");
    assert!(row(&t, &tips[2]).contains(&format!("[round 3, since {}]", &tips[1][..10])), "{t}");
    assert!(row(&t, &tips[1]).contains("[round 2, full]") && row(&t, &tips[0]).contains("[round 1, full]"), "{t}");
    sign_waived(p, "wu", "docs/a.html");
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));

    // Round 6 never runs, not even under a higher cap the branch commits for itself.
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &format!("{cfg}[review]\nmax_rounds = 9\n"));
    commit(p, "a cap of the branch's own");
    step(p, 6);
    let o = preflight(p);
    assert_red(&o, "0·review-cap");
    assert!(text(&o).contains("round 6"), "{}", text(&o));
    refused(&write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]), &["round 6"]);
    assert!(!has_note(p, "cr", "HEAD"));
}

#[test]
fn max_rounds_on_the_target_moves_the_cap() {
    let d = fixture();
    let p = d.path();
    git(p, &["switch", "-q", "epic/e"]);
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    write(p, ".keel/config.toml", &format!("{cfg}[review]\nmax_rounds = 6\n"));
    commit(p, "the target's cap");
    git(p, &["switch", "-q", "wu/e/a"]);
    test_note(p);
    assert!(text(&ok(preflight(p))).contains("review round 1 of 6"));
    for n in 1..=5 {
        ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
        step(p, n + 1);
    }
    assert!(text(&ok(preflight(p))).contains("review round 6 of 6"));
    let o = ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    assert!(text(&o).contains("review cap reached"), "{}", text(&o));
    ok(write_cr(p, &["--verdict", "WAIVED"]));
    assert_eq!(note(p, "cr", "HEAD")["waiver"], "review cap: 6 rounds, 1 blockers open");
}

#[test]
fn a_merged_wus_rounds_do_not_count_for_its_epic() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    doc_note(p);
    ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    step(p, 2);
    doc_note(p);
    ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    step(p, 3);
    doc_note(p);
    ok(write_cr(p, &["--verdict", "PASS"]));
    assert_eq!(note(p, "cr", "HEAD")["round"], 3);
    sign(p, "wu", "docs/a.html");
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], ""));
    write(p, "docs/result.html", "<p>result</p>");
    commit(p, "close epic");
    ok(run(env!("CARGO_BIN_EXE_keel"), p, &["notes", "write", "test"], EPIC_TESTS));
    let o = ok(keel(p, &["preflight", "epic", "epic/e"]));
    assert!(text(&o).contains(&whole(1, 5, &git(p, &["merge-base", "main", "HEAD"]), &head(p))), "{}", text(&o));
    ok(write_cr(p, &["--verdict", "PASS"]));
    assert_eq!(note(p, "cr", "HEAD")["round"], 1);
}

#[test]
fn the_writer_records_the_rounds_lists_and_takes_the_counts_from_its_findings() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    let findings = r#"[{"location": "src/a.py:2", "claim": "wrong", "severity": "blocker", "evidence": "e"},
        {"location": "src/a.py:1", "claim": "costly", "severity": "important", "evidence": "e", "waived": "owner: tracked as B9"}]"#;
    let refuted = r#"[{"location": "src/a.py:1", "claim": "slow", "why": "src/a.py:1 is O(1)"}]"#;
    let nits = r#"[{"location": "src/a.py:1", "finding": "naming", "waived": "taste"}]"#;
    let fleet = r#"{"dimensions": ["correctness", "efficiency"], "finders": 2}"#;
    for (args, needle) in [
        (&["--verdict", "PASS", "--findings", findings][..], "blocker"),
        (&["--verdict", "FAIL", "--findings", findings, "--blockers", "0"], "--blockers"),
        (&["--verdict", "FAIL", "--findings", findings, "--importants", "1"], "--importants"),
        (&["--verdict", "FAIL", "--findings", "\"prose\""], "--findings"),
        (&["--verdict", "FAIL", "--refuted", "{}"], "--refuted"),
        (&["--verdict", "FAIL", "--nits", "[1"], "--nits"),
        (&["--verdict", "FAIL", "--fleet", "[]"], "--fleet"),
        (&["--verdict", "FAIL", "--findings", r#"[{"claim": "x", "severity": "nit"}]"#], "severity"),
        (&["--verdict", "FAIL", "--findings", "--nits", "[]"], "--findings"),
    ] {
        let o = write_cr(p, args);
        assert!(!o.status.success() && text(&o).contains(needle), "{args:?}: {}", text(&o));
        assert!(!has_note(p, "cr", "HEAD"), "{args:?} wrote a note");
    }
    ok(write_cr(p, &["--verdict", "FAIL", "--findings", findings, "--refuted", refuted, "--nits", nits, "--fleet", fleet, "--blockers", "1"]));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["blockers"].as_u64(), n["importants"].as_u64()), (Some(1), Some(0)), "a waived important is not open: {n}");
    assert_eq!((n["findings"][1]["waived"].as_str(), n["refuted"][0]["why"].as_str(), n["nits"][0]["finding"].as_str(), n["fleet"]["finders"].as_u64()),
        (Some("owner: tracked as B9"), Some("src/a.py:1 is O(1)"), Some("naming"), Some(2)), "{n}");

    // A fix-PASS ran no reviewers: it records none of the lists.
    step(p, 2);
    ok(write_cr(p, &["--verdict", "PASS"]));
    let c2 = head(p);
    step(p, 3);
    for list in ["--findings", "--refuted", "--nits"] {
        refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c2, list, "[]"]), &[list]);
    }
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c2, "--fleet", "{}"]), &["--fleet"]);
}

#[test]
fn an_unreadable_earlier_note_stops_the_schedule_and_shows_in_the_history() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", "{not json", "HEAD"]);
    let c1 = head(p);
    step(p, 2);
    let o = preflight(p);
    assert_red(&o, "0·review-cap");
    assert!(text(&o).contains(&c1[..10]) && text(&o).contains("unreadable"), "{}", text(&o));
    refused(&write_cr(p, &["--verdict", "PASS"]), &[&c1[..10], "unreadable"]);
    assert!(!has_note(p, "cr", "HEAD"));

    // A note git lists but cannot print (its blob is gone) is an unreadable row, never dropped.
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", r#"{"lost": true}"#, &c1]);
    let blob = git(p, &["notes", "--ref=cr", "list", &c1]);
    std::fs::remove_file(p.join(".git/objects").join(&blob[..2]).join(&blob[2..])).unwrap();
    doc_note(p);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING]);
    assert!(row(&text(&o), &c1).contains("unreadable"), "{}", text(&o));
}

#[test]
fn a_cr_note_whose_fixes_names_no_full_commit_is_unreadable_and_never_reaches_git() {
    // Code review round 1, B1: a note's `fixes` went unchecked into `git diff`, so `--output=<path>`
    // made every sign-off create or truncate that file.
    for bad in ["--output=.keel/config.toml", "HEAD~1", " "] {
        let d = fixture();
        let p = d.path();
        test_note(p);
        let c1 = head(p);
        let forged = json!({"schema": "cr/v1", "tip": c1, "base": base(p), "branch": null, "committer": "x", "timestamp": 0,
            "verdict": "PASS", "blockers": 0, "importants": 0, "code_diff_hash": "h", "fixes": bad});
        git(p, &["notes", "--ref=cr", "add", "-f", "-m", &forged.to_string(), &c1]);
        step(p, 2);
        doc_note(p);
        let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
        let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING]);
        assert_eq!(std::fs::read_to_string(p.join(".keel/config.toml")).unwrap(), cfg, "fixes {bad:?} reached git: {}", text(&o));
        assert!(row(&text(&o), &c1).contains("unreadable"), "fixes {bad:?}: {}", text(&o));
        let o = preflight(p);
        assert_red(&o, "0·review-cap");
        assert!(text(&o).contains(&c1[..10]) && text(&o).contains("unreadable"), "fixes {bad:?}: {}", text(&o));
    }
}

#[test]
fn no_fix_line_names_a_round_the_schedule_refuses() {
    // Code review round 1, B2: after a round-5 blocker a fix-PASS named round 6, which never runs.
    let d = fixture();
    let p = d.path();
    test_note(p);
    let mut tips = vec![head(p)];
    for n in 1..=5 {
        ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
        tips.push(step(p, n + 1));
    }
    let o = write_cr(p, &["--verdict", "PASS", "--fixes", &tips[4]]);
    refused(&o, &["--verdict WAIVED", &tips[4][..10], "--waive-review"]);
    assert!(!text(&o).contains("round 6 on the fix"), "{}", text(&o));

    // B3: a round-4 FAIL with nothing counted. No round 5 follows it and no fix-PASS answers it, so
    // every refusal names the owner's waiver.
    let d = fixture();
    let p = d.path();
    test_note(p);
    for n in 1..=3 {
        ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
        step(p, n + 1);
    }
    let o = ok(write_cr(p, &["--verdict", "FAIL"]));
    assert!(text(&o).contains("--waive-review"), "{}", text(&o));
    let c4 = head(p);
    step(p, 5);
    doc_note(p);
    let o = preflight(p);
    assert_red(&o, "0·review-cap");
    assert!(text(&o).contains("--waive-review") && !text(&o).contains("--fixes"), "{}", text(&o));
    let o = write_cr(p, &["--verdict", "PASS", "--fixes", &c4]);
    refused(&o, &["--waive-review"]);
    assert!(!text(&o).contains("round 5 on the fix"), "{}", text(&o));
}

#[test]
fn a_notes_ref_git_cannot_list_is_an_error_never_no_note() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    doc_note(p);
    let blob = run("git", p, &["hash-object", "-w", "--stdin"], "not a notes tree");
    let blob = String::from_utf8_lossy(&blob.stdout).trim().to_string();
    git(p, &["update-ref", "refs/notes/cr", &blob]);
    let o = keel(p, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html", "--waive-review", RULING]);
    let t = text(&o);
    assert!(!o.status.success() && !t.contains("is WAIVED:") && t.contains("notes --ref=cr list"), "{t}");
    let o = keel(p, &["verify-dod", "wu", "wu/e/a"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("1·notes") && t.contains("cannot be read") && !t.contains("no cr/v1 note"), "{t}");
}

#[test]
fn the_archived_doc_list_ignores_the_signature_lines_git_log_would_print() {
    let d = fixture();
    let p = d.path();
    let k = TempDir::new().unwrap();
    ok(run("ssh-keygen", k.path(), &["-q", "-t", "ed25519", "-N", "", "-f", "signer"], ""));
    let key = k.path().join("signer");
    let public = std::fs::read_to_string(k.path().join("signer.pub")).unwrap();
    std::fs::write(k.path().join("signers"), format!("t@t {}\n", public.split_whitespace().take(2).collect::<Vec<_>>().join(" "))).unwrap();
    for (k_, v) in [("gpg.format", "ssh"), ("user.signingkey", key.to_str().unwrap()), ("gpg.ssh.allowedSignersFile", k.path().join("signers").to_str().unwrap())] {
        git(p, &["config", k_, v]);
    }
    write(p, "docs/work/plan.html", "<p>plan</p>");
    commit(p, "wu plan");
    git(p, &["rm", "-q", "docs/work/plan.html"]);
    write(p, "docs/decisions/d.html", "<p>why</p>");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-S", "-m", "signed: distill + archive the plan"]);
    git(p, &["config", "log.showSignature", "true"]);
    let shown = git(p, &["log", "-1"]);
    assert!(shown.to_lowercase().contains("signature"), "git log prints signature lines here: {shown}");
    ok(keel(p, &["notes", "write", "doc", "--distill", "docs/work/plan.html=docs/decisions/d.html"]));
    let n = note(p, "doc", "HEAD");
    let paths: Vec<&str> = n["changes"].as_array().unwrap().iter().map(|c| c["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["docs/a.html", "docs/decisions/d.html", "docs/work/plan.html"], "{n}");
}

/// Independent text review round 1, I: a later commit that changes no code path since the last cr
/// note needs no round: the fix-PASS carries over it, even on top of a fix-PASS; a code change does not.
#[test]
fn a_commit_that_changes_no_code_since_the_last_cr_note_gets_a_fix_pass_even_on_a_fix_pass() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    ok(write_cr(p, &["--verdict", "PASS", "--nits", r#"[{"location": "src/a.py:1", "finding": "naming", "fixed": "renamed"}]"#]));
    let c1 = head(p);
    step(p, 2);
    ok(write_cr(p, &["--verdict", "PASS", "--fixes", &c1]));
    write(p, "docs/a.html", "<p>a, with its result</p>");
    commit(p, "the result report, after the fix-PASS");
    test_note(p);
    doc_note(p);
    ok(write_cr(p, &["--verdict", "PASS", "--fixes", &c1]));
    let n = note(p, "cr", "HEAD");
    assert_eq!((n["fixes"].as_str(), n["round"].as_u64()), (Some(c1.as_str()), Some(1)), "{n}");
    assert_red(&keel(p, &["verify-dod", "wu", "wu/e/a"]), "8·manual-review");
    sign(p, "wu", "docs/a.html");
    ok(keel(p, &["verify-dod", "wu", "wu/e/a"]));

    // A code change after it is a second fix: it needs a round.
    step(p, 3);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c1]), &["fix-PASS", "second fix"]);
    assert!(!has_note(p, "cr", "HEAD"));
}

/// A failed round stays failed: a docs-only commit after it fixes nothing, so it gets no PASS.
#[test]
fn a_commit_that_changes_no_code_after_a_failed_round_gets_no_fix_pass_it_could_not_get_anyway() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    ok(write_cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    let c1 = head(p);
    write(p, "docs/a.html", "<p>a, reworded</p>");
    commit(p, "docs only");
    test_note(p);
    refused(&write_cr(p, &["--verdict", "PASS", "--fixes", &c1]), &["round 1", "importants"]);
}

/// Independent text review round 2, I1: from round 4 a second code fix after a fix-PASS has no round
/// to go to, so the refusal names the owner's waiver, never a round preflight would refuse.
#[test]
fn a_second_code_fix_after_a_round_4_fix_pass_points_at_the_owners_waiver_not_a_refused_round() {
    let d = fixture();
    let p = d.path();
    test_note(p);
    for n in 1..=3 {
        ok(write_cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
        step(p, n + 1);
    }
    ok(write_cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    let r4 = head(p);
    step(p, 5);
    ok(write_cr(p, &["--verdict", "PASS", "--fixes", &r4]));
    step(p, 6);
    let o = write_cr(p, &["--verdict", "PASS", "--fixes", &r4]);
    refused(&o, &["second fix", "--waive-review"]);
    assert!(!text(&o).contains("run review round 5"), "round 5 does not exist here: {}", text(&o));
    // Preflight's refusal names the same way out, never the fix-PASS the writer refuses here.
    let o = preflight(p);
    assert_red(&o, "0·review-cap");
    assert!(text(&o).contains("--waive-review") && !text(&o).contains("--fixes"), "{}", text(&o));
}
