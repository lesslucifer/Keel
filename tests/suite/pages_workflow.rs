//! E-GHFLOW2.W8: the Pages workflow's contract - what starts it, what each job may touch, that the
//! build job runs main's builder on git data with a read token and the deploy job runs no repository
//! code. Read line by line, like `keel_gate.rs` reads keel-gate.yml.
compiled!();

use crate::keel_gate::{block, job, jobs, read};

fn yml() -> String {
    read(".github/workflows/pages.yml")
}

/// The workflow's lines that are not comments, so a word in the header comment proves nothing.
fn code(y: &str) -> Vec<&str> {
    y.lines().filter(|l| !l.trim_start().starts_with('#')).collect()
}

#[test]
fn it_runs_only_after_a_keel_gate_run_ends_never_on_a_push_a_pull_request_or_by_hand() {
    let y = yml();
    assert_eq!(block(&y, "on"), ["  workflow_run:", "    workflows:", "      - Keel gate", "    types:", "      - completed"]);
    let gate_name = read(".github/workflows/keel-gate.yml").lines().find_map(|l| l.strip_prefix("name: ").map(String::from));
    assert_eq!(gate_name.as_deref(), Some("Keel gate"), "the workflow it follows keeps that name");
    for never in
        ["push", "pull_request", "pull_request_target", "pull_request_review", "workflow_dispatch", "issue_comment", "schedule", "tags", "branches", "release"]
    {
        assert!(!code(&y).iter().any(|l| l.trim_start().trim_start_matches("- ").starts_with(&format!("{never}:"))), "pages.yml has a {never} key");
    }
}

#[test]
fn the_build_job_reads_with_contents_read_and_only_the_deploy_job_holds_the_pages_token() {
    let y = yml();
    assert!(code(&y).contains(&"permissions: {}"), "nothing by default");
    assert_eq!(y.matches("permissions:").count(), 3, "the top and one per job");
    let perms = |name: &str| -> Vec<String> {
        job(&y, name).into_iter().skip_while(|l| l != "    permissions:").skip(1).take_while(|l| l.starts_with("      ")).collect()
    };
    assert_eq!(perms("build"), ["      contents: read"]);
    assert_eq!(perms("deploy"), ["      pages: write", "      id-token: write"]);
    assert!(!y.contains("secrets."), "no secret");
    assert!(!y.contains("GITHUB_TOKEN") && !y.contains("github.token"), "the token is never handed to a step");
}

/// The commands of every `run:` key in job lines `j` - a step's first key (`- run: x`) or a later one
/// (`run: x` under `- name: y`) - or `Err` naming a line that mentions run in a form this reader does
/// not parse (a flow mapping `- {run: x}`, a quoted key, `run :`), so no command hides from it.
/// `runs-on:`, the `runner.` context and the fork filter's `event.workflow_run.` are the only other
/// words holding "run" a job may use.
fn runs(j: &[String]) -> Result<Vec<String>, String> {
    let mut out = vec![];
    for l in j {
        let key = l.trim().trim_start_matches("- ").trim_start();
        if let Some(c) = key.strip_prefix("run:") {
            out.push(c.trim().to_string());
        } else if l.replace("runs-on:", "").replace("runner.", "").replace("event.workflow_run.", "").to_ascii_lowercase().contains("run") {
            return Err(l.clone());
        }
    }
    Ok(out)
}

#[test]
fn the_run_reader_sees_a_run_key_wherever_a_step_puts_it() {
    let y = yml();
    let deploy_end = "        uses: actions/deploy-pages@368f82528645a54fb793d4d04e342629a3f51346";
    assert!(y.trim_end().ends_with(deploy_end), "the deploy job is the file's last block");
    let with = |extra: &str| format!("{}\n{extra}", y.trim_end());
    let cases: [(&str, Result<Vec<String>, String>); 6] = [
        ("      - run: curl x", Ok(vec!["curl x".into()])),
        ("      - name: x\n        run: curl x", Ok(vec!["curl x".into()])),
        ("      - name: x\n        shell: sh\n        run: curl x", Ok(vec!["curl x".into()])),
        ("      - {run: curl x}", Err("      - {run: curl x}".into())),
        ("      - name: x\n        \"run\": curl x", Err("        \"run\": curl x".into())),
        ("      - name: x\n        RUN : curl x", Err("        RUN : curl x".into())),
    ];
    for (extra, want) in cases {
        assert_eq!(runs(&job(&with(extra), "deploy")), want, "{extra:?}");
    }
    assert_eq!(runs(&job(&with("      - name: x\n        run: curl x"), "build")), runs(&job(&y, "build")), "the other job is untouched");
}

#[test]
fn two_jobs_build_runs_main_s_builder_and_deploy_runs_only_the_deploy_action() {
    let y = yml();
    assert_eq!(jobs(&y), ["build", "deploy"]);
    let build = job(&y, "build");
    let deploy = job(&y, "deploy");
    assert_eq!(runs(&build), Ok(vec![r#"scripts/pages-build.sh "$RUNNER_TEMP/site""#.to_string()]));
    assert_eq!(runs(&deploy), Ok(vec![]), "the deploy job runs no command");
    let uses = |j: &[String]| -> Vec<String> { j.iter().filter_map(|l| l.trim().trim_start_matches("- ").strip_prefix("uses: ").map(String::from)).collect() };
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    let checkout = dist["dist"]["github-action-commits"]["actions/checkout"].as_str().unwrap();
    assert_eq!(uses(&build), [format!("actions/checkout@{checkout}"), "actions/upload-pages-artifact@fc324d3547104276b827a68afc52ff2a11cc49c9".to_string()]);
    assert_eq!(uses(&deploy), ["actions/deploy-pages@368f82528645a54fb793d4d04e342629a3f51346"], "no checkout: no repository code in the deploy job");
    for u in uses(&build).iter().chain(&uses(&deploy)) {
        let pin = u.split_once('@').unwrap().1;
        assert!(pin.len() == 40 && pin.bytes().all(|b| b.is_ascii_hexdigit()), "{u} is pinned to a 40-hex commit");
    }
    for want in ["          ref: main", "          fetch-depth: 0", "          persist-credentials: false", "          path: ${{ runner.temp }}/site"] {
        assert!(build.iter().any(|l| l == want), "build lacks {want:?}");
    }
    assert!(deploy.iter().any(|l| l == "    needs: build"));
    assert!(deploy.iter().any(|l| l == "      name: github-pages"), "the environment whose rule admits only main");
    for j in ["build", "deploy"] {
        let lines = job(&y, j);
        assert!(lines.iter().any(|l| l == "    runs-on: ubuntu-24.04"), "{j}");
        assert!(lines.iter().any(|l| l.starts_with("    timeout-minutes: ")), "{j}");
        assert!(!lines.iter().any(|l| l.contains("continue-on-error")), "{j}");
    }
}

#[test]
fn no_event_value_reaches_a_step_and_no_cache_is_used() {
    let y = yml();
    let event_lines: Vec<&str> = code(&y).into_iter().filter(|l| l.contains("github.event")).collect();
    assert_eq!(event_lines, ["    if: github.event.workflow_run.head_repository.full_name == github.repository"], "the only event value is the fork filter");
    for l in code(&y).into_iter().filter(|l| l.trim_start().starts_with("- run:") || l.trim_start().starts_with("run:")) {
        assert!(!l.contains("${{"), "an expression in a run line: {l}");
        assert!(!l.trim_end().ends_with('|') && !l.trim_end().ends_with('>'), "no multi-line run block: {l}");
    }
    assert!(!y.contains("cache") && !y.contains("download-artifact") && !y.contains("workflow_run.id"), "nothing from the run that started it, and no cache");
}

#[test]
fn deploys_queue_and_are_never_cancelled() {
    assert_eq!(block(&yml(), "concurrency"), ["  group: pages", "  cancel-in-progress: false"]);
}

#[test]
fn the_builder_is_executable_and_reads_git_data_only() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/pages-build.sh");
    let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&p).unwrap().permissions());
    assert_eq!(mode & 0o111, 0o111, "the workflow step runs it directly");
    let s = read("scripts/pages-build.sh");
    assert!(s.starts_with("#!/bin/sh\n"));
    for never in ["checkout", "worktree", "cargo", ". ./", "source ", "eval", "sh -c", "tar ", "cp ", "ln "] {
        assert!(!code(&s).iter().any(|l| l.contains(never)), "the builder runs or copies branch content through {never:?}");
    }
    assert_eq!(code(&s).iter().filter(|l| l.contains("git cat-file blob")).count(), 2, "files and the note are written only as blobs");
}
