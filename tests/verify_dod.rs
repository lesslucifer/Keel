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
