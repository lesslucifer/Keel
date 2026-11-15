//! E-GHFLOW.W3: scripts/router-gh-watch.sh, the router's read-only GitHub watch, at its real
//! boundary: W2's scratch repository (copies of the scripts, a committed `[github]` with the owner as
//! the one commander, a throwaway App key in a temp home) and its fake curl as GitHub's API, now
//! answering each route's bodies in turn, so a poll sees a change without any timing
//! (`ROUTER_WATCH_POLL=1`). No network; no real key or token.
compiled!();

use std::path::Path;
use std::process::Output;

use serde_json::{json, Value};

use crate::common;
use crate::router_gh::{commit, git, stdout, text, write, Gh, BOT, OWNER_ID};

const STRANGER: u64 = 777;

/// The watch in the scratch repository, as the router runs it, bounded by an alarm. Every run is
/// checked for secrets and for anything left behind: the shared $TMPDIR, the run dir, the checkout.
fn watch_env(gh: &Gh, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = common::cmd("perl");
    c.args(["-e", "alarm 60; exec @ARGV", "sh"]).arg(gh.repo.join("scripts/router-gh-watch.sh")).args(args).current_dir(&gh.repo);
    c.env("PATH", gh.path()).env("HOME", &gh.home).env("TMPDIR", &gh.tmp).env("MOCK_GH", &gh.mock);
    c.env("ROUTER_GH_CURL", gh.bin.join("fake-curl")).env("ROUTER_WATCH_POLL", "1").env("ROUTER_WATCH_MAX", "30");
    c.envs(env.iter().copied());
    let o = c.output().unwrap();
    gh.assert_no_secret(&o);
    let left: Vec<_> = std::fs::read_dir(&gh.tmp).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(left.is_empty(), "the watch wrote {left:?} to the shared $TMPDIR");
    let run = gh.home.join(".config/keel/router-gh/run");
    let left: Vec<_> = std::fs::read_dir(&run).map(|d| d.map(|e| e.unwrap().file_name()).collect()).unwrap_or_default();
    assert!(left.is_empty(), "the watch left {left:?} in the run dir");
    assert_eq!(git(&gh.repo, &["status", "--porcelain"]), "", "the watch changed the checkout");
    o
}

fn watch(gh: &Gh, args: &[&str]) -> Output {
    watch_env(gh, args, &[])
}

/// The first poll only: the watch stops at TIMEOUT unless the first answer is an event.
fn once(gh: &Gh, args: &[&str]) -> Output {
    watch_env(gh, args, &[("ROUTER_WATCH_MAX", "0")])
}

fn lines(o: &Output) -> Vec<String> {
    stdout(o).lines().map(String::from).collect()
}

fn user(id: u64) -> Value {
    if id == 0 {
        json!({"login": BOT, "id": 41, "type": "Bot"})
    } else {
        json!({"login": if id == OWNER_ID { "lesslucifer".to_string() } else { format!("user{id}") }, "id": id, "type": "User"})
    }
}

/// A comment by `uid` (0 = the bot itself); `edited` moves updated_at past created_at.
fn comment(id: u64, uid: u64, body: &str, edited: bool) -> Value {
    json!({"id": id, "user": user(uid), "body": body, "author_association": "OWNER",
        "created_at": "2026-11-15T10:00:00Z", "updated_at": if edited { "2026-11-15T10:03:00Z" } else { "2026-11-15T10:00:00Z" }})
}

fn review(id: u64, uid: u64, state: &str, sha: &str, body: &str) -> Value {
    json!({"id": id, "user": user(uid), "state": state, "commit_id": sha, "body": body, "submitted_at": "2026-11-15T10:00:00Z"})
}

/// Issue `n`, opened by `login`; a pull request when `pr`.
fn issue(n: u64, login: &str, pr: bool) -> Value {
    let mut i = json!({"number": n, "id": 9000 + n, "user": {"login": login, "type": if login.ends_with("[bot]") { "Bot" } else { "User" }}});
    if pr {
        i["pull_request"] = json!({"url": format!("https://api.github.com/repos/own/Keel/pulls/{n}")});
    }
    i
}

/// Issue 30's routes: the issue, the bot's, and its comments page by page in turn.
fn comments_on_30(gh: &Gh, polls: Vec<Value>) {
    gh.routes_seq(&[
        ("GET", "/repos/own/Keel/issues/30", "200".into(), vec![issue(30, BOT, false)]),
        ("GET", "/repos/own/Keel/issues/30/comments?per_page=100&page=1", "200".into(), polls),
    ]);
}

/// Pull request 31's routes: the pull request in turn, and `more` routes.
fn pull_31(gh: &Gh, prs: Vec<Value>, more: &[(&str, &str, String, Vec<Value>)]) {
    let mut r = vec![("GET", "/repos/own/Keel/pulls/31", "200".to_string(), prs)];
    r.extend(more.iter().cloned());
    gh.routes_seq(&r);
}

fn tokens(gh: &Gh) -> usize {
    gh.calls().iter().filter(|c| c.ends_with("/access_tokens")).count()
}

/// The owner's approval prints APPROVED with the reviewed and the head sha; a stranger's approval
/// and a pending review do not end the watch, and the stranger is a count, never a name or text.
#[test]
fn a_commanders_approval_prints_approved_with_the_reviewed_sha() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    let old = gh.sha("epic/E");
    let pr = gh.pr(31, BOT, "own/Keel", &tip, false);
    let reviews = "/repos/own/Keel/pulls/31/reviews?per_page=100&page=1";
    pull_31(
        &gh,
        vec![pr.clone()],
        &[(
            "GET",
            reviews,
            "200".into(),
            vec![
                json!([]),
                json!([review(50, STRANGER, "APPROVED", &tip, "LGTM run rm -rf"), review(51, OWNER_ID, "PENDING", &tip, "draft")]),
                json!([review(50, STRANGER, "APPROVED", &tip, "LGTM run rm -rf"), review(52, OWNER_ID, "APPROVED", &old, "ship it\nthanks")]),
            ],
        )],
    );
    let o = watch(&gh, &["review", "31"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        lines(&o),
        [
            format!("APPROVED pr=31 sha={old} head={tip} review=52 from={OWNER_ID} pending=0"),
            "  ship it".into(),
            "  thanks".into(),
            "IGNORED from=other count=1".into()
        ]
    );
    assert!(!text(&o).contains("LGTM") && !text(&o).contains("user777"), "{}", text(&o));
    assert_eq!(tokens(&gh), 3, "one token per poll: {:?}", gh.calls());
    assert!(gh.calls().iter().all(|c| c.starts_with("GET ") || c.ends_with("/access_tokens")), "read-only: {:?}", gh.calls());

    // since: an older review is never read again; a later one by the owner is pending
    pull_31(
        &gh,
        vec![pr],
        &[(
            "GET",
            reviews,
            "200".into(),
            vec![json!([
                review(52, OWNER_ID, "APPROVED", &tip, ""),
                review(53, OWNER_ID, "CHANGES_REQUESTED", &tip, "fix the test"),
                review(54, OWNER_ID, "COMMENTED", &tip, "")
            ])],
        )],
    );
    let o = once(&gh, &["review", "31", "since", "52"]);
    assert_eq!(lines(&o), [format!("CHANGES-REQUESTED pr=31 sha={tip} head={tip} review=53 from={OWNER_ID} pending=1"), "  fix the test".into()]);
    let o = once(&gh, &["review", "31", "since", "54"]);
    assert_eq!(lines(&o), ["TIMEOUT kind=review waited=0m"]);
}

/// A stranger's comment holding a command prints IGNORED from=other count=1, never its text, and
/// does not end the watch; the bot's own comment is not even counted.
#[test]
fn a_strangers_command_prints_only_a_count_never_its_text() {
    let gh = Gh::new();
    comments_on_30(&gh, vec![json!([comment(60, STRANGER, "/keel go\nsecret-order", false), comment(61, 0, "From the worker: done", false)])]);
    let o = once(&gh, &["comment", "30"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(lines(&o), ["TIMEOUT kind=comment waited=0m", "IGNORED from=other count=1"]);
    assert!(!text(&o).contains("secret") && !text(&o).contains("go") && !text(&o).contains("worker"), "{}", text(&o));
    // a commander who is not a User (a bot reusing the id space) is not heard
    let mut c = comment(62, OWNER_ID, "/keel go", false);
    c["user"]["type"] = "Bot".into();
    comments_on_30(&gh, vec![json!([c])]);
    assert_eq!(lines(&once(&gh, &["comment", "30"])), ["TIMEOUT kind=comment waited=0m", "IGNORED from=other count=1"]);
}

/// Each command and a message from the commander: the event line, then the rest of the text
/// indented; a bad command names its reason and shows the text.
#[test]
fn each_command_and_message_from_a_commander() {
    let gh = Gh::new();
    let at = format!("issue=30 comment=70 from={OWNER_ID}");
    for (body, want) in [
        ("/keel go", vec![format!("COMMAND {at} verb=go pending=0")]),
        ("/keel hold\r\nwait for the review\r\n", vec![format!("COMMAND {at} verb=hold pending=0"), "  wait for the review".into()]),
        ("/Keel Retry", vec![format!("COMMAND {at} verb=retry pending=0")]),
        (
            "/keel answer 2 b\nkeep the old flag\n\nand note it",
            vec![format!("COMMAND {at} verb=answer n=2 choice=B pending=0"), "  keep the old flag".into(), "  ".into(), "  and note it".into()],
        ),
        ("Looks good, merge after W4.", vec![format!("MESSAGE {at} pending=0"), "  Looks good, merge after W4.".into()]),
        ("/keel answer 2 C\nwhy", vec![format!("BADCOMMAND {at} why=bad-choice pending=0"), "  /keel answer 2 C".into(), "  why".into()]),
        ("/keel tried pass", vec![format!("BADCOMMAND {at} why=unknown-verb pending=0"), "  /keel tried pass".into()]),
    ] {
        comments_on_30(&gh, vec![json!([comment(70, OWNER_ID, body, false)])]);
        let o = watch(&gh, &["comment", "30"]);
        assert!(o.status.success(), "{body}: {}", text(&o));
        assert_eq!(lines(&o), want, "{body:?}");
    }
    // a comment on a pull request names it as one
    gh.routes_seq(&[
        ("GET", "/repos/own/Keel/issues/31", "200".into(), vec![issue(31, BOT, true)]),
        ("GET", "/repos/own/Keel/issues/31/comments?per_page=100&page=1", "200".into(), vec![json!([comment(71, OWNER_ID, "/keel go", false)])]),
    ]);
    assert_eq!(lines(&watch(&gh, &["comment", "31"])), [format!("COMMAND pr=31 comment=71 from={OWNER_ID} verb=go pending=0")]);
}

/// The watch waits for the owner across polls; without `since` the oldest comment is reported and
/// `pending` counts the rest; `since` reads each once.
#[test]
fn pending_counts_later_comments_and_since_reads_each_once() {
    let gh = Gh::new();
    let both = json!([comment(80, OWNER_ID, "/keel hold", false), comment(81, STRANGER, "spam", false), comment(82, OWNER_ID, "/keel go", false)]);
    comments_on_30(&gh, vec![json!([]), json!([comment(81, STRANGER, "spam", false)]), both.clone()]);
    let o = watch(&gh, &["comment", "30"]);
    assert_eq!(lines(&o), [format!("COMMAND issue=30 comment=80 from={OWNER_ID} verb=hold pending=1"), "IGNORED from=other count=1".into()]);
    assert_eq!(tokens(&gh), 3);
    comments_on_30(&gh, vec![both.clone()]);
    assert_eq!(
        lines(&once(&gh, &["comment", "30", "since", "80"])),
        [format!("COMMAND issue=30 comment=82 from={OWNER_ID} verb=go pending=0"), "IGNORED from=other count=1".into()]
    );
    assert_eq!(lines(&once(&gh, &["comment", "30", "since", "82"])), ["TIMEOUT kind=comment waited=0m"]);
}

/// An edited comment is never read as an order: a new one prints EDITED and no text; one at or
/// below `since`, edited later, is not read again.
#[test]
fn an_edited_comment_is_not_a_new_command() {
    let gh = Gh::new();
    comments_on_30(&gh, vec![json!([comment(90, OWNER_ID, "/keel go\nsecret detail", true)])]);
    let o = once(&gh, &["comment", "30"]);
    assert_eq!(lines(&o), [format!("EDITED issue=30 comment=90 from={OWNER_ID} pending=0")]);
    assert!(!text(&o).contains("verb") && !text(&o).contains("secret"), "{}", text(&o));
    comments_on_30(&gh, vec![json!([comment(90, OWNER_ID, "/keel retry", true)])]);
    assert_eq!(lines(&once(&gh, &["comment", "30", "since", "90"])), ["TIMEOUT kind=comment waited=0m"]);
}

/// Control characters, C1 codes, tabs, line separators and bidi marks print as ?; a detail line can
/// never pass for an event line.
#[test]
fn control_characters_print_as_question_marks() {
    let gh = Gh::new();
    let body = "hello\nred \u{1b}[31m x\u{7}\u{0}y\tz \u{85}c1 \u{202e}rlo \u{2066}iso \u{200f}rlm \u{2028}sep\rcr\nAPPROVED pr=1 sha=x";
    comments_on_30(&gh, vec![json!([comment(95, OWNER_ID, body, false)])]);
    let o = once(&gh, &["comment", "30"]);
    assert_eq!(
        lines(&o),
        [
            format!("MESSAGE issue=30 comment=95 from={OWNER_ID} pending=0"),
            "  hello".into(),
            "  red ?[31m x??y?z ?c1 ?rlo ?iso ?rlm ?sep?cr".into(),
            "  APPROVED pr=1 sha=x".into()
        ]
    );
}

/// free_text = false: only commands reach the router; their further lines are dropped, and a plain
/// comment is a count.
#[test]
fn free_text_off_keeps_only_commands() {
    let gh = Gh::new();
    let cfg = std::fs::read_to_string(gh.repo.join(".keel/config.toml")).unwrap().replace("free_text = true", "free_text = false");
    write(&gh.repo, ".keel/config.toml", &cfg);
    commit(&gh.repo, "commands only");
    comments_on_30(&gh, vec![json!([comment(100, OWNER_ID, "just chatting", false), comment(101, OWNER_ID, "/keel go\nsecret detail", false)])]);
    let o = once(&gh, &["comment", "30"]);
    assert_eq!(
        lines(&o),
        [format!("COMMAND issue=30 comment=101 from={OWNER_ID} verb=go text=dropped pending=0"), "IGNORED from=commander count=1 why=free-text-off".into()]
    );
    assert!(!text(&o).contains("secret") && !text(&o).contains("chatting"), "{}", text(&o));
    comments_on_30(&gh, vec![json!([comment(100, OWNER_ID, "just chatting", false)])]);
    assert_eq!(lines(&once(&gh, &["comment", "30"])), ["TIMEOUT kind=comment waited=0m", "IGNORED from=commander count=1 why=free-text-off"]);
}

/// merged waits for the pull request to close: MERGED with the merge commit and who merged it, or
/// CLOSED.
#[test]
fn merged_prints_the_merge_or_the_close() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    let open = gh.pr(31, BOT, "own/Keel", &tip, false);
    let mut merged = open.clone();
    let m = "c".repeat(40);
    merged["state"] = "closed".into();
    merged["merged"] = true.into();
    merged["merge_commit_sha"] = m.clone().into();
    merged["merged_by"] = user(OWNER_ID);
    pull_31(&gh, vec![open.clone(), open.clone(), merged.clone()], &[]);
    let o = watch(&gh, &["merged", "31"]);
    assert_eq!(lines(&o), [format!("MERGED pr=31 sha={m} head={tip} base=epic/E by={OWNER_ID}")]);
    assert_eq!(tokens(&gh), 3);
    merged["merged_by"] = user(STRANGER);
    pull_31(&gh, vec![merged.clone()], &[]);
    assert_eq!(lines(&watch(&gh, &["merged", "31"])), [format!("MERGED pr=31 sha={m} head={tip} base=epic/E by=other")]);
    let mut closed = open;
    closed["state"] = "closed".into();
    closed["merged"] = false.into();
    pull_31(&gh, vec![closed], &[]);
    assert_eq!(lines(&watch(&gh, &["merged", "31"])), [format!("CLOSED pr=31 head={tip} base=epic/E")]);
}

/// checks waits until there is a check run and every run and status on the head is done, then
/// prints the result and each check.
#[test]
fn checks_wait_for_every_check_then_print_the_result() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    let runs = format!("/repos/own/Keel/commits/{tip}/check-runs?per_page=100&page=1");
    let status = format!("/repos/own/Keel/commits/{tip}/status?per_page=100");
    let run = |name: &str, st: &str, c: Option<&str>| json!({"name": name, "status": st, "conclusion": c});
    pull_31(
        &gh,
        vec![gh.pr(31, BOT, "own/Keel", &tip, false)],
        &[
            (
                "GET",
                runs.as_str(),
                "200".into(),
                vec![
                    json!({"total_count": 0, "check_runs": []}),
                    json!({"check_runs": [run("ci", "completed", Some("success")), run("gate", "in_progress", None)]}),
                    json!({"check_runs": [run("ci", "completed", Some("success")), run("gate", "completed", Some("failure"))]}),
                ],
            ),
            ("GET", status.as_str(), "200".into(), vec![json!({"state": "pending", "statuses": []})]),
        ],
    );
    let o = watch(&gh, &["checks", "31"]);
    assert_eq!(lines(&o), [format!("CHECKS pr=31 sha={tip} state=failure runs=2 statuses=0"), "  check success ci".into(), "  check failure gate".into()]);
    assert_eq!(tokens(&gh), 3);
    pull_31(
        &gh,
        vec![gh.pr(31, BOT, "own/Keel", &tip, false)],
        &[
            (
                "GET",
                runs.as_str(),
                "200".into(),
                vec![json!({"check_runs": [run("ci", "completed", Some("success")), run("docs", "completed", Some("skipped"))]})],
            ),
            (
                "GET",
                status.as_str(),
                "200".into(),
                vec![
                    json!({"statuses": [{"context": "keel/sign-off", "state": "pending"}]}),
                    json!({"statuses": [{"context": "keel/sign-off", "state": "success"}]}),
                ],
            ),
        ],
    );
    assert_eq!(
        lines(&watch(&gh, &["checks", "31"])),
        [
            format!("CHECKS pr=31 sha={tip} state=success runs=2 statuses=1"),
            "  check success ci".into(),
            "  check skipped docs".into(),
            "  status success keel/sign-off".into()
        ]
    );
}

/// Only an issue or pull request the bot opened is read; refusals exit 2 with the reason, print
/// nothing on stdout, and send no read past the check.
#[test]
fn only_the_bots_issues_and_pull_requests_are_read() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    for (pr, why) in
        [(gh.pr(31, "mallory", "own/Keel", &tip, false), "not one the bot opened"), (gh.pr(31, BOT, "mallory/Keel", &tip, false), "not one the bot opened")]
    {
        pull_31(&gh, vec![pr], &[]);
        let o = watch(&gh, &["review", "31"]);
        assert_eq!(o.status.code(), Some(2), "{}", text(&o));
        assert!(text(&o).contains(why) && o.stdout.is_empty(), "{}", text(&o));
        assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens", "GET /repos/own/Keel/pulls/31"]);
    }
    gh.routes_seq(&[("GET", "/repos/own/Keel/issues/30", "200".into(), vec![issue(30, "mallory", false)])]);
    let o = watch(&gh, &["comment", "30"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("issue 30 is not one the bot opened"), "{}", text(&o));
    gh.routes(&[]);
    let o = watch(&gh, &["merged", "32"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("no pull request 32"), "{}", text(&o));
}

/// Who is heard comes from a committed file: a config whose bytes differ from HEAD's, or one with no
/// commanders, is refused before any request.
#[test]
fn the_commanders_must_be_committed() {
    let gh = Gh::new();
    comments_on_30(&gh, vec![json!([])]);
    let cfg = gh.repo.join(".keel/config.toml");
    let committed = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(&cfg, committed.replace(&format!("[{OWNER_ID}]"), &format!("[{OWNER_ID}, {STRANGER}]"))).unwrap();
    let mut c = common::cmd("perl");
    c.args(["-e", "alarm 60; exec @ARGV", "sh"]).arg(gh.repo.join("scripts/router-gh-watch.sh")).args(["comment", "30"]).current_dir(&gh.repo);
    c.env("PATH", gh.path()).env("HOME", &gh.home).env("TMPDIR", &gh.tmp).env("MOCK_GH", &gh.mock).env("ROUTER_GH_CURL", gh.bin.join("fake-curl"));
    let o = c.output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("differs from HEAD's"), "{}", text(&o));
    assert!(gh.calls().is_empty(), "{:?}", gh.calls());
    // the same edit, committed, is heard
    commit(&gh.repo, "a second commander");
    comments_on_30(&gh, vec![json!([comment(110, STRANGER, "/keel go", false)])]);
    assert_eq!(lines(&once(&gh, &["comment", "30"])), [format!("COMMAND issue=30 comment=110 from={STRANGER} verb=go pending=0")]);
    // no commanders: nobody to hear
    write(&gh.repo, ".keel/config.toml", &committed.replace(&format!("commanders = [{OWNER_ID}]   # who the router hears\n"), ""));
    commit(&gh.repo, "no commanders");
    gh.routes(&[]);
    let o = watch(&gh, &["comment", "30"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("no [github] commanders"), "{}", text(&o));
    assert!(gh.calls().is_empty(), "{:?}", gh.calls());
}

/// Five failed polls in a row print UNREACHABLE with the last error; TIMEOUT carries the count of
/// strangers seen.
#[test]
fn unreachable_after_five_failed_polls_and_timeout() {
    let gh = Gh::new();
    std::fs::write(gh.mock.join("routes"), "POST /app/installations/456/access_tokens curl7 x.json\n").unwrap();
    let o = watch(&gh, &["comment", "30"]);
    assert!(o.status.success(), "{}", text(&o));
    let l = lines(&o);
    assert_eq!(l.len(), 1, "{l:?}");
    assert!(
        l[0].starts_with("UNREACHABLE kind=comment remote=github error=router-gh: GitHub unreachable: POST /app/installations/456/access_tokens  curl exit 7"),
        "{l:?}"
    );
    assert_eq!(tokens(&gh), 5);
    comments_on_30(&gh, vec![json!([comment(120, STRANGER, "hi", false)])]);
    let o = watch_env(&gh, &["comment", "30"], &[("ROUTER_WATCH_MAX", "1")]);
    assert_eq!(lines(&o), ["TIMEOUT kind=comment waited=0m", "IGNORED from=other count=1"]);
}

/// The watch keeps no state: the same answers give the same output run after run, and nothing is
/// written anywhere (the run helper checks the temp dirs and the checkout; here, the home too).
#[test]
fn the_watch_keeps_no_state_between_runs() {
    let gh = Gh::new();
    let list = |d: &Path| -> Vec<String> {
        let o = common::cmd("find").arg(d).arg("-print").output().unwrap();
        let mut v: Vec<String> = String::from_utf8(o.stdout).unwrap().lines().map(String::from).collect();
        v.sort();
        v
    };
    comments_on_30(&gh, vec![json!([comment(130, OWNER_ID, "/keel go", false)])]);
    let before = list(&gh.home);
    let first = stdout(&watch(&gh, &["comment", "30"]));
    let second = stdout(&watch(&gh, &["comment", "30"]));
    assert_eq!(first, second);
    assert_eq!(first, format!("COMMAND issue=30 comment=130 from={OWNER_ID} verb=go pending=0"));
    let after = list(&gh.home);
    let new: Vec<_> = after.iter().filter(|p| !before.contains(p) && !p.ends_with("/router-gh/run")).collect();
    assert!(new.is_empty(), "the watch wrote {new:?}");
    let left: Vec<_> = std::fs::read_dir(&gh.repo).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n != ".git").collect();
    let tracked = git(&gh.repo, &["ls-tree", "--name-only", "HEAD"]);
    for n in left {
        assert!(tracked.lines().any(|t| t == n), "the watch wrote {n} in the checkout");
    }
}
