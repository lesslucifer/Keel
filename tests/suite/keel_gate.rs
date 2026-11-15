//! E-GHFLOW.W1: the Keel gate workflow's contract - what starts it, what it may touch, and that each
//! job is one call of scripts/ci-gate.sh. Read line by line, like `release.rs` reads release.yml.
compiled!();

use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

fn yml() -> String {
    read(".github/workflows/keel-gate.yml")
}

/// The lines of the top-level `key:` block, up to the next top-level key.
fn block(yml: &str, key: &str) -> Vec<String> {
    let header = format!("{key}:");
    let mut lines = yml.lines().skip_while(|l| *l != header);
    assert!(lines.next().is_some(), "no top-level {key}");
    lines.take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('#')).filter(|l| !l.trim().is_empty()).map(String::from).collect()
}

/// The lines of job `name`, up to the next job.
fn job(yml: &str, name: &str) -> Vec<String> {
    let header = format!("  {name}:");
    let mut lines = yml.lines().skip_while(|l| *l != header);
    assert!(lines.next().is_some(), "job {name} missing");
    lines.take_while(|l| l.is_empty() || l.starts_with("   ")).filter(|l| !l.trim().is_empty()).map(String::from).collect()
}

fn jobs(yml: &str) -> Vec<String> {
    block(yml, "jobs")
        .iter()
        .filter(|l| l.starts_with("  ") && !l.starts_with("   ") && l.ends_with(':'))
        .map(|l| l.trim().trim_end_matches(':').to_string())
        .collect()
}

const GATED_ONLY: &str = "    if: github.event_name == 'push' && github.ref_name != 'main' || github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository";

#[test]
fn block_and_job_end_at_the_next_key() {
    let y = "on:\n  push:\n\n    branches: [a]\njobs:\n  a:\n    x: 1\n  b:\n    y: 2\n";
    assert_eq!(block(y, "on"), ["  push:", "    branches: [a]"]);
    assert_eq!(job(y, "a"), ["    x: 1"]);
    assert_eq!(jobs(y), ["a", "b"]);
}

#[test]
fn it_runs_on_branch_pushes_and_pull_requests_only_never_on_a_tag() {
    let y = yml();
    assert_eq!(
        block(&y, "on"),
        [
            "  push:",
            "    branches:",
            "      - main",
            "      - 'epic/**'",
            "      - 'wu/**'",
            "  pull_request:",
            "    branches:",
            "      - main",
            "      - 'epic/**'"
        ]
    );
    for never in ["tags", "pull_request_target", "workflow_run", "workflow_dispatch", "release", "issue_comment", "schedule"] {
        assert!(!y.contains(never), "keel-gate.yml mentions {never}");
    }
}

#[test]
fn its_token_reads_contents_only_and_it_uses_no_secret() {
    let y = yml();
    assert_eq!(block(&y, "permissions"), ["  contents: read"]);
    assert_eq!(y.matches("permissions:").count(), 1, "no job widens the token");
    assert!(!y.contains("secrets."), "no secret");
    assert!(!y.contains("GITHUB_TOKEN"), "the token is never handed to a step");
}

#[test]
fn a_superseded_work_branch_run_is_cancelled_but_every_main_commit_keeps_its_run() {
    assert_eq!(
        block(&yml(), "concurrency"),
        [
            "  group: keel-gate-${{ github.event_name }}-${{ github.ref == 'refs/heads/main' && github.sha || github.ref }}",
            "  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}"
        ]
    );
}

#[test]
fn every_action_is_pinned_to_the_dist_commit_and_keeps_no_credential() {
    let y = yml();
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    let pin = dist["dist"]["github-action-commits"]["actions/checkout"].as_str().unwrap();
    let uses: Vec<&str> = y.lines().filter_map(|l| l.trim().strip_prefix("- uses: ")).collect();
    assert_eq!(uses.len(), 3, "one checkout per job");
    for u in &uses {
        assert_eq!(*u, format!("actions/checkout@{pin}"), "pinned to dist's 40-hex commit");
    }
    assert!(pin.len() == 40 && pin.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(y.matches("persist-credentials: false").count(), 3);
    assert_eq!(y.matches("fetch-depth: 0").count(), 3);
}

#[test]
fn no_run_line_holds_an_expression_and_event_values_come_through_env() {
    let y = yml();
    for l in y.lines().filter(|l| l.trim_start().starts_with("- run:") || l.trim_start().starts_with("run:")) {
        assert!(!l.contains("${{"), "an expression in a run line: {l}");
        assert!(!l.trim_end().ends_with('|') && !l.trim_end().ends_with('>'), "no multi-line run block: {l}");
    }
    for name in ["ci", "gate", "docs"] {
        let j = job(&y, name);
        for env in [
            "          HEAD_BRANCH: ${{ github.head_ref || github.ref_name }}",
            "          HEAD_SHA: ${{ github.event.pull_request.head.sha || github.sha }}",
            "          BASE_BRANCH: ${{ github.base_ref }}",
            "          BASE_SHA: ${{ github.event.pull_request.base.sha }}",
        ] {
            assert!(j.iter().any(|l| l == env), "{name} lacks {env}");
        }
    }
}

#[test]
fn three_jobs_each_one_call_of_the_ci_script_on_ubuntu_24_04() {
    let y = yml();
    assert_eq!(jobs(&y), ["ci", "gate", "docs"]);
    for name in ["ci", "gate", "docs"] {
        let j = job(&y, name);
        let runs: Vec<&str> = j.iter().filter_map(|l| l.trim().strip_prefix("- run: ")).collect();
        let want: Vec<String> = if name == "ci" {
            vec!["rustup component add rustfmt clippy".into(), "scripts/ci-gate.sh ci".into()]
        } else {
            vec![format!("scripts/ci-gate.sh {name}")]
        };
        assert_eq!(runs, want, "{name}");
        assert!(j.iter().any(|l| l == "    runs-on: ubuntu-24.04"), "{name}");
        assert!(j.iter().any(|l| l.starts_with("    timeout-minutes: ")), "{name}");
        assert!(!j.iter().any(|l| l.contains("continue-on-error")), "{name}");
    }
    assert!(!job(&y, "ci").iter().any(|l| l.starts_with("    if:")), "ci runs on every event");
    for name in ["gate", "docs"] {
        assert!(job(&y, name).iter().any(|l| l == GATED_ONLY), "{name} skips a push to main and fork pull requests");
    }
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
    assert_eq!(block(&y, "concurrency"), ["  group: keel-sign-off", "  cancel-in-progress: false"], "one writer at a time, none cancelled");
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

#[test]
fn the_writer_script_builds_keel_only_from_the_base_and_never_checks_out_the_head() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-sign-off.sh");
    let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&p).unwrap().permissions());
    assert_eq!(mode & 0o111, 0o111, "the workflow step runs it directly");
    let s = read("scripts/ci-sign-off.sh");
    assert!(s.starts_with("#!/bin/sh\n"));
    assert_eq!(s.matches("cargo build").count(), 1);
    assert!(s.contains("\ngit worktree add -q --detach \"$scratch/base\" \"$base_sha\"\n"), "the one worktree is the base's");
    assert!(s.contains("(cd \"$scratch/base\" && CARGO_TARGET_DIR=$scratch/target cargo build --locked --quiet)"), "keel is built inside the base worktree");
    assert_eq!(s.matches("worktree add").count(), 1, "no head worktree");
    let code: String = s.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    for never in ["checkout", "switch", "git reset", "git merge ", "cargo test", "cargo run", "keel check"] {
        assert!(!code.contains(never), "the writer runs {never}");
    }
    assert!(s.contains("unset GH_TOKEN"), "no child inherits the token");
}
