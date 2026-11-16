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

/// `Err` naming the first line where `y`'s code lines leave `want`, or a missing or extra line; the
/// exact-lines rule E-GHFLOW2.W8 made for pages.yml (its code reviews 2 and 3), shared by every
/// workflow lock. A line reader is sound only on a file held to the exact lines it understands: any
/// other line - a plain scalar's continuation, an anchor or alias, a flow mapping, a quoted or escaped
/// key, an unlisted key at any level (env, defaults, shell, a step-level if, container, services), a
/// trailing comment, a CR inside a line - is refused, never parsed. A comment line at any column is
/// dropped first, so a comment can neither hide a step from the readers nor fail the lock.
pub(crate) fn exact_lines(y: &str, want: &str) -> Result<(), String> {
    // every byte is printable ASCII or '\n' before a line is judged, so no other line break (CR, NEL,
    // LS, PS), BOM or Unicode space can make a line YAML reads differently from the one this compares;
    // then a blank line is all spaces and a comment line is spaces, then '#'. A tab line is never blank
    // or a comment here, so the lock fails it as an unlisted line
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

/// `y` with `add` put after its line `after` (which must be there once), or `after` replaced by `add`
/// when `replace`.
pub(crate) fn mutated_in(y: &str, after: &str, add: &str, replace: bool) -> String {
    assert_eq!(y.lines().filter(|l| *l == after).count(), 1, "{after:?} is one line");
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

/// The lines of job `name`'s scripts/ci-gate.sh step, from its run line to the next step.
fn script_step(y: &str, name: &str) -> Vec<String> {
    let j = job(y, name);
    let at = j
        .iter()
        .position(|l| l.trim_start().trim_start_matches("- ") == format!("run: scripts/ci-gate.sh {name}"))
        .unwrap_or_else(|| panic!("{name} runs no ci-gate.sh"));
    j[at..].iter().enumerate().take_while(|(i, l)| *i == 0 || !l.starts_with("      - ")).map(|(_, l)| l.clone()).collect()
}

/// The pinned action `a` as dist-workspace.toml pins it.
fn pin(a: &str) -> String {
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    format!("{a}@{}", dist["dist"]["github-action-commits"][a].as_str().unwrap())
}

/// The env of a ci-gate.sh step that judges a pull request: its head and base, both repositories.
const PR_ENV: &str = "        env:
          HEAD_BRANCH: ${{ github.event.pull_request.head.ref }}
          HEAD_SHA: ${{ github.event.pull_request.head.sha }}
          BASE_BRANCH: ${{ github.event.pull_request.base.ref }}
          BASE_SHA: ${{ github.event.pull_request.base.sha }}
          REPO: ${{ github.repository }}
          HEAD_REPO: ${{ github.event.pull_request.head.repo.full_name }}";

/// A judging job's head: its required name, after build, never skipped (code review round 1).
const JUDGE: &str = "    needs: build
    if: ${{ !cancelled() }}
    runs-on: ubuntu-24.04";

/// Every code line of keel-gate.yml (security review and code review round 1, E-GHFLOW2.W4).
const GATE_LOCK: &str = "name: Keel gate
on:
  pull_request_target:
    types:
      - opened
      - synchronize
      - reopened
      - edited
    branches:
      - main
      - 'epic/**'
permissions:
  contents: read
concurrency:
  group: keel-gate-${{ github.event.pull_request.number }}
  cancel-in-progress: true
jobs:
  build:
    runs-on: ubuntu-24.04
    timeout-minutes: 30
    outputs:
      keel_sha256: ${{ steps.build.outputs.keel_sha256 }}
    steps:
{checkout}
      - id: build
        run: scripts/ci-gate.sh build
{env}
          KEEL_OUT: ${{ runner.temp }}/keel-bin
      - uses: {upload}
        with:
          name: keel
          path: ${{ runner.temp }}/keel-bin/keel
          retention-days: 7
          if-no-files-found: error
  ci:
    name: keel/ci
{judge}
    timeout-minutes: 45
    steps:
{checkout}
{download}
      - run: rustup component add rustfmt clippy
      - run: scripts/ci-gate.sh ci
{env}
{keel}
  gate:
    name: keel/gate
{judge}
    timeout-minutes: 20
    steps:
{checkout}
{download}
      - run: scripts/ci-gate.sh gate
{env}
{keel}
  docs:
    name: keel/docs
{judge}
    timeout-minutes: 20
    steps:
{checkout}
{download}
      - run: scripts/ci-gate.sh docs
{env}
{keel}";

/// Every code line of keel-main.yml.
const MAIN_LOCK: &str = "name: Keel main
on:
  push:
    branches:
      - main
permissions:
  contents: read
concurrency:
  group: keel-main-${{ github.sha }}
  cancel-in-progress: false
jobs:
  ci:
    name: main/ci
    runs-on: ubuntu-24.04
    timeout-minutes: 45
    steps:
{checkout}
      - run: rustup component add rustfmt clippy
      - run: scripts/ci-gate.sh ci
        env:
          HEAD_BRANCH: ${{ github.ref_name }}
          HEAD_SHA: ${{ github.sha }}
          REPO: ${{ github.repository }}
          HEAD_REPO: ${{ github.repository }}";

/// The checkout step both locks hold: main's commit, all history, no credential kept.
fn checkout_step() -> String {
    format!(
        "      - uses: {}\n        with:\n          ref: ${{{{ github.sha }}}}\n          fetch-depth: 0\n          persist-credentials: false",
        pin("actions/checkout")
    )
}

fn gate_lock() -> String {
    let download =
        format!("      - uses: {}\n        with:\n          name: keel\n          path: ${{{{ runner.temp }}}}/keel-bin", pin("actions/download-artifact"));
    GATE_LOCK
        .replace("{checkout}", &checkout_step())
        .replace("{download}", &download)
        .replace("{upload}", &pin("actions/upload-artifact"))
        .replace("{env}", PR_ENV)
        .replace("{judge}", JUDGE)
        .replace("{keel}", &KEEL_ENV.join("\n"))
}

fn main_lock() -> String {
    MAIN_LOCK.replace("{checkout}", &checkout_step())
}

/// Code review round 1: the readers below go line by line, so both workflows are held to their exact
/// code lines; a key they never read (a step-level if, a shell, a job's defaults) cannot slip in.
#[test]
fn both_gate_workflows_are_exactly_their_locked_lines() {
    assert_eq!(exact_lines(&yml(), &gate_lock()), Ok(()));
    assert_eq!(exact_lines(&read(".github/workflows/keel-main.yml"), &main_lock()), Ok(()));
}

/// Each mutation the round-1 reviewer showed turning keel/ci, keel/gate and keel/docs green without
/// running the script, and each class W8's reviews found passing a line reader, is refused by the
/// lock; a comment at any column, or a blank line, alone, is not.
#[test]
fn the_gate_locks_refuse_every_layout_that_could_skip_or_steer_the_script() {
    let y = yml();
    let m = |after: &str, add: &str, replace: bool| mutated_in(&y, after, add, replace);
    let step = |job: &str| format!("      - run: scripts/ci-gate.sh {job}");
    let build_head = "    timeout-minutes: 30";
    let gate_head = "    name: keel/gate";
    let refused: Vec<(&str, String)> = vec![
        // the round-1 reviewer's three: the script never runs, the job is green
        ("a step-level if: false on ci's script", m(&step("ci"), "        if: false", false)),
        ("a step-level if: false on gate's script", m(&step("gate"), "        if: false", false)),
        ("a step-level if on docs' script undoing edited", m(&step("docs"), "        if: github.event.action != 'edited'", false)),
        ("a step shell that runs nothing on ci", m(&step("ci"), "        shell: 'true {0}'", false)),
        ("a step shell that runs nothing on docs", m(&step("docs"), "        shell: 'true {0}'", false)),
        ("a job-level defaults shell on gate", m(gate_head, "    defaults:\n      run:\n        shell: 'true {0}'", false)),
        ("a job-level defaults working-directory on gate", m(gate_head, "    defaults:\n      run:\n        working-directory: /tmp", false)),
        ("continue-on-error on ci's script", m(&step("ci"), "        continue-on-error: true", false)),
        ("continue-on-error on the gate job", m(gate_head, "    continue-on-error: true", false)),
        ("KEEL moved off docs' script step", m(&step("docs"), "      - run: true\n        env:\n          KEEL: x", false)),
        // W8's classes
        ("a plain scalar's continuation line under ci's run", m(&step("ci"), "        ; true", false)),
        ("an anchor on gate's script step", m(&step("gate"), &step("gate").replace("- run:", "- &g run:"), true)),
        ("an alias reusing a step in docs", m(&step("docs"), "      - *g", false)),
        ("a flow-mapping step in docs", m(&step("docs"), "      - {run: 'true'}", false)),
        ("a quoted run key in docs", m(&step("docs"), "      - name: x\n        \"run\": curl x", false)),
        ("an escaped run key in docs", m(&step("docs"), "      - name: x\n        \"\\x72un\": curl x", false)),
        ("a run after - name: in gate", m(&step("gate"), "      - name: x\n        run: curl x", false)),
        ("a step after a column-0 comment", m(&step("gate"), "# note\n      - run: curl x", false)),
        ("an unpinned action after a comment", m(&step("ci"), "  # note\n      - uses: evil/action@main", false)),
        ("a permission after a comment", m("  contents: read", "  # note\n  pull-requests: write", false)),
        ("a top-level env", m("  contents: read", "env:\n  BASH_ENV: x", false)),
        ("a top-level defaults", m("  contents: read", "defaults:\n  run:\n    shell: 'true {0}'", false)),
        ("a container on gate", m(gate_head, "    container: alpine", false)),
        ("services on gate", m(gate_head, "    services:\n      x:\n        image: y", false)),
        ("an env on gate", m(gate_head, "    env:\n      KEEL: x", false)),
        ("a trailing comment", m("    timeout-minutes: 45", "    timeout-minutes: 45 # x", true)),
        ("a CR inside a line", m("    timeout-minutes: 45", "    timeout-minutes: 45\r    if: false", true)),
        ("a fifth job", format!("{y}  extra:\n    runs-on: ubuntu-24.04\n")),
        ("a second document", format!("{y}---\nname: x\n")),
        ("a lone CR after a comment", m(&step("gate"), "      # note\r      - uses: evil/action@main", false)),
        ("a NEL after a comment", m(&step("gate"), "      # note\u{85}      - uses: evil/action@main", false)),
        ("an LS after a comment", m(&step("gate"), "      # note\u{2028}      - uses: evil/action@main", false)),
        ("a PS after a comment", m(&step("gate"), "      # note\u{2029}      - uses: evil/action@main", false)),
        ("an NBSP-then-# line under ci's run", m(&step("ci"), "          \u{a0}#|| true", false)),
        ("a U+3000 'blank' line", m(build_head, "        \u{3000}", false)),
        ("a tab-indented comment, kept as code", m("    timeout-minutes: 45", "\t# note", false)),
        ("a tab-only line, kept as code", m("    timeout-minutes: 45", "\t", false)),
        ("a BOM", format!("\u{feff}{y}")),
        ("CRLF line ends", y.replace('\n', "\r\n")),
    ];
    for (what, bad) in &refused {
        assert!(exact_lines(bad, &gate_lock()).is_err(), "{what} passed the lock:\n{bad}");
    }
    for (what, ok) in [
        ("a column-0 comment in a job", m(&step("gate"), "# note", false)),
        ("an indented comment", m("    timeout-minutes: 45", "      # note", false)),
        ("a blank line", m("    timeout-minutes: 45", "", false)),
    ] {
        assert_eq!(exact_lines(&ok, &gate_lock()), Ok(()), "{what}");
    }
    let main = read(".github/workflows/keel-main.yml");
    let mm = |after: &str, add: &str| mutated_in(&main, after, add, false);
    for (what, bad) in [
        ("a step-level if: false", mm("      - run: scripts/ci-gate.sh ci", "        if: false")),
        ("a step shell", mm("      - run: scripts/ci-gate.sh ci", "        shell: 'true {0}'")),
        ("a job-level defaults", mm("    name: main/ci", "    defaults:\n      run:\n        shell: 'true {0}'")),
        ("a required name", mutated_in(&main, "    name: main/ci", "    name: keel/ci", true)),
        ("a trigger on a work branch", mm("      - main", "      - 'wu/**'")),
    ] {
        assert!(exact_lines(&bad, &main_lock()).is_err(), "keel-main.yml: {what} passed the lock");
    }
}

#[test]
fn block_and_job_end_at_the_next_key() {
    let y = "on:\n  push:\n\n    branches: [a]\njobs:\n  a:\n    x: 1\n  b:\n    y: 2\n";
    assert_eq!(block(y, "on"), ["  push:", "    branches: [a]"]);
    assert_eq!(job(y, "a"), ["    x: 1"]);
    assert_eq!(jobs(y), ["a", "b"]);
}

/// E-GHFLOW2.W4: GitHub takes a pull_request_target workflow from main for every base, so no pull
/// request changes what judges it; a work branch's push runs nothing here (its pull request does).
/// `edited` re-judges a retargeted pull request (security review: a WU retargeted to main).
#[test]
fn it_runs_from_main_on_pull_requests_into_main_or_an_epic_only_and_again_when_one_is_edited() {
    let y = yml();
    assert_eq!(
        block(&y, "on"),
        [
            "  pull_request_target:",
            "    types:",
            "      - opened",
            "      - synchronize",
            "      - reopened",
            "      - edited",
            "    branches:",
            "      - main",
            "      - 'epic/**'"
        ]
    );
    for never in [
        "wu/**",
        "tags",
        "push",
        "  pull_request:",
        "workflow_run",
        "workflow_dispatch",
        "release:",
        "issue_comment",
        "schedule",
        "merge_group",
        "github.ref_name",
        "github.repository ||",
        "|| github",
    ] {
        assert!(!code(&y).join("\n").contains(never), "keel-gate.yml mentions {never}");
    }
}

/// A workflow's lines that are not comments, so a word in a comment proves nothing; shared with
/// pages_workflow.rs.
pub(crate) fn code(y: &str) -> Vec<&str> {
    y.lines().filter(|l| !l.trim_start().starts_with('#')).collect()
}

/// A push of main runs keel-main.yml: its one job is main/ci, never a required name, so a back-merge
/// pull request at main's commit never carries a skipped or main-defined keel/ check.
#[test]
fn a_push_of_main_runs_main_ci_under_a_name_no_ruleset_requires() {
    let y = read(".github/workflows/keel-main.yml");
    assert_eq!(block(&y, "on"), ["  push:", "    branches:", "      - main"]);
    assert_eq!(block(&y, "permissions"), ["  contents: read"]);
    assert_eq!(block(&y, "concurrency"), ["  group: keel-main-${{ github.sha }}", "  cancel-in-progress: false"]);
    assert_eq!(jobs(&y), ["ci"]);
    let j = job(&y, "ci");
    let checkout = pin("actions/checkout");
    for l in [
        "    name: main/ci",
        "    runs-on: ubuntu-24.04",
        &format!("      - uses: {checkout}"),
        "          ref: ${{ github.sha }}",
        "          persist-credentials: false",
        "      - run: rustup component add rustfmt clippy",
        "      - run: scripts/ci-gate.sh ci",
        "          HEAD_BRANCH: ${{ github.ref_name }}",
        "          HEAD_SHA: ${{ github.sha }}",
        "          REPO: ${{ github.repository }}",
        "          HEAD_REPO: ${{ github.repository }}",
    ] {
        assert!(j.iter().any(|x| x == l), "keel-main.yml lacks {l}");
    }
    for never in ["secrets.", "github.token", "GITHUB_TOKEN", "if:", "keel/"] {
        assert!(!code(&y).join("\n").contains(never), "keel-main.yml has {never}");
    }
    for l in y.lines().filter(|l| l.contains("run:")) {
        assert!(!l.contains("${{"), "{l}");
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
fn a_superseded_pull_request_run_is_cancelled() {
    assert_eq!(block(&yml(), "concurrency"), ["  group: keel-gate-${{ github.event.pull_request.number }}", "  cancel-in-progress: true"]);
}

/// Every checkout is main's commit (the workflow's own), never the head's; the head is git data.
#[test]
fn every_action_is_pinned_to_the_dist_commit_and_every_checkout_is_mains_commit_keeping_no_credential() {
    let y = yml();
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
    // bound to the job's scripts/ci-gate.sh step, not just somewhere in the job (code review round 1)
    let has = |name: &str, env: &str| assert!(script_step(&y, name).iter().any(|l| l == env), "{name}'s ci-gate.sh step lacks {env}");
    for name in ["build", "ci", "gate", "docs"] {
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
    // each job's own if: line, so one moved from a judge to build is seen (code review round 1)
    let ifs: Vec<Vec<String>> =
        ["build", "ci", "gate", "docs"].iter().map(|n| job(&y, n).into_iter().filter(|l| l.trim_start().starts_with("if:")).collect()).collect();
    assert_eq!(
        ifs,
        [
            vec![],
            vec!["    if: ${{ !cancelled() }}".to_string()],
            vec!["    if: ${{ !cancelled() }}".to_string()],
            vec!["    if: ${{ !cancelled() }}".to_string()]
        ],
        "no job skips a pull request (a skipped job counts as passed): a failed build or a fork is red in each"
    );
    let b = job(&y, "build");
    for l in [
        "    outputs:",
        "      keel_sha256: ${{ steps.build.outputs.keel_sha256 }}",
        "      - id: build",
        "          retention-days: 7",
        "          if-no-files-found: error",
    ] {
        assert!(b.iter().any(|x| x == l), "build lacks {l}");
    }
    assert!(!y.contains("name: ci\n") && !y.contains("name: gate\n") && !y.contains("name: docs\n"));
}

/// The files under .github/workflows, exactly (code review round 2): a new workflow fails here until
/// this list, and so the decoded checks below, take it in.
const WORKFLOWS: [&str; 6] = ["keel-gate.yml", "keel-main.yml", "keel-sign-off-request.yml", "keel-sign-off.yml", "pages.yml", "release.yml"];

/// The names a ruleset requires, read from scripts/gh-settings-check.sh's `def checks` (code review
/// round 3), so the two lists cannot drift: a check or status of one of these from GitHub Actions
/// passes the ruleset.
fn reserved() -> Vec<String> {
    let script = read("scripts/gh-settings-check.sh");
    let line = script.lines().map(str::trim).find(|l| l.starts_with("def checks:")).expect("gh-settings-check.sh defines checks");
    let list = &line[line.find('[').unwrap()..=line.rfind(']').unwrap()];
    let names: Vec<String> = serde_json::from_str(list).unwrap_or_else(|e| panic!("def checks is not a JSON list of names: {line}: {e}"));
    assert!(!names.is_empty() && names.iter().all(|n| n.starts_with("keel/")), "{names:?}");
    names
}

/// The only actions a workflow may use, each at its pin, and the files it may appear in (code review
/// round 3): what the six workflows use today. A deny-list of cache inputs is never complete (a
/// Cache: key, setup-go and newer setup-node caching by default, build-push-action's gha cache,
/// setup-ruby's bundler-cache); an allow-list closes the class at its root. Owner and repository
/// match case-insensitively, as GitHub resolves them; the ref matches exactly.
fn allowed_actions() -> Vec<(String, Vec<&'static str>)> {
    let all = WORKFLOWS.to_vec();
    vec![
        (pin("actions/checkout"), all.clone()),
        (pin("actions/upload-artifact"), vec!["keel-gate.yml", "release.yml"]),
        (pin("actions/download-artifact"), vec!["keel-gate.yml", "release.yml"]),
        // dist's npm publish job only: no checkout there, so no lockfile for it to cache
        (pin("actions/setup-node"), vec!["release.yml"]),
        (crate::pages_workflow::UPLOAD.to_string(), vec!["pages.yml"]),
        (crate::pages_workflow::DEPLOY.to_string(), vec!["pages.yml"]),
    ]
}

/// Whether `uses` in `file` is an allowed action at its pin.
fn action_allowed(file: &str, uses: &str) -> bool {
    let Some((repo, at)) = uses.split_once('@') else { return false };
    allowed_actions().iter().any(|(a, files)| {
        let (arepo, aat) = a.split_once('@').unwrap();
        arepo.eq_ignore_ascii_case(repo) && aat == at && files.contains(&file)
    })
}

/// The events each workflow may start on: pull_request_target is keel-gate.yml's alone, and
/// release.yml runs on no pull request (its token can write; DEC-38 amended).
fn events(file: &str) -> Vec<&'static str> {
    match file {
        "keel-gate.yml" => vec!["pull_request_target"],
        "keel-main.yml" | "release.yml" => vec!["push"],
        "keel-sign-off-request.yml" => vec!["pull_request_review"],
        "keel-sign-off.yml" | "pages.yml" => vec!["workflow_run"],
        _ => vec![],
    }
}

/// Workflow files, as (file name, text).
type Files = Vec<(String, String)>;

/// The workflow files on disk, by name.
fn workflow_files() -> Files {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect();
    files.sort();
    files
}

/// A decoded key or scalar as text; anything else as its debug form, which never equals a name.
fn text_of(y: &yaml_rust2::Yaml) -> String {
    y.as_str().map(String::from).unwrap_or_else(|| format!("{y:?}"))
}

/// What the workflows break, decoded by a YAML loader rather than read by line (code review round 2:
/// a quoted or double-spaced name, a flow-mapping cache input, a `pull_request: {}` slip past a line
/// reader): the file list is WORKFLOWS; each file is one document whose `on` keys are its events;
/// no job id or name is a reserved name but keel-gate.yml's ci, gate and docs, and a name built by an
/// expression starts with text no reserved name starts with; no job calls a reusable workflow; every
/// step's action is on the allow-list, at its pin, in a file it may appear in (round 3); and, as a
/// second line, no step passes a cache input under any case of its key.
fn decoded_problems(files: &[(String, String)]) -> Vec<String> {
    use yaml_rust2::{Yaml, YamlLoader};
    let reserved = reserved();
    let mut out = Vec::new();
    let mut names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    let mut want = WORKFLOWS.to_vec();
    want.sort();
    if names != want {
        out.push(format!("the workflow files are {names:?}, not {want:?}"));
    }
    for (file, src) in files {
        let docs = match YamlLoader::load_from_str(src) {
            Ok(d) => d,
            Err(e) => {
                out.push(format!("{file}: not YAML: {e}"));
                continue;
            }
        };
        if docs.len() != 1 {
            out.push(format!("{file}: {} documents", docs.len()));
            continue;
        }
        let w = &docs[0];
        let mut on: Vec<String> = match &w["on"] {
            Yaml::String(s) => vec![s.clone()],
            Yaml::Array(a) => a.iter().map(text_of).collect(),
            Yaml::Hash(h) => h.keys().map(text_of).collect(),
            other => vec![format!("{other:?}")],
        };
        on.sort();
        let mut want = events(file);
        want.sort();
        if on != want {
            out.push(format!("{file}: starts on {on:?}, not {want:?}"));
        }
        let Some(jobs) = w["jobs"].as_hash() else {
            out.push(format!("{file}: no jobs"));
            continue;
        };
        for (id, job) in jobs {
            let id = text_of(id);
            let name = match &job["name"] {
                Yaml::BadValue => None,
                n => Some(text_of(n)),
            };
            let own = file == "keel-gate.yml"
                && matches!((id.as_str(), name.as_deref()), ("ci", Some("keel/ci")) | ("gate", Some("keel/gate")) | ("docs", Some("keel/docs")));
            for shown in [Some(id.clone()), name.clone()].into_iter().flatten() {
                let literal = shown.split("${{").next().unwrap_or_default().to_string();
                let imitates = if shown.contains("${{") { reserved.iter().any(|r| r.starts_with(literal.trim())) } else { reserved.contains(&shown) };
                if imitates && !own {
                    out.push(format!("{file}: job {id} reports {shown:?}, a required name"));
                }
            }
            if !matches!(job["uses"], Yaml::BadValue) {
                out.push(format!("{file}: job {id} calls a reusable workflow"));
            }
            for step in job["steps"].as_vec().map(Vec::as_slice).unwrap_or_default() {
                // the allow-list first: any other action, or one outside its files, is red
                match &step["uses"] {
                    Yaml::BadValue => {}
                    u => {
                        let uses = text_of(u);
                        if !action_allowed(file, &uses) {
                            out.push(format!("{file}: job {id} uses {uses}, not an allowed action at its pin"));
                        }
                    }
                }
                // a second line: no cache input, whatever its case (GitHub upper-cases an input's key)
                if let Some(with) = step["with"].as_hash() {
                    for k in with.keys().map(text_of) {
                        if ["cache", "cache-dependency-path", "save-always", "package-manager-cache", "bundler-cache", "cache-from", "cache-to"]
                            .contains(&k.to_ascii_lowercase().as_str())
                        {
                            out.push(format!("{file}: job {id} passes {k}"));
                        }
                    }
                }
            }
        }
    }
    out
}

/// No workflow reads or writes a cache (a pull_request_target run's cache is the default branch's,
/// and head code in keel/ci could write one); only keel-gate.yml runs on pull_request_target and
/// only it reports keel/ci, keel/gate and keel/docs; release.yml runs on no pull request. Decoded,
/// over an exact file list (code review round 2), beside the exact-lines locks.
#[test]
fn the_decoded_workflows_keep_the_required_names_events_and_caches_to_themselves() {
    let files = workflow_files();
    assert_eq!(decoded_problems(&files), Vec::<String>::new());
    // non-vacuous: the decoder sees keel-gate.yml's three judges under their names
    let gate = yaml_rust2::YamlLoader::load_from_str(&files.iter().find(|(n, _)| n == "keel-gate.yml").unwrap().1).unwrap();
    let named: Vec<String> = gate[0]["jobs"].as_hash().unwrap().values().filter_map(|j| j["name"].as_str().map(String::from)).collect();
    assert_eq!(named, ["keel/ci", "keel/gate", "keel/docs"]);
    // the allow-list is what the workflows use today: every entry is used in one of its files
    for (action, may) in allowed_actions() {
        assert!(files.iter().any(|(n, t)| may.contains(&n.as_str()) && t.contains(&format!("uses: {action}"))), "{action} is allowed but used nowhere");
    }
    assert_eq!(reserved(), ["keel/ci", "keel/gate", "keel/docs", "keel/sign-off"], "read from gh-settings-check.sh's def checks");
    let dist: toml::Table = read("dist-workspace.toml").parse().unwrap();
    assert_eq!(dist["dist"]["pr-run-mode"].as_str(), Some("skip"), "dist generates release.yml without its pull-request run");
}

/// `t` with one more job, `imitate`, whose lines after its runs-on are `body`.
fn add_job(t: &str, body: &str) -> String {
    format!("{t}  imitate:\n    runs-on: ubuntu-24.04\n{body}")
}

/// The round-2 reviewer's mutations and their neighbours, each red: a new file, and in a file the
/// list allows, a quoted, single-quoted, double-spaced, anchored or expression-built required name, a
/// cache input in a flow mapping, a cache action, a pull-request trigger in any form, a reusable
/// workflow call, a second document.
#[test]
fn the_decoder_refuses_every_imitated_name_cache_and_trigger() {
    let files = workflow_files();
    let node = pin("actions/setup-node");
    let with = |file: &str, f: &dyn Fn(&str) -> String| -> Vec<(String, String)> {
        files.iter().map(|(n, t)| (n.clone(), if n == file { f(t) } else { t.clone() })).collect()
    };
    let mut new_file = files.clone();
    new_file.push((
        "imitate.yml".into(),
        "name: x\non: push\njobs:\n  ci:\n    name: keel/ci\n    runs-on: ubuntu-24.04\n    steps:\n      - run: 'true'\n".into(),
    ));
    let cases: Vec<(&str, Files, &str)> = vec![
        ("a new workflow file", new_file, "the workflow files are"),
        ("a double-quoted name", with("release.yml", &|t: &str| add_job(t, "    name: \"keel/ci\"\n")), "reports \"keel/ci\""),
        ("a single-quoted name", with("release.yml", &|t: &str| add_job(t, "    name: 'keel/gate'\n")), "reports \"keel/gate\""),
        ("a double-spaced name", with("release.yml", &|t: &str| add_job(t, "    name:  keel/ci\n")), "reports \"keel/ci\""),
        ("the sign-off status as a job name", with("keel-sign-off.yml", &|t: &str| add_job(t, "    name: keel/sign-off\n")), "reports \"keel/sign-off\""),
        ("an anchored name", with("pages.yml", &|t: &str| add_job(t, "    name: &n keel/docs\n")), "reports \"keel/docs\""),
        ("an expression-built name", with("release.yml", &|t: &str| add_job(t, "    name: ${{ 'keel/ci' }}\n")), "a required name"),
        ("a keel/ prefix and an expression", with("release.yml", &|t: &str| add_job(t, "    name: keel/${{ 'ci' }}\n")), "a required name"),
        (
            "a required name on another keel-gate.yml job",
            with("keel-gate.yml", &|t: &str| mutated_in(t, "    name: keel/ci", "    name: keel/gate", true)),
            "job ci reports \"keel/gate\"",
        ),
        (
            "a cache input in a flow mapping",
            with("release.yml", &|t: &str| add_job(t, "    steps:\n      - uses: actions/setup-node@v4\n        with: { cache: npm }\n")),
            "passes cache",
        ),
        (
            "a cache action",
            with("keel-main.yml", &|t: &str| {
                add_job(t, "    steps:\n      - uses: actions/cache/restore@v4\n        with:\n          path: x\n          key: y\n")
            }),
            "uses actions/cache/restore",
        ),
        ("a rust cache", with("keel-main.yml", &|t: &str| add_job(t, "    steps:\n      - uses: Swatinem/rust-cache@v2\n")), "rust-cache"),
        ("pull_request: {} on release.yml", with("release.yml", &|t: &str| mutated_in(t, "on:", "  pull_request: {}", false)), "release.yml: starts on"),
        (
            "a quoted pull_request_target key",
            with("keel-sign-off.yml", &|t: &str| mutated_in(t, "on:", "  \"pull_request_target\":", false)),
            "keel-sign-off.yml: starts on",
        ),
        (
            "a flow-style trigger list",
            with("keel-main.yml", &|t: &str| t.replace("on:\n  push:\n    branches:\n      - main\n", "on: [push, pull_request]\n")),
            "keel-main.yml: starts on",
        ),
        ("a reusable workflow call", with("release.yml", &|t: &str| add_job(t, "    uses: ./.github/workflows/x.yml\n")), "calls a reusable workflow"),
        ("a second document", with("pages.yml", &|t: &str| format!("{t}---\nname: x\n")), "2 documents"),
        // code review round 3: caching no deny-list names, closed by the action allow-list
        (
            "a Cache: key on the allowed setup-node",
            with("release.yml", &|t: &str| add_job(t, &format!("    steps:\n      - uses: {node}\n        with:\n          Cache: npm\n"))),
            "passes Cache",
        ),
        (
            "a CACHE: key on setup-python",
            with("release.yml", &|t: &str| add_job(t, "    steps:\n      - uses: actions/setup-python@v5\n        with:\n          CACHE: pip\n")),
            "passes CACHE",
        ),
        (
            "setup-go, which caches by default",
            with("keel-main.yml", &|t: &str| add_job(t, "    steps:\n      - uses: actions/setup-go@v5\n")),
            "actions/setup-go@v5, not an allowed action",
        ),
        (
            "setup-node@v5 with no with",
            with("release.yml", &|t: &str| add_job(t, "    steps:\n      - uses: actions/setup-node@v5\n")),
            "actions/setup-node@v5, not an allowed action",
        ),
        (
            "build-push-action's gha cache",
            with("release.yml", &|t: &str| {
                add_job(t, "    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          cache-from: type=gha\n          cache-to: type=gha,mode=max\n")
            }),
            "docker/build-push-action@v6, not an allowed action",
        ),
        (
            "setup-ruby's bundler cache",
            with("release.yml", &|t: &str| add_job(t, "    steps:\n      - uses: ruby/setup-ruby@v1\n        with:\n          bundler-cache: true\n")),
            "ruby/setup-ruby@v1, not an allowed action",
        ),
        (
            "an unknown action",
            with("pages.yml", &|t: &str| add_job(t, "    steps:\n      - uses: someone/anything@0123456789012345678901234567890123456789\n")),
            "not an allowed action",
        ),
        (
            "an allowed action at another ref",
            with("keel-main.yml", &|t: &str| add_job(t, "    steps:\n      - uses: actions/checkout@v4\n")),
            "actions/checkout@v4, not an allowed action",
        ),
        (
            "an allowed action outside its files",
            with("keel-main.yml", &|t: &str| add_job(t, &format!("    steps:\n      - uses: {node}\n"))),
            "not an allowed action",
        ),
        (
            "a local action",
            with("keel-main.yml", &|t: &str| add_job(t, "    steps:\n      - uses: ./.github/actions/x\n")),
            "./.github/actions/x, not an allowed action",
        ),
        (
            "a docker action",
            with("keel-main.yml", &|t: &str| add_job(t, "    steps:\n      - uses: docker://alpine:3\n")),
            "docker://alpine:3, not an allowed action",
        ),
    ];
    // GitHub resolves owner and repository in any case; the ref is exact
    let upper = with("keel-main.yml", &|t: &str| t.replace("uses: actions/checkout@", "uses: Actions/Checkout@"));
    assert_eq!(decoded_problems(&upper), Vec::<String>::new());
    for (what, set, why) in &cases {
        let got = decoded_problems(set);
        assert!(got.iter().any(|p| p.contains(why)), "{what}: want {why:?} in {got:?}");
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
