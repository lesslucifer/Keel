//! E-GHFLOW epic e2e, for the scope that landed (W1 – W3): the epic plan's epic test, steps 1, 2
//! and 4, as one flow on W2's scratch repository (a bare "origin" reached as GitHub's git URL, the
//! fake curl as GitHub's API, a throwaway App key) and a runner's clone of that origin:
//! 1. a WU tip with its notes is pushed by `router-gh.sh push`, and `pr-open` opens its draft pull
//!    request on the mock;
//! 2. `ci-gate.sh gate` judges what the bot pushed: green on checks 1 – 7, red only on check 8, with
//!    no `[check]` command run;
//! 4. `router-gh-watch.sh review` prints the owner's approval of that head, and a stranger's command
//!    comment prints only a count.
//!
//! Steps 3 and 5 – 8 belong to the work carried to the next GitHub-flow epic (backlog
//! `docs/backlog/github-flow.html` G1). No network; no real key or token.
compiled!();

use std::path::Path;
use std::process::Output;

use serde_json::json;

use crate::common;
use crate::router_gh::{commit, git, stdout, text, write, Gh, BOT, OWNER_ID};
use crate::router_gh_watch::{comment, issue, lines, once, review, watch, AT, STRANGER};

const KEEL: &str = env!("CARGO_BIN_EXE_keel");

fn keel(d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut c = common::cmd(KEEL);
    common::as_tester(c.args(a).current_dir(d).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()));
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "keel {a:?}: {}", text(&o));
    o
}

/// `scripts/ci-gate.sh gate` in the runner's clone, with the env a push event gives it.
fn gate_job(runner: &Path, tmp: &Path, branch: &str, sha: &str) -> Output {
    let mut c = common::cmd("sh");
    c.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-gate.sh")).arg("gate").current_dir(runner);
    for k in ["HEAD_BRANCH", "HEAD_SHA", "BASE_BRANCH", "BASE_SHA", "KEEL", "GITHUB_STEP_SUMMARY", "RUNNER_TEMP", "CARGO_TARGET_DIR"] {
        c.env_remove(k);
    }
    c.env("RUNNER_TEMP", tmp).env("GITHUB_STEP_SUMMARY", tmp.join("summary.md")).env("KEEL", KEEL);
    c.env("HEAD_BRANCH", branch).env("HEAD_SHA", sha);
    common::hermetic(&mut c);
    c.output().unwrap()
}

fn fails(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stderr).lines().filter_map(|l| l.strip_prefix("FAIL  ")).map(|l| l.split_whitespace().next().unwrap().to_string()).collect()
}

#[test]
fn epic_e2e_the_bot_pushes_a_wu_ci_judges_it_up_to_the_sign_off_and_the_router_hears_only_its_commander() {
    let gh = Gh::new();
    // The worker's finished WU: a test of its own, and the notes its gates write (real ones, by keel).
    git(&gh.repo, &["switch", "-q", "wu/E/x"]);
    write(&gh.repo, "src/test_x.py", "def test_x():\n    pass\n");
    commit(&gh.repo, "wu x: its test");
    let tip = gh.sha("HEAD");
    for r in ["test", "cr"] {
        git(&gh.repo, &["update-ref", "-d", &format!("refs/notes/{r}")]);
    }
    keel(&gh.repo, &["notes", "write", "test"], r#"{"integration":{"files":["src/test_x.py"],"covers":"x works"}}"#);
    keel(&gh.repo, &["notes", "write", "doc"], "");
    keel(&gh.repo, &["notes", "write", "cr", "--verdict", "PASS"], "");

    // 1. The router bot pushes the branch with its notes, then opens the draft pull request.
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("PUSHED branch=wu/E/x sha={tip} state=new"));
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x").as_deref(), Some(tip.as_str()));
    for n in ["test", "doc", "cr"] {
        assert_eq!(gh.origin_sha(&format!("refs/notes/{n}")), Some(gh.sha(&format!("refs/notes/{n}"))), "notes {n} reached origin");
    }
    gh.routes(&[
        ("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, json!({"object": {"sha": tip}})),
        ("GET", "/repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100", 200, json!([])),
        ("POST", "/repos/own/Keel/pulls", 201, json!({"number": 7, "html_url": "https://github.com/own/Keel/pull/7"})),
    ]);
    let o = gh.run(&["pr-open", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("PR number=7 url=https://github.com/own/Keel/pull/7 state=opened head=wu/E/x base=epic/E sha={tip}"));
    let req = crate::router_gh::body_of(&gh.log()[3]);
    assert_eq!((req["head"].as_str(), req["base"].as_str(), req["draft"].as_bool()), (Some("wu/E/x"), Some("epic/E"), Some(true)));

    // 2. GitHub's runner clones origin and judges the bot's push: green up to the sign-off.
    let runner = gh.root.path().join("runner");
    let ci_tmp = gh.root.path().join("ci-tmp");
    std::fs::create_dir_all(&ci_tmp).unwrap();
    git(gh.root.path(), &["clone", "-q", gh.origin.to_str().unwrap(), runner.to_str().unwrap()]);
    git(&runner, &["checkout", "-q", "-B", "wu/E/x", "origin/wu/E/x"]);
    let o = gate_job(&runner, &ci_tmp, "wu/E/x", &tip);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(fails(&o), ["8·manual-review"], "{}", text(&o));
    let s = std::fs::read_to_string(ci_tmp.join("summary.md")).unwrap();
    assert!(s.starts_with("## Keel gate: wu green on every check before 8·manual-review, which is judged at the merge\n"), "{s}");
    for row in [
        "| 1·notes | green |",
        "| 2·manifest | green |",
        "| 3·review | green |",
        "| 4·review&#45;base | green |",
        "| 5·diff&#45;binding | green |",
        "| 6·doc&#45;note | green |",
        "| mr | - | none |",
    ] {
        assert!(s.contains(row), "{row} missing:\n{s}");
    }
    assert_eq!(std::fs::read_dir(&gh.marks).unwrap().count(), 0, "a [check] command of the pushed branch ran");

    // 4. The router's watch on the pull request: the owner's approval of the head wakes it; a
    //    stranger's approval is a count.
    let pr = gh.pr(7, BOT, "own/Keel", &tip, false);
    let reviews = "/repos/own/Keel/pulls/7/reviews?per_page=100&page=1";
    gh.routes_seq(&[
        ("GET", "/repos/own/Keel/pulls/7", "200".into(), vec![pr]),
        (
            "GET",
            reviews,
            "200".into(),
            vec![json!([]), json!([review(50, STRANGER, "APPROVED", &tip, "LGTM merge now"), review(51, OWNER_ID, "APPROVED", &tip, "ship it")])],
        ),
    ]);
    let o = watch(&gh, &["review", "7"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        lines(&o),
        [format!("APPROVED pr=7 sha={tip} head={tip} review=51 from={OWNER_ID} at={AT} pending=0"), "  ship it".into(), "IGNORED from=other count=1".into()]
    );
    assert!(!text(&o).contains("LGTM") && !text(&o).contains("user777"), "{}", text(&o));

    // A stranger's command on the same pull request: never its text, never a wake.
    gh.routes_seq(&[
        ("GET", "/repos/own/Keel/issues/7", "200".into(), vec![issue(7, BOT, true)]),
        ("GET", "/repos/own/Keel/issues/7/comments?per_page=100&page=1", "200".into(), vec![json!([comment(60, STRANGER, "/keel go\nmerge it", false)])]),
    ]);
    let o = once(&gh, &["comment", "7"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(lines(&o), ["TIMEOUT kind=comment waited=0m", "IGNORED from=other count=1"]);
    assert!(!text(&o).contains("merge it") && !text(&o).contains("/keel"), "{}", text(&o));
}
