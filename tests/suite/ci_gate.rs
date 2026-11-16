//! E-GHFLOW.W1: scripts/ci-gate.sh, the gate's CI jobs, on a scratch repo with a bare "origin" and a
//! runner's clone of it, as GitHub's runner sees them. E-GHFLOW2.W4: the build job and its hash, the
//! fork refusal, keel check on the merged tree, and a back-merge of main.
compiled!();

use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

use crate::common;
use crate::router_gh::{commit, text, write};

const KEEL: &str = env!("CARGO_BIN_EXE_keel");
const WU_TESTS: &str = r#"{"integration":{"files":["tests/test_a.py"],"covers":"a works"}}"#;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-gate.sh")
}

fn run(prog: &str, d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut c = common::cmd(prog);
    common::as_tester(c.args(a).current_dir(d).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()));
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn git(d: &Path, a: &[&str]) -> String {
    let o = run("git", d, a, "");
    assert!(o.status.success(), "git {a:?}: {}", text(&o));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

pub(crate) fn keel(d: &Path, a: &[&str], input: &str) -> Output {
    run(KEEL, d, a, input)
}

/// `ci-gate.sh <job>` in the runner's clone `runner` with the env the workflow gives it: the
/// runner's temp dir and step summary under `tmp`, `KEEL` and `PATH` when given, then `env`.
pub(crate) fn run_job(runner: &Path, tmp: &Path, job: &str, env: &[(&str, &str)], keel: Option<&str>, path: Option<&str>) -> Output {
    let mut c = common::cmd("sh");
    c.arg(script()).arg(job).current_dir(runner);
    for k in [
        "HEAD_BRANCH",
        "HEAD_SHA",
        "BASE_BRANCH",
        "BASE_SHA",
        "KEEL",
        "KEEL_SHA256",
        "KEEL_OUT",
        "REPO",
        "HEAD_REPO",
        "GITHUB_OUTPUT",
        "GITHUB_STEP_SUMMARY",
        "RUNNER_TEMP",
        "CARGO_TARGET_DIR",
    ] {
        c.env_remove(k);
    }
    c.env("RUNNER_TEMP", tmp).env("GITHUB_STEP_SUMMARY", tmp.join("summary.md"));
    if let Some(k) = keel {
        c.env("KEEL", k);
    }
    if let Some(p) = path {
        c.env("PATH", p);
    }
    c.envs(env.iter().copied());
    common::hermetic(&mut c);
    c.output().unwrap()
}

/// A developer's repo, its bare origin and a runner's clone, plus a directory where the fixture's
/// `[check]` commands leave a mark if they ever run.
struct Ci {
    _root: TempDir,
    dev: PathBuf,
    runner: PathBuf,
    marks: PathBuf,
    tmp: PathBuf,
}

impl Ci {
    /// main (config, roster, a red ready doc, a doc the WU deletes) → epic/e (a README change) →
    /// wu/e/a (code, a python test, a green doc), all pushed; no notes, no hooks in the dev repo.
    fn new(check_test: &str) -> Ci {
        let root = TempDir::new().unwrap();
        let [dev, origin, runner, marks, tmp] = ["dev", "origin.git", "runner", "marks", "tmp"].map(|n| root.path().join(n));
        for d in [&dev, &marks, &tmp] {
            std::fs::create_dir_all(d).unwrap();
        }
        git(&dev, &["init", "-q", "-b", "main"]);
        git(&dev, &["config", "user.email", "t@t"]);
        git(&dev, &["config", "user.name", "t"]);
        let lint = format!("touch {}", common::sh_quote(&marks.join("lint-ran").to_string_lossy()));
        let test = check_test.replace("{marks}", &common::sh_quote(&marks.to_string_lossy()));
        write(
            &dev,
            ".keel/config.toml",
            &format!(
                "code_paths = [\"src/\", \"tests/\"]\n[check]\nlint = {}\ntest = {}\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
                toml::Value::String(lint),
                toml::Value::String(test)
            ),
        );
        let setup = keel(&dev, &["mr", "setup", "--as", "t"], "");
        assert!(setup.status.success(), "{}", text(&setup));
        write(&dev, "README", "x");
        write(&dev, "docs/old-red.html", READY_RED);
        write(&dev, "docs/gone.html", READY_RED);
        commit(&dev, "base");
        git(&dev, &["switch", "-q", "-c", "epic/e"]);
        write(&dev, "README", "epic");
        commit(&dev, "epic work");
        git(&dev, &["switch", "-q", "-c", "wu/e/a"]);
        write(&dev, "src/a.py", "def a():\n    return 1\n");
        write(&dev, "tests/test_a.py", "def test_a():\n    pass\n");
        write(&dev, "docs/a.html", "<p>a</p>");
        std::fs::remove_file(dev.join("docs/gone.html")).unwrap();
        commit(&dev, "wu a");
        git(root.path(), &["init", "-q", "--bare", origin.to_str().unwrap()]);
        git(&dev, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&dev, &["push", "-q", "--no-verify", "origin", "main", "epic/e", "wu/e/a"]);
        git(root.path(), &["clone", "-q", origin.to_str().unwrap(), runner.to_str().unwrap()]);
        Ci { _root: root, dev, runner, marks, tmp }
    }

    fn sha(&self, rev: &str) -> String {
        git(&self.dev, &["rev-parse", rev])
    }

    /// The test, doc and (with `review`) cr notes on wu/e/a, pushed to origin.
    fn bless(&self, review: bool) {
        git(&self.dev, &["switch", "-q", "wu/e/a"]);
        let mut writes = vec![(vec!["notes", "write", "test"], WU_TESTS), (vec!["notes", "write", "doc"], "")];
        if review {
            writes.push((vec!["notes", "write", "cr", "--verdict", "PASS"], ""));
        }
        for (args, input) in writes {
            let o = keel(&self.dev, &args, input);
            assert!(o.status.success(), "{args:?}: {}", text(&o));
        }
        self.push_notes();
    }

    fn sign(&self) {
        let o = keel(&self.dev, &["mr", "confirm", "wu", "--as", "t", "--report", "docs/a.html"], "");
        assert!(o.status.success(), "mr confirm: {}", text(&o));
        self.push_notes();
    }

    fn push_notes(&self) {
        git(&self.dev, &["push", "-q", "--no-verify", "-f", "origin", "refs/notes/*:refs/notes/*"]);
    }

    fn summary(&self) -> String {
        std::fs::read_to_string(self.tmp.join("summary.md")).unwrap_or_default()
    }

    /// `ci-gate.sh <job>` in the runner's clone with the env the workflow gives it.
    fn job(&self, job: &str, env: &[(&str, &str)], keel: Option<&str>, path: Option<&str>) -> Output {
        let marks = self.marks.to_string_lossy();
        let env: Vec<(&str, &str)> = std::iter::once(("MARKS", marks.as_ref())).chain(env.iter().copied()).collect();
        run_job(&self.runner, &self.tmp, job, &env, keel, path)
    }

    /// A push of `branch` as GitHub's push event names it.
    fn push_job(&self, job: &str, branch: &str) -> Output {
        let sha = self.sha(branch);
        self.job(job, &[("HEAD_BRANCH", branch), ("HEAD_SHA", &sha)], Some(KEEL), None)
    }

    fn checkout(&self, branch: &str) {
        git(&self.runner, &["fetch", "-q", "origin"]);
        git(&self.runner, &["checkout", "-q", "-B", branch, &format!("origin/{branch}")]);
    }

    fn marked(&self) -> Vec<String> {
        std::fs::read_dir(&self.marks).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
    }
}

const READY_RED: &str = r#"<html data-artifact="wu-plan" data-status="ready"><body><p>no items</p></body></html>"#;
const MARK_TEST: &str = "touch {marks}/test-ran";

/// The gate's printout lines: the FAIL blocks and the verdict line, as the local gate prints them.
fn gate_lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .filter(|l| l.starts_with("FAIL") || l.starts_with("      ") || l.starts_with("verify-dod "))
        .map(String::from)
        .collect()
}

pub(crate) fn fails(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stderr).lines().filter_map(|l| l.strip_prefix("FAIL  ")).map(|l| l.split_whitespace().next().unwrap().to_string()).collect()
}

#[test]
fn the_gate_is_green_up_to_the_sign_off_before_it_and_fully_green_after() {
    let ci = Ci::new(MARK_TEST);
    ci.bless(true);
    ci.checkout("wu/e/a");
    assert!(run("git", &ci.runner, &["config", "--get", "core.hooksPath"], "").stdout.is_empty(), "a fresh clone has no hooks path");
    let o = ci.push_job("gate", "wu/e/a");
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(fails(&o), ["8·manual-review"], "{}", text(&o));
    assert!(text(&o).contains("passes; 8·manual-review is judged at the merge"), "{}", text(&o));
    let s = ci.summary();
    assert!(s.starts_with("## Keel gate: wu green on every check before 8·manual-review, which is judged at the merge\n"), "{s}");
    for row in [
        "| 0·hooks | green |",
        "| 1·notes | green |",
        "| 2·manifest | green |",
        "| 3·review | green |",
        "| 4·review&#45;base | green |",
        "| 5·diff&#45;binding | green |",
        "| 6·doc&#45;note | green |",
        "| test | yes | test&#47;v1 · integration 1 tests",
        "| doc | yes |",
        "| cr | yes | cr&#47;v1 · verdict PASS",
        "| mr | - | none |",
        "| dod | - | none |",
    ] {
        assert!(s.contains(row), "{row} missing:\n{s}");
    }
    assert!(!s.contains("| 7·walk"), "a WU has no walk: {s}");
    assert!(s.contains(&format!("over base `{}` (merge-base with epic&#47;e).", &ci.sha("epic/e")[..10])), "the target outside a code span: {s}");
    assert!(ci.marked().is_empty(), "the gate ran a [check] command: {:?}", ci.marked());
    let hooks = git(&ci.runner, &["config", "--get", "core.hooksPath"]);
    assert!(hooks.ends_with("keel-hooks"), "the runner's clone has Keel's hooks: {hooks}");
    assert!(!text(&o).contains("0·hooks"), "{}", text(&o));
    assert_eq!(git(&ci.runner, &["for-each-ref", "--format=%(refname)", "refs/notes/dod"]), "", "no receipt is written on the runner");

    ci.sign();
    std::fs::remove_file(ci.tmp.join("summary.md")).unwrap();
    let o = ci.push_job("gate", "wu/e/a");
    assert!(o.status.success(), "{}", text(&o));
    assert!(fails(&o).is_empty(), "{}", text(&o));
    assert!(text(&o).contains("- every check passes"), "{}", text(&o));
    assert!(!text(&o).contains("judged at the merge"), "{}", text(&o));
    let s = ci.summary();
    assert!(s.starts_with("## Keel gate: wu green on every check\n"), "{s}");
    assert!(s.contains("| mr | yes | mr&#47;v1 · signer t &#40;tier "), "{s}");
    assert!(s.contains("| 8·manual&#45;review | green |"), "{s}");
    assert!(ci.marked().is_empty(), "{:?}", ci.marked());
    assert_eq!(git(&ci.runner, &["for-each-ref", "--format=%(refname)", "refs/notes/dod"]), "", "a fully green run writes no receipt either");
}

#[test]
fn a_missing_note_prints_the_same_fail_block_as_the_local_gate() {
    let ci = Ci::new(MARK_TEST);
    ci.bless(false);
    ci.checkout("wu/e/a");
    let o = ci.push_job("gate", "wu/e/a");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert_eq!(fails(&o), ["1·notes", "8·manual-review"], "{}", text(&o));
    assert!(text(&o).contains("ci gate red on"), "{}", text(&o));
    assert!(ci.summary().starts_with("## Keel gate: wu red\n"), "{}", ci.summary());
    let s = ci.summary();
    assert!(s.contains("| 1·notes | red (1) |"), "{s}");
    for id in ["3·review", "4·review&#45;base", "5·diff&#45;binding"] {
        assert!(s.contains(&format!("| {id} | not run: no readable cr note |")), "a check that never ran is never green: {id}\n{s}");
    }
    assert!(s.contains("| 2·manifest | green |") && s.contains("| 6·doc&#45;note | green |"), "{s}");

    assert!(keel(&ci.dev, &["hooks", "install"], "").status.success());
    let local = keel(&ci.dev, &["verify-dod", "wu", "wu/e/a"], "");
    assert_eq!(local.status.code(), Some(1), "{}", text(&local));
    assert_eq!(gate_lines(&o), gate_lines(&local), "CI:\n{}\nlocal:\n{}", text(&o), text(&local));
    assert!(!gate_lines(&o).is_empty());
}

#[test]
fn a_base_without_keel_ci_gate_runs_its_plain_verify_dod() {
    let ci = Ci::new(MARK_TEST);
    ci.bless(true);
    ci.checkout("wu/e/a");
    let old = ci.tmp.join("old-keel");
    std::fs::write(
        &old,
        format!("#!/bin/sh\ncase \"$1\" in ci) echo 'usage: keel <init | check | verify-dod>' >&2; exit 1 ;; esac\nexec {} \"$@\"\n", common::sh_quote(KEEL)),
    )
    .unwrap();
    std::fs::set_permissions(&old, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let sha = ci.sha("wu/e/a");
    let o = ci.job("gate", &[("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", &sha)], Some(old.to_str().unwrap()), None);
    assert_eq!(o.status.code(), Some(1), "red until signed: {}", text(&o));
    assert_eq!(fails(&o), ["8·manual-review"], "{}", text(&o));
    assert!(text(&o).contains("verify-dod wu red on"), "{}", text(&o));
    assert!(!text(&o).contains("ci gate"), "{}", text(&o));
    assert!(ci.summary().starts_with("## Keel gate: bootstrap\n"), "{}", ci.summary());
}

#[test]
fn an_epic_push_is_judged_at_level_epic_against_main() {
    let ci = Ci::new(MARK_TEST);
    ci.checkout("epic/e");
    let o = ci.push_job("gate", "epic/e");
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(fails(&o), ["8·manual-review"], "{}", text(&o));
    assert!(text(&o).contains(&format!("verify-dod epic red on {}", &ci.sha("epic/e")[..10])), "{}", text(&o));
    let s = ci.summary();
    assert!(s.starts_with("## Keel gate: epic green on every check before 8·manual-review, which is judged at the merge - a docs/infra-only range\n"), "{s}");
    assert!(s.contains(&format!("over base `{}` (merge-base with main)", &ci.sha("main")[..10])), "{s}");
    for ran in ["| 6·doc&#45;note | green |", "| 7·walk | green |", "| 0·hooks | green |"] {
        assert!(s.contains(ran), "the epic layout and walk run on an exempt range (DEC-16): {ran}\n{s}");
    }
    for skipped in ["1·notes", "2·manifest", "3·review", "4·review&#45;base", "5·diff&#45;binding"] {
        assert!(s.contains(&format!("| {skipped} | not run: docs/infra-only range |")), "{skipped}\n{s}");
    }
}

#[test]
fn a_docs_only_wu_runs_only_what_is_never_exempt_and_an_archive_is_judged_on_its_doc_note() {
    let ci = Ci::new(MARK_TEST);
    let dev = &ci.dev;
    git(dev, &["switch", "-q", "epic/e"]);
    write(dev, "docs/work/old.html", "<p>old plan</p>");
    commit(dev, "a T3 doc on the epic");
    git(dev, &["switch", "-q", "-c", "wu/e/docs"]);
    write(dev, "docs/note.html", "<p>note</p>");
    commit(dev, "a T1 doc");
    git(dev, &["switch", "-q", "-c", "wu/e/arch", "epic/e"]);
    git(dev, &["rm", "-q", "docs/work/old.html"]);
    commit(dev, "archive the T3 doc");
    git(dev, &["push", "-q", "--no-verify", "origin", "epic/e", "wu/e/docs", "wu/e/arch"]);
    ci.checkout("wu/e/docs");

    let o = ci.push_job("gate", "wu/e/docs");
    assert!(o.status.success(), "{}", text(&o));
    let s = ci.summary();
    assert!(s.starts_with("## Keel gate: wu green on every check before 8·manual-review, which is judged at the merge - a docs/infra-only range\n"), "{s}");
    for id in ["1·notes", "2·manifest", "3·review", "4·review&#45;base", "5·diff&#45;binding", "6·doc&#45;note"] {
        assert!(s.contains(&format!("| {id} | not run: docs/infra-only range |")), "{id}\n{s}");
    }
    assert!(s.contains("| 0·hooks | green |") && s.contains("| 8·manual&#45;review | waiting"), "{s}");

    std::fs::remove_file(ci.tmp.join("summary.md")).unwrap();
    let o = ci.push_job("gate", "wu/e/arch");
    assert_eq!(o.status.code(), Some(1), "an archive without its doc note: {}", text(&o));
    let s = ci.summary();
    assert!(s.contains("| 1·notes | red (1) |") && s.contains("| 6·doc&#45;note | not run: no readable doc note |"), "{s}");
    assert!(s.contains("| 2·manifest | not run: docs/infra-only range |"), "{s}");

    let o = keel(dev, &["notes", "write", "doc", "--distill", "docs/work/old.html=none: superseded by the fixture"], "");
    assert!(o.status.success(), "{}", text(&o));
    ci.push_notes();
    std::fs::remove_file(ci.tmp.join("summary.md")).unwrap();
    let o = ci.push_job("gate", "wu/e/arch");
    assert!(o.status.success(), "{}", text(&o));
    let s = ci.summary();
    assert!(s.contains("| 1·notes | green |") && s.contains("| 6·doc&#45;note | green |"), "the archive's doc note is read and judged: {s}");
    for id in ["2·manifest", "3·review", "4·review&#45;base", "5·diff&#45;binding"] {
        assert!(s.contains(&format!("| {id} | not run: docs/infra-only range |")), "{id}\n{s}");
    }
}

#[test]
fn the_docs_job_reads_only_the_docs_the_range_changed() {
    let ci = Ci::new(MARK_TEST);
    let o = ci.push_job("docs", "wu/e/a");
    assert!(o.status.success(), "a green changed doc, a deleted one and an unchanged red one: {}", text(&o));
    assert!(ci.summary().contains("## Keel docs: green\n\n1 changed docs"), "{}", ci.summary());
    assert!(!text(&o).contains("old-red") && !text(&o).contains("gone"), "{}", text(&o));

    git(&ci.dev, &["switch", "-q", "wu/e/a"]);
    write(&ci.dev, "docs/work/red plan.html", READY_RED);
    write(&ci.dev, "docs/work/draft.html", r#"<html data-artifact="wu-plan" data-status="draft"><body></body></html>"#);
    commit(&ci.dev, "two docs");
    git(&ci.dev, &["push", "-q", "--no-verify", "origin", "wu/e/a"]);
    git(&ci.runner, &["fetch", "-q", "origin"]);
    let o = ci.push_job("docs", "wu/e/a");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("red plan.html"), "{}", text(&o));
    assert!(!text(&o).contains("draft.html") && !text(&o).contains("old-red"), "{}", text(&o));
    assert!(ci.summary().contains("## Keel docs: red"), "{}", ci.summary());

    let o = ci.push_job("docs", "epic/e");
    assert!(o.status.success(), "{}", text(&o));
    assert!(ci.summary().contains("## Keel docs: no changed doc"), "{}", ci.summary());
    assert!(ci.marked().is_empty(), "{:?}", ci.marked());
}

#[test]
fn the_ci_job_runs_keel_check_and_a_red_test_is_red() {
    let ci = Ci::new(MARK_TEST);
    ci.checkout("wu/e/a");
    let o = ci.push_job("ci", "wu/e/a");
    assert!(o.status.success(), "{}", text(&o));
    let mut marked = ci.marked();
    marked.sort();
    assert_eq!(marked, ["lint-ran", "test-ran"]);
    assert!(ci.summary().starts_with("## Keel check: green"), "{}", ci.summary());

    let red = Ci::new("false");
    red.checkout("wu/e/a");
    let o = red.push_job("ci", "wu/e/a");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(red.summary().starts_with("## Keel check: red"), "{}", red.summary());

    let main = ci.sha("main");
    let o = ci.job("ci", &[("HEAD_BRANCH", "main"), ("HEAD_SHA", &main)], Some(KEEL), None);
    assert!(o.status.success(), "a push to main runs ci: {}", text(&o));
}

/// A `cargo` that records how the script built keel - its arguments, its working directory, the
/// commit there, and every cargo or rustup config file a real cargo would read from that directory
/// up - and makes a keel that records its calls.
fn fake_cargo(ci: &Ci) -> String {
    let shims = ci.tmp.join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    let cargo = r#"#!/bin/sh
printf '%s\n' "$*" > "$MARKS/cargo-args"
pwd -P > "$MARKS/cargo-cwd"
git rev-parse HEAD > "$MARKS/cargo-head"
: > "$MARKS/cargo-configs"
d=$(pwd -P)
while :; do
  for f in .cargo/config.toml .cargo/config rust-toolchain.toml rust-toolchain; do
    [ -e "$d/$f" ] && echo "$d/$f" >> "$MARKS/cargo-configs"
  done
  [ "$d" = / ] && break
  d=$(dirname "$d")
done
mkdir -p "$CARGO_TARGET_DIR/debug"
printf '#!/bin/sh\necho "$*" >> "$MARKS/keel-calls"\n' > "$CARGO_TARGET_DIR/debug/keel"
chmod +x "$CARGO_TARGET_DIR/debug/keel"
"#;
    std::fs::write(shims.join("cargo"), cargo).unwrap();
    std::fs::set_permissions(shims.join("cargo"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    common::path_with(&shims)
}

#[test]
fn without_keel_the_script_builds_one_from_the_base_commit_out_of_the_heads_reach() {
    let ci = Ci::new(MARK_TEST);
    git(&ci.dev, &["switch", "-q", "wu/e/a"]);
    write(&ci.dev, ".cargo/config.toml", "[build]\nrustc-wrapper = \"/tmp/evil\"\n");
    write(&ci.dev, "rust-toolchain.toml", "[toolchain]\nchannel = \"evil\"\n");
    commit(&ci.dev, "a head that would steer cargo and rustup");
    git(&ci.dev, &["push", "-q", "--no-verify", "origin", "wu/e/a"]);
    ci.checkout("wu/e/a");
    let path = fake_cargo(&ci);
    let sha = ci.sha("wu/e/a");
    let o = ci.job("ci", &[("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", &sha)], None, Some(&path));
    assert!(o.status.success(), "{}", text(&o));
    let read = |f: &str| std::fs::read_to_string(ci.marks.join(f)).unwrap();
    assert_eq!(read("cargo-args"), "build --locked --quiet\n");
    assert_eq!(read("cargo-head").trim(), ci.sha("epic/e"), "keel is built from the base, not the head");
    assert!(Path::new(read("cargo-cwd").trim()).starts_with(ci.tmp.canonicalize().unwrap()), "built outside the checkout: {}", read("cargo-cwd"));
    assert_eq!(read("cargo-configs"), "", "no cargo or rustup file of the head is in the build's reach");
    assert_eq!(read("keel-calls"), "check\n");
}

#[test]
fn a_fork_pull_request_from_a_branch_named_main_still_builds_keel_from_its_base() {
    let ci = Ci::new(MARK_TEST);
    let path = fake_cargo(&ci);
    let (head, base) = (ci.sha("wu/e/a"), ci.sha("main"));
    let pr = [("HEAD_BRANCH", "main"), ("HEAD_SHA", head.as_str()), ("BASE_BRANCH", "main"), ("BASE_SHA", base.as_str())];
    let o = ci.job("ci", &pr, None, Some(&path));
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(ci.marks.join("cargo-head")).unwrap().trim(), base, "the base, not the fork's head");
    for job in ["gate", "docs"] {
        let o = ci.job(job, &pr, Some(KEEL), None);
        assert_eq!(o.status.code(), Some(2), "{job}: {}", text(&o));
    }
}

#[test]
fn bad_input_is_refused_before_any_git_call_uses_it() {
    let ci = Ci::new(MARK_TEST);
    let sha = ci.sha("wu/e/a");
    let main = ci.sha("main");
    for (job, branch, head, base) in [
        ("gate", "-x", sha.as_str(), ""),
        ("gate", "wu/e/../a", &sha, ""),
        ("gate", "wu/e/a b", &sha, ""),
        ("gate", "wu/e/a", "abc", ""),
        ("gate", "wu/e/a", &sha, "--upload-pack=x"),
        ("gate", "feature/x", &sha, ""),
        ("gate", "main", &main, ""),
        ("docs", "main", &main, ""),
        ("deploy", "wu/e/a", &sha, ""),
        ("gate", "wu/nope/a", &sha, ""),
    ] {
        let mut env = vec![("HEAD_BRANCH", branch), ("HEAD_SHA", head)];
        if !base.is_empty() {
            env.push(("BASE_BRANCH", base));
        }
        let o = ci.job(job, &env, Some(KEEL), None);
        assert_eq!(o.status.code(), Some(2), "{job} {branch} {head} {base}: {}", text(&o));
        assert!(String::from_utf8_lossy(&o.stderr).starts_with("ci-gate: "), "{}", text(&o));
    }
    assert!(ci.summary().is_empty(), "nothing ran: {}", ci.summary());
}

/// The SHA-256 of a file, as a second tool computes it.
fn sha256(f: &Path) -> String {
    let o = run("shasum", Path::new("/"), &["-a", "256", f.to_str().unwrap()], "");
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8(o.stdout).unwrap()[..64].to_string()
}

/// G4: the build job copies the base's keel out and prints its hash; a job given that hash runs a
/// keel with it - made executable, since an artifact keeps no mode bits - and refuses any other.
#[test]
fn the_build_prints_the_keels_hash_and_every_job_refuses_another_keel() {
    let ci = Ci::new(MARK_TEST);
    let (out, output) = (ci.tmp.join("keel-bin"), ci.tmp.join("github-output"));
    let sha = ci.sha("wu/e/a");
    let o = ci.job(
        "build",
        &[("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", &sha), ("KEEL_OUT", out.to_str().unwrap()), ("GITHUB_OUTPUT", output.to_str().unwrap())],
        Some(KEEL),
        None,
    );
    assert!(o.status.success(), "{}", text(&o));
    let sum = sha256(&out.join("keel"));
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), format!("keel_sha256={sum}"));
    assert_eq!(std::fs::read_to_string(&output).unwrap(), format!("keel_sha256={sum}\n"), "the job output the other jobs read");
    assert_eq!(sum, sha256(Path::new(KEEL)), "the keel it was given, copied whole");
    assert!(ci.summary().contains(&format!("SHA-256 `{sum}`")), "{}", ci.summary());

    let downloaded = ci.tmp.join("downloaded");
    std::fs::create_dir_all(&downloaded).unwrap();
    let k = downloaded.join("keel");
    std::fs::copy(out.join("keel"), &k).unwrap();
    std::fs::set_permissions(&k, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    let base = [("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", sha.as_str())];
    let with = |h: &str| -> Vec<(&str, String)> { base.iter().map(|(a, b)| (*a, b.to_string())).chain([("KEEL_SHA256", h.to_string())]).collect() };
    let job = |env: &[(&str, String)]| {
        let env: Vec<(&str, &str)> = env.iter().map(|(a, b)| (*a, b.as_str())).collect();
        ci.job("docs", &env, Some(k.to_str().unwrap()), None)
    };
    let o = job(&with(&sum));
    assert!(o.status.success(), "the right hash: {}", text(&o));
    let wrong = format!("{}{}", if sum.starts_with('0') { "1" } else { "0" }, &sum[1..]);
    for (h, why) in [(wrong.as_str(), "not the build job's"), ("", "did the build job fail?"), ("abc", "is not a SHA-256")] {
        let o = job(&with(h));
        assert_eq!(o.status.code(), Some(2), "{h:?}: {}", text(&o));
        assert!(text(&o).contains(why), "{h:?}: {}", text(&o));
    }
    std::fs::write(&k, b"#!/bin/sh\necho swapped\n").unwrap();
    let o = job(&with(&sum));
    assert_eq!(o.status.code(), Some(2), "a swapped artifact: {}", text(&o));
    assert!(text(&o).contains("refused"), "{}", text(&o));
    assert!(ci.marked().is_empty(), "{:?}", ci.marked());
}

/// A fork's pull request is refused by every job before any git call: red, never skipped, and no
/// head code runs.
#[test]
fn a_head_from_another_repository_is_refused_by_every_job() {
    let ci = Ci::new(MARK_TEST);
    let sha = ci.sha("wu/e/a");
    let out = ci.tmp.join("out");
    for job in ["build", "ci", "gate", "docs"] {
        for head_repo in [Some("fork/Keel"), Some(""), None] {
            let mut env = vec![("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", sha.as_str()), ("REPO", "owner/Keel"), ("KEEL_OUT", out.to_str().unwrap())];
            if let Some(r) = head_repo {
                env.push(("HEAD_REPO", r));
            }
            let o = ci.job(job, &env, Some(KEEL), None);
            assert_eq!(o.status.code(), Some(2), "{job} {head_repo:?}: {}", text(&o));
            assert!(text(&o).contains("a fork's pull request is never gated here"), "{}", text(&o));
        }
    }
    assert!(ci.summary().is_empty() && ci.marked().is_empty(), "nothing ran: {} {:?}", ci.summary(), ci.marked());
    let o = ci.job("docs", &[("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", &sha), ("REPO", "owner/Keel"), ("HEAD_REPO", "owner/Keel")], Some(KEEL), None);
    assert!(o.status.success(), "the same repository: {}", text(&o));
    let o = ci.job("docs", &[("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", &sha), ("REPO", "-x"), ("HEAD_REPO", "-x")], Some(KEEL), None);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
}

/// DEC-60's ci: keel check on the head merged into the base, as GitHub merges it - the base's newer
/// commits included. A head that does not merge cleanly is red.
#[test]
fn ci_checks_the_head_merged_into_the_base_and_a_conflict_is_red() {
    let ci = Ci::new("test -f epic-later && touch {marks}/merged-tree");
    let dev = &ci.dev;
    git(dev, &["switch", "-q", "epic/e"]);
    write(dev, "epic-later", "x");
    commit(dev, "the epic moves on after wu/e/a left it");
    git(dev, &["push", "-q", "--no-verify", "origin", "epic/e"]);
    git(&ci.runner, &["fetch", "-q", "origin"]);
    let (head, base) = (ci.sha("wu/e/a"), ci.sha("epic/e"));
    let pr = [("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", head.as_str()), ("BASE_BRANCH", "epic/e"), ("BASE_SHA", base.as_str())];
    let o = ci.job("ci", &pr, Some(KEEL), None);
    assert!(o.status.success(), "{}", text(&o));
    assert!(ci.marked().contains(&"merged-tree".to_string()), "the test saw the base's newer file: {:?}", ci.marked());
    assert!(ci.summary().contains(&format!("Head `{head}` merged into base `{base}`")), "{}", ci.summary());

    git(dev, &["switch", "-q", "epic/e"]);
    write(dev, "README", "the epic's");
    commit(dev, "epic README");
    git(dev, &["switch", "-q", "wu/e/a"]);
    write(dev, "README", "the WU's");
    commit(dev, "wu README");
    git(dev, &["push", "-q", "--no-verify", "origin", "epic/e", "wu/e/a"]);
    git(&ci.runner, &["fetch", "-q", "origin"]);
    std::fs::remove_file(ci.tmp.join("summary.md")).unwrap();
    let (head, base) = (ci.sha("wu/e/a"), ci.sha("epic/e"));
    let pr = [("HEAD_BRANCH", "wu/e/a"), ("HEAD_SHA", head.as_str()), ("BASE_BRANCH", "epic/e"), ("BASE_SHA", base.as_str())];
    let o = ci.job("ci", &pr, Some(KEEL), None);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(ci.summary().contains("does not merge cleanly into base"), "{}", ci.summary());
    assert_eq!(git(&ci.runner, &["worktree", "list", "--porcelain"]).matches("worktree ").count(), 1, "no scratch worktree is left");
}

/// A pull request from main into an epic carries only main's commits: gate and docs are green when
/// the head is on origin's main, red when a head named main is not; ci checks the merged tree.
#[test]
fn a_back_merge_of_main_is_green_on_gate_and_docs_only_when_its_head_is_on_main() {
    let ci = Ci::new(MARK_TEST);
    let (main, epic, wu) = (ci.sha("main"), ci.sha("epic/e"), ci.sha("wu/e/a"));
    for job in ["gate", "docs"] {
        std::fs::remove_file(ci.tmp.join("summary.md")).ok();
        let pr = [("HEAD_BRANCH", "main"), ("HEAD_SHA", main.as_str()), ("BASE_BRANCH", "epic/e"), ("BASE_SHA", epic.as_str())];
        let o = ci.job(job, &pr, Some(KEEL), None);
        assert!(o.status.success(), "{job}: {}", text(&o));
        assert!(ci.summary().contains(&format!("## Keel {job}: back-merge of main")), "{}", ci.summary());
        let forged = [("HEAD_BRANCH", "main"), ("HEAD_SHA", wu.as_str()), ("BASE_BRANCH", "epic/e"), ("BASE_SHA", epic.as_str())];
        let o = ci.job(job, &forged, Some(KEEL), None);
        assert_eq!(o.status.code(), Some(1), "{job}: a head named main that is not on main: {}", text(&o));
        assert!(text(&o).contains("is named main but is not on origin's main"), "{}", text(&o));
    }
    let pr = [("HEAD_BRANCH", "main"), ("HEAD_SHA", main.as_str()), ("BASE_BRANCH", "epic/e"), ("BASE_SHA", epic.as_str())];
    let o = ci.job("ci", &pr, Some(KEEL), None);
    assert!(o.status.success(), "{}", text(&o));
    assert!(ci.marked().contains(&"test-ran".to_string()), "{:?}", ci.marked());
    for base in ["main", "wu/e/a"] {
        let o = ci.job("gate", &[("HEAD_BRANCH", "main"), ("HEAD_SHA", &main), ("BASE_BRANCH", base), ("BASE_SHA", &epic)], Some(KEEL), None);
        assert_eq!(o.status.code(), Some(2), "main into {base} is no back-merge: {}", text(&o));
    }
}
