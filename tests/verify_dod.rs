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
