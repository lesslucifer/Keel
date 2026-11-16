//! E-GHFLOW.W1: the Keel gate workflow's contract - what starts it, what it may touch, and that each
//! job is one call of scripts/ci-gate.sh. Read line by line, like `release.rs` reads release.yml.
compiled!();

use std::path::Path;

pub(crate) fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

fn yml() -> String {
    read(".github/workflows/keel-gate.yml")
}

/// The lines of the top-level `key:` block, up to the next top-level key.
pub(crate) fn block(yml: &str, key: &str) -> Vec<String> {
    let header = format!("{key}:");
    let mut lines = yml.lines().skip_while(|l| *l != header);
    assert!(lines.next().is_some(), "no top-level {key}");
    lines.take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('#')).filter(|l| !l.trim().is_empty()).map(String::from).collect()
}

/// The lines of job `name`, up to the next job.
pub(crate) fn job(yml: &str, name: &str) -> Vec<String> {
    let header = format!("  {name}:");
    let mut lines = yml.lines().skip_while(|l| *l != header);
    assert!(lines.next().is_some(), "job {name} missing");
    lines.take_while(|l| l.is_empty() || l.starts_with("   ")).filter(|l| !l.trim().is_empty()).map(String::from).collect()
}

pub(crate) fn jobs(yml: &str) -> Vec<String> {
    block(yml, "jobs")
        .iter()
        .filter(|l| l.starts_with("  ") && !l.starts_with("   ") && l.ends_with(':'))
        .map(|l| l.trim().trim_end_matches(':').to_string())
        .collect()
}

/// The env every job hands the script: the pull request's base and both repositories.
const BASE_ENV: [&str; 3] = [
    "          BASE_BRANCH: ${{ github.event.pull_request.base.ref }}",
    "          BASE_SHA: ${{ github.event.pull_request.base.sha }}",
    "          REPO: ${{ github.repository }}",
];
/// What each judging job adds: the build job's keel and its hash.
const KEEL_ENV: [&str; 2] = ["          KEEL: ${{ runner.temp }}/keel-bin/keel", "          KEEL_SHA256: ${{ needs.build.outputs.keel_sha256 }}"];

#[test]
fn block_and_job_end_at_the_next_key() {
    let y = "on:\n  push:\n\n    branches: [a]\njobs:\n  a:\n    x: 1\n  b:\n    y: 2\n";
    assert_eq!(block(y, "on"), ["  push:", "    branches: [a]"]);
    assert_eq!(job(y, "a"), ["    x: 1"]);
    assert_eq!(jobs(y), ["a", "b"]);
}

/// E-GHFLOW2.W4: GitHub takes a pull_request_target workflow from main for every base, so no pull
/// request changes what judges it; a work branch's push runs nothing here (its pull request does).
#[test]
fn it_runs_from_main_on_pull_requests_into_main_or_an_epic_and_on_a_push_of_main_only() {
    let y = yml();
    assert_eq!(block(&y, "on"), ["  push:", "    branches:", "      - main", "  pull_request_target:", "    branches:", "      - main", "      - 'epic/**'"]);
    for never in ["wu/**", "tags", "  pull_request:", "workflow_run", "workflow_dispatch", "release:", "issue_comment", "schedule", "merge_group"] {
        assert!(!y.contains(never), "keel-gate.yml mentions {never}");
    }
}

#[test]
fn its_token_reads_contents_only_and_it_uses_no_secret() {
    let y = yml();
    assert_eq!(block(&y, "permissions"), ["  contents: read"]);
    assert_eq!(y.matches("permissions:").count(), 1, "no job widens the token");
    assert!(!y.contains("secrets."), "no secret");
    assert!(!y.contains("GITHUB_TOKEN") && !y.contains("github.token"), "the token is never handed to a step");
}

/// github.ref is main for every pull_request_target run, so the group is the pull request's number.
#[test]
fn a_superseded_pull_request_run_is_cancelled_but_every_main_commit_keeps_its_run() {
    assert_eq!(
        block(&yml(), "concurrency"),
        [
            "  group: keel-gate-${{ github.event_name }}-${{ github.event.pull_request.number || github.sha }}",
            "  cancel-in-progress: ${{ github.event_name == 'pull_request_target' }}"
        ]
    );
}

/// Every checkout is main's commit (the workflow's own), never the head's; the head is git data.
#[test]
fn every_action_is_pinned_to_the_dist_commit_and_every_checkout_is_mains_commit_keeping_no_credential() {
    let y = yml();
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    let pin = |a: &str| format!("{a}@{}", dist["dist"]["github-action-commits"][a].as_str().unwrap());
    let (checkout, upload, download) = (pin("actions/checkout"), pin("actions/upload-artifact"), pin("actions/download-artifact"));
    let uses = |name: &str| -> Vec<String> { job(&y, name).iter().filter_map(|l| l.trim().strip_prefix("- uses: ")).map(String::from).collect() };
    assert_eq!(uses("build"), [checkout.clone(), upload]);
    for name in ["ci", "gate", "docs"] {
        assert_eq!(uses(name), [checkout.clone(), download.clone()], "{name}");
    }
    for p in [&checkout, &pin("actions/upload-artifact"), &download] {
        let sha = p.rsplit('@').next().unwrap();
        assert!(sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{p}");
    }
    assert_eq!(y.matches("persist-credentials: false").count(), 4);
    assert_eq!(y.matches("fetch-depth: 0").count(), 4);
    let refs: Vec<&str> = y.lines().filter(|l| l.trim_start().starts_with("ref:")).collect();
    assert_eq!(refs, ["          ref: ${{ github.sha }}"; 4], "main's commit, never the head's");
    for never in ["repository:", "refs/pull/", "token:", "allow-unsafe-pr-checkout"] {
        assert!(!y.contains(never), "{never}");
    }
}

#[test]
fn no_run_line_holds_an_expression_and_event_values_come_through_env() {
    let y = yml();
    for l in y.lines().filter(|l| l.trim_start().starts_with("- run:") || l.trim_start().starts_with("run:")) {
        assert!(!l.contains("${{"), "an expression in a run line: {l}");
        assert!(!l.trim_end().ends_with('|') && !l.trim_end().ends_with('>'), "no multi-line run block: {l}");
    }
    let has = |name: &str, env: &str| assert!(job(&y, name).iter().any(|l| l == env), "{name} lacks {env}");
    for name in ["build", "ci"] {
        has(name, "          HEAD_BRANCH: ${{ github.event.pull_request.head.ref || github.ref_name }}");
        has(name, "          HEAD_SHA: ${{ github.event.pull_request.head.sha || github.sha }}");
        has(name, "          HEAD_REPO: ${{ github.event.pull_request.head.repo.full_name || github.repository }}");
    }
    for name in ["gate", "docs"] {
        has(name, "          HEAD_BRANCH: ${{ github.event.pull_request.head.ref }}");
        has(name, "          HEAD_SHA: ${{ github.event.pull_request.head.sha }}");
        has(name, "          HEAD_REPO: ${{ github.event.pull_request.head.repo.full_name }}");
    }
    for name in ["build", "ci", "gate", "docs"] {
        for env in BASE_ENV {
            has(name, env);
        }
    }
    for name in ["ci", "gate", "docs"] {
        for env in KEEL_ENV {
            has(name, env);
        }
    }
    has("build", "          KEEL_OUT: ${{ runner.temp }}/keel-bin");
}

/// keel/ci, keel/gate and keel/docs: names no workflow before W4 used, so an old workflow on an old
/// branch, running its head's own script, can never report a required check.
#[test]
fn four_jobs_the_three_judges_named_keel_each_one_call_of_the_ci_script_on_ubuntu_24_04() {
    let y = yml();
    assert_eq!(jobs(&y), ["build", "ci", "gate", "docs"]);
    for name in ["build", "ci", "gate", "docs"] {
        let j = job(&y, name);
        let runs: Vec<&str> = j.iter().filter_map(|l| l.trim().strip_prefix("run: ").or_else(|| l.trim().strip_prefix("- run: "))).collect();
        let want: Vec<String> = if name == "ci" {
            vec!["rustup component add rustfmt clippy".into(), "scripts/ci-gate.sh ci".into()]
        } else {
            vec![format!("scripts/ci-gate.sh {name}")]
        };
        assert_eq!(runs, want, "{name}");
        assert!(j.iter().any(|l| l == "    runs-on: ubuntu-24.04"), "{name}");
        assert!(j.iter().any(|l| l.starts_with("    timeout-minutes: ")), "{name}");
        for never in ["continue-on-error", "permissions", "secrets", "services:", "container:"] {
            assert!(!j.iter().any(|l| l.contains(never)), "{name}: {never}");
        }
        if name != "build" {
            assert!(j.iter().any(|l| *l == format!("    name: keel/{name}")), "{name} reports keel/{name}");
            assert!(j.iter().any(|l| l == "    needs: build"), "{name}");
        }
    }
    let ifs: Vec<String> = ["build", "ci", "gate", "docs"].iter().flat_map(|n| job(&y, n).into_iter().filter(|l| l.starts_with("    if:"))).collect();
    assert_eq!(
        ifs,
        [
            "    if: ${{ !cancelled() }}",
            "    if: ${{ !cancelled() && github.event_name == 'pull_request_target' }}",
            "    if: ${{ !cancelled() && github.event_name == 'pull_request_target' }}",
        ],
        "no job skips a pull request (a skipped job counts as passed): a failed build or a fork is red in each"
    );
    let b = job(&y, "build");
    for l in [
        "    outputs:",
        "      keel_sha256: ${{ steps.build.outputs.keel_sha256 }}",
        "      - id: build",
        "          retention-days: 1",
        "          if-no-files-found: error",
    ] {
        assert!(b.iter().any(|x| x == l), "build lacks {l}");
    }
    assert!(!y.contains("name: ci\n") && !y.contains("name: gate\n") && !y.contains("name: docs\n"));
}

/// No workflow reads or writes a cache: a pull_request_target run's cache is the default branch's,
/// and head code in keel/ci could write one. Only keel-gate.yml runs on pull_request_target, and
/// release.yml no longer runs on a pull request (its token can write; DEC-38 amended).
#[test]
fn no_workflow_uses_a_cache_and_only_the_gate_runs_on_pull_request_target() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut seen = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let y = std::fs::read_to_string(&p).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        seen += 1;
        for never in ["actions/cache", "rust-cache", "cache-dependency-path", "save-always"] {
            assert!(!y.contains(never), "{name} uses {never}");
        }
        assert!(!y.lines().any(|l| l.trim_start().starts_with("cache:")), "{name} has a cache: input");
        assert_eq!(y.contains("pull_request_target"), name == "keel-gate.yml", "{name}");
    }
    assert!(seen >= 5, "{seen} workflows read");
    let release = read(".github/workflows/release.yml");
    assert!(!release.lines().any(|l| l == "  pull_request:"), "release.yml runs on no pull request");
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    assert_eq!(dist["dist"]["pr-run-mode"].as_str(), Some("skip"), "dist generates it so");
}

#[test]
fn the_ci_script_is_executable_and_builds_keel_only_from_the_base() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-gate.sh");
    let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&p).unwrap().permissions());
    assert_eq!(mode & 0o111, 0o111, "workflow steps run it directly");
    let s = read("scripts/ci-gate.sh");
    assert!(s.starts_with("#!/bin/sh\n"));
    assert!(s.contains("\ngit worktree add -q --detach \"$scratch/base\" \"$base_sha\"\n"), "the base worktree is made from the base commit");
    assert_eq!(s.matches("cargo build").count(), 1);
    assert!(
        s.contains("(cd \"$scratch/base\" && CARGO_TARGET_DIR=$scratch/target cargo build --locked --quiet) >&2"),
        "keel is built inside the base worktree"
    );
    assert!(s.contains("merge -q --no-ff --no-edit \"$head_sha\""), "ci merges the head into the base");
    assert!(s.contains("git -C \"$scratch/merge\" -c core.hooksPath=/dev/null"), "with no hook of the clone's running");
    let (check, run) = (s.find("got=$(sha256 \"$KEEL\")").unwrap(), s.find("case $job in\nbuild)").unwrap());
    assert!(check < run, "the hash is checked before any job runs the keel");
}

/// E-GHFLOW2.W3: the sign-off by approval's two workflows. The request runs from the pull request's
/// merge commit (GitHub's rule for pull_request_review), so it holds nothing; the writer runs on
/// workflow_run, from main, and never checks out or builds the head.
fn request_yml() -> String {
    read(".github/workflows/keel-sign-off-request.yml")
}

fn writer_yml() -> String {
    read(".github/workflows/keel-sign-off.yml")
}

#[test]
fn the_sign_off_request_runs_on_a_submitted_review_with_no_permission_and_no_checkout() {
    let y = request_yml();
    assert!(y.contains("\nname: Keel sign-off request\n"));
    assert_eq!(block(&y, "on"), ["  pull_request_review:", "    types:", "      - submitted"]);
    assert!(y.contains("\npermissions: {}\n"), "the request's token can do nothing");
    assert_eq!(y.matches("permissions").count(), 1, "no job widens it");
    assert_eq!(jobs(&y), ["request"]);
    let j = job(&y, "request");
    let steps: Vec<&String> = j.iter().filter(|l| l.trim_start().starts_with("- ")).collect();
    assert_eq!(steps.len(), 1, "one step");
    assert!(steps[0].trim().starts_with("- run: echo "), "{}", steps[0]);
    for never in ["uses:", "${{", "secrets", "env:", "checkout", "GITHUB_TOKEN", "github.token"] {
        assert!(!y.contains(never), "the request holds {never}");
    }
}

#[test]
fn the_writer_runs_from_main_on_the_requests_end_with_three_scoped_permissions() {
    let y = writer_yml();
    assert!(y.contains("\nname: Keel sign-off\n"));
    assert_eq!(block(&y, "on"), ["  workflow_run:", "    workflows:", "      - Keel sign-off request", "    types:", "      - completed"]);
    assert_eq!(
        y.lines().filter(|l| !l.starts_with(' ') && !l.starts_with('#') && l.ends_with(':')).collect::<Vec<_>>(),
        ["on:", "permissions:", "concurrency:", "jobs:"]
    );
    for never in ["pull_request:", "pull_request_target", "issue_comment", "push:", "workflow_dispatch", "schedule"] {
        assert!(!y.contains(never), "the writer is started by {never}");
    }
    assert_eq!(block(&y, "permissions"), ["  contents: write", "  statuses: write", "  pull-requests: read"]);
    assert_eq!(y.matches("permissions").count(), 1, "no job widens the token");
    assert_eq!(
        block(&y, "concurrency"),
        ["  group: keel-sign-off-${{ github.event.workflow_run.head_sha }}", "  cancel-in-progress: false"],
        "one group per head commit, none cancelled: a later review of another commit never cancels a pending approval (security review I5)"
    );
    assert!(!y.contains("secrets."), "no secret");
}

#[test]
fn the_writer_checks_out_main_only_keeps_no_credential_and_is_one_call_of_its_script() {
    let y = writer_yml();
    assert_eq!(jobs(&y), ["sign-off"]);
    let j = job(&y, "sign-off");
    assert!(j.iter().any(|l| l == "    if: github.event.workflow_run.event == 'pull_request_review'"));
    assert!(j.iter().any(|l| l == "    runs-on: ubuntu-24.04"));
    assert!(j.iter().any(|l| l.starts_with("    timeout-minutes: ")));
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    let pin = dist["dist"]["github-action-commits"]["actions/checkout"].as_str().unwrap();
    let uses: Vec<&str> = y.lines().filter_map(|l| l.trim().strip_prefix("- uses: ")).collect();
    assert_eq!(uses, [format!("actions/checkout@{pin}")], "one checkout, pinned to dist's commit");
    let with: Vec<&str> = y.lines().map(str::trim).filter(|l| l.starts_with("ref:")).collect();
    assert_eq!(with, ["ref: ${{ github.event.repository.default_branch }}"], "the default branch, never the head");
    assert!(y.contains("persist-credentials: false") && y.contains("fetch-depth: 0"));
    let runs: Vec<&str> = j.iter().filter_map(|l| l.trim().strip_prefix("- run: ")).collect();
    assert_eq!(runs, ["scripts/ci-sign-off.sh"]);
    for l in y.lines().filter(|l| l.trim_start().starts_with("- run:")) {
        assert!(!l.contains("${{") && !l.trim_end().ends_with('|') && !l.trim_end().ends_with('>'), "{l}");
    }
    let env: Vec<&str> = j.iter().map(|l| l.trim()).skip_while(|l| *l != "env:").skip(1).collect();
    assert_eq!(
        env,
        [
            "GH_TOKEN: ${{ github.token }}",
            "REPO: ${{ github.repository }}",
            "PR_NUMBER: ${{ github.event.workflow_run.pull_requests[0].number }}",
            "RUN_HEAD_SHA: ${{ github.event.workflow_run.head_sha }}",
            "RUN_EVENT: ${{ github.event.workflow_run.event }}"
        ],
        "event values reach the script through env only, and no head branch or ref is named"
    );
    assert!(!y.contains("head_branch") && !y.contains("head.ref") && !y.contains("head_ref"), "no head branch is named anywhere");
}

/// Security review I4: the writer's job holds a write token, so it builds and runs no code a worker
/// can reach - keel comes from this checkout (main, the owner's), never from the base or the head.
#[test]
fn the_writer_script_builds_keel_only_from_main_and_never_builds_the_base_or_the_head() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-sign-off.sh");
    let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&p).unwrap().permissions());
    assert_eq!(mode & 0o111, 0o111, "the workflow step runs it directly");
    let s = read("scripts/ci-sign-off.sh");
    assert!(s.starts_with("#!/bin/sh\n"));
    assert_eq!(s.matches("cargo build").count(), 1);
    assert!(s.contains("\ngit worktree add -q --detach \"$scratch/base\" \"$base_sha\"\n"), "the one worktree is the base's, read as data");
    assert!(s.contains("(cd \"$repo_dir\" && CARGO_TARGET_DIR=$scratch/target cargo build --locked --quiet)"), "keel is built in this checkout, main");
    assert!(s.contains("\nrepo_dir=$(pwd)\n"), "the checkout the workflow made");
    assert_eq!(s.matches("worktree add").count(), 1, "no head worktree");
    let code: String = s.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    for never in ["checkout", "switch", "git reset", "git merge ", "cargo test", "cargo run", "keel check", "cd \"$scratch/base\" &&", "$scratch/base/"] {
        assert!(!code.contains(never), "the writer runs {never}");
    }
    assert!(s.contains("unset GH_TOKEN"), "no child inherits the token");
    assert!(s.contains("\nset -euf\n"), "no word splits into a glob");
    // security review round 2, F9: the main the build logs is read before the script moves any ref
    let read = s.find("\nmain_sha=$(git rev-parse -q --verify 'HEAD^{commit}')").expect("main's sha is read");
    assert!(read < s.find("git update-ref").unwrap(), "read before the first update-ref");
    assert!(s.contains("building keel from main at $main_sha"));
}
