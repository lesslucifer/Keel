//! E-RGUARD.W2: the router's owner-notify sender, spawned as the router runs it.
//! The script posts one message to the paired Telegram bot and prints its id; these tests
//! pin its three modes against a local mock API, its failure line, the no-redirection rule,
//! the chat it actually addresses, and that neither the credential nor the chat id reaches
//! stdout or stderr.
//!
//! E-APPLIST.W3: before it sends, the sender writes the router's alert line (and a question line)
//! through its sibling `scripts/router-app.sh`, which finds the update log from its own directory.
//! So every case runs copies of both scripts in its own temp git repo: no test reaches a real
//! checkout's `.router/app/`. The body's header parse is driven as a unit through python3.
compiled!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::common;

/// A credential-shaped fake. The sender must never print it; a test failure never formats it.
const TOKEN: &str = "123456789:AAHfakeTOKENfakeTOKENfakeTOKENfake00";
const CHAT: &str = "4242";

/// An address nothing answers on without binding a port (worker sandboxes refuse a bind): a send
/// to it fails after the sender's log writes, so the lines can be judged with no mock.
const UNREACHED: &str = "http://127.0.0.1:9";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn git(dir: &Path, args: &[&str]) {
    let o = common::cmd("git").args(args).current_dir(dir).envs(common::TESTER).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A temp git repo that is also the sender's home: the channel state it reads (a token file and
/// the paired chat), copies of `scripts/notify-owner.py` and `scripts/router-app.sh`, and a git
/// shim (router-app.sh clears every GIT_*). Its update log is `<dir>/.router/app/`.
fn channel() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), format!("TELEGRAM_BOT_TOKEN={TOKEN}\n")).unwrap();
    std::fs::write(dir.path().join("access.json"), format!(r#"{{"dmPolicy":"pairing","allowFrom":["{CHAT}"],"groups":{{}},"pending":{{}}}}"#)).unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    for f in ["scripts/notify-owner.py", "scripts/router-app.sh"] {
        std::fs::copy(root().join(f), dir.path().join(f)).unwrap();
    }
    common::git_shim(&dir.path().join(".shim"));
    git(dir.path(), &["init", "-q", "-b", "main"]);
    dir
}

/// `channel()` with the update log started, as a router does at its start.
fn started() -> TempDir {
    let dir = channel();
    let o = app(&dir).args(["start", "test-router"]).output().unwrap();
    assert!(o.status.success(), "start failed: {}", String::from_utf8_lossy(&o.stderr));
    dir
}

/// The temp repo's copy of router-app.sh, as a test runs it directly.
fn app(dir: &TempDir) -> Command {
    let mut c = common::cmd("sh");
    c.arg(dir.path().join("scripts/router-app.sh")).env("PATH", common::path_with(dir.path().join(".shim"))).current_dir(dir.path());
    c
}

fn log_path(dir: &TempDir) -> PathBuf {
    dir.path().join(".router/app/log.jsonl")
}

/// The log's lines after `start`, each parsed.
fn written(dir: &TempDir) -> Vec<serde_json::Value> {
    let raw = std::fs::read_to_string(log_path(dir)).unwrap_or_default();
    raw.lines().skip(1).map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not one JSON line ({e}): {l:?}"))).collect()
}

/// The sender spawned with HOME in the test's temp dir and no proxy, so a regression that falls
/// back to the default channel files can never read the real token, and no request leaves loopback;
/// bytecode off, so no run writes a cache into the checkout. It is the temp repo's copy, so its
/// sibling router-app.sh writes the temp repo's log.
fn sender(home: &Path) -> Command {
    let mut command = common::cmd(home.join("scripts/notify-owner.py"));
    command.env("HOME", home).env("PYTHONDONTWRITEBYTECODE", "1").env("PATH", common::path_with(home.join(".shim")));
    for proxy in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        command.env_remove(proxy);
    }
    command
}

/// Run the sender as the router does: the file itself, no interpreter named on the command line.
/// The KEEL_NOTIFY_* variables are the only redirection, and only this test sets them.
fn run_with(dir: &TempDir, args: &[&str], api_base: &str, extra: &[(&str, &str)]) -> Output {
    let mut command = sender(dir.path());
    command
        .args(args)
        .env("KEEL_NOTIFY_ENV", dir.path().join(".env"))
        .env("KEEL_NOTIFY_ACCESS", dir.path().join("access.json"))
        .env("KEEL_NOTIFY_API_BASE", api_base)
        .current_dir(dir.path())
        .stdin(Stdio::null());
    for (key, value) in extra {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn run(dir: &TempDir, args: &[&str], api_base: &str) -> Output {
    run_with(dir, args, api_base, &[])
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

fn assert_no_credential(out: &Output) {
    let shown = stdout(out) + &stderr(out);
    assert!(!shown.contains(TOKEN), "the credential reached the output");
    assert!(!shown.contains(CHAT), "the chat id reached the output");
}

/// The JSON body the mock captured, so a test can assert which chat the sender addressed.
fn sent_payload(request: &str) -> serde_json::Value {
    let body = request.split("\r\n\r\n").nth(1).expect("the request has no body");
    serde_json::from_str(body).expect("the send body is not JSON")
}

/// A port nothing listens on: a dry run that tried to send would fail to connect.
fn closed_base() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

/// One request, one canned response; the request bytes come back on the channel.
fn mock(status: &'static str, body: &'static str) -> (String, Receiver<String>, JoinHandle<()>) {
    let (base, seen, server) = serve(status, body, None);
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        if let Ok((request, _)) = seen.recv() {
            let _ = tx.send(request);
        }
        server.join().unwrap();
    });
    (base, rx, handle)
}

/// A mock's address, the channel bringing back each request with the log it saw, and its thread.
type Served = (String, Receiver<(String, Option<String>)>, JoinHandle<()>);

/// `mock`, and the bytes of `log` read the moment the request arrives, before any answer: what
/// the update log held when the sender called Telegram (None for an absent log).
fn mock_seeing(status: &'static str, body: &'static str, log: PathBuf) -> Served {
    serve(status, body, Some(log))
}

fn serve(status: &'static str, body: &'static str, log: Option<PathBuf>) -> Served {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let held = log.and_then(|p| std::fs::read_to_string(p).ok());
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(head) = text.find("\r\n\r\n") {
                let length = text[..head]
                    .lines()
                    .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                    .unwrap_or(0);
                if buf.len() >= head + 4 + length {
                    break;
                }
            }
        }
        tx.send((String::from_utf8_lossy(&buf).to_string(), held)).unwrap();
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        stream.write_all(response.as_bytes()).unwrap();
    });
    (format!("http://{addr}"), rx, handle)
}

/// A loopback server that accepts and never answers, so an injectable timeout can be exercised.
fn stalled_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
    format!("http://{addr}")
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn dry_run_prints_the_request_without_the_chat_or_a_send() {
    let dir = channel();
    let base = closed_base();
    let out = run(&dir, &["--dry-run", "Owner action\ncd /keel && keel mr confirm wu abc --as vuluu"], &base);
    assert!(out.status.success(), "dry run failed: {}", stderr(&out));
    let shown = stdout(&out);
    assert!(shown.contains("dry-run:") && shown.contains("no request sent"), "{shown}");
    assert!(shown.contains("cd /keel && keel mr confirm wu abc --as vuluu"), "{shown}");
    assert!(!shown.contains(CHAT), "the chat id is in the dry-run line");
    assert_no_credential(&out);
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn check_names_the_bot_and_sends_no_message() {
    let dir = channel();
    let (base, request, server) = mock("200 OK", r#"{"ok":true,"result":{"id":42,"is_bot":true,"first_name":"Keel Test","username":"keel_test_bot"}}"#);
    let out = run(&dir, &["--check"], &base);
    let seen = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "check failed: {}", stderr(&out));
    let shown = stdout(&out);
    assert!(shown.contains("@keel_test_bot"), "{shown}");
    assert!(seen.contains("/getMe") && !seen.contains("/sendMessage"), "check called the wrong method");
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn check_failure_is_one_line_and_never_prints_the_credential() {
    let dir = channel();
    let (base, request, server) = mock("401 Unauthorized", r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#);
    let out = run(&dir, &["--check"], &base);
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(!out.status.success(), "a 401 check must fail");
    let shown = stdout(&out);
    let problem = stderr(&out);
    assert!(shown.is_empty(), "a failure writes stdout: {shown}");
    assert_eq!(problem.lines().count(), 1, "a failure is one line: {problem:?}");
    assert!(problem.contains("401"), "{problem}");
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn send_prints_the_id_and_never_the_credential() {
    let dir = channel();
    let (base, request, server) = mock("200 OK", r#"{"ok":true,"result":{"message_id":1731}}"#);
    let out = run(&dir, &["--send", "hello owner"], &base);
    let seen = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "send failed: {}", stderr(&out));
    assert_eq!(stdout(&out), "id 1731\n");
    let payload = sent_payload(&seen);
    assert_eq!(payload["chat_id"].as_str(), Some(CHAT), "the wrong chat was addressed");
    assert_eq!(payload["text"].as_str(), Some("hello owner"));
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn bare_send_reads_and_clears_the_body_so_it_cannot_resend_a_stale_message() {
    let dir = channel();
    let body = dir.path().join(".router/notify/owner.md");
    std::fs::create_dir_all(body.parent().unwrap()).unwrap();
    std::fs::write(&body, "Sign-off ready\ncd /keel && keel mr confirm wu 1a2b3c --as vuluu\n").unwrap();
    let (base, request, server) = mock("200 OK", r#"{"ok":true,"result":{"message_id":7}}"#);
    let out = run(&dir, &["--send"], &base);
    let seen = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "send failed: {}", stderr(&out));
    assert_eq!(stdout(&out), "id 7\n");
    let payload = sent_payload(&seen);
    assert_eq!(payload["chat_id"].as_str(), Some(CHAT), "the wrong chat was addressed");
    assert!(payload["text"].as_str().is_some_and(|t| t.contains("Sign-off ready") && t.contains("keel mr confirm")), "send carried the wrong body");
    assert_no_credential(&out);
    assert!(std::fs::read_to_string(&body).unwrap().is_empty(), "the sent body was left in place and a later bare --send would resend it");
    let again = run(&dir, &["--send"], &closed_base());
    assert!(!again.status.success(), "a cleared body must not send");
    let problem = stderr(&again);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("empty"), "a cleared body must fail as empty: {problem}");
    assert_no_credential(&again);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn preview_and_failed_send_leave_the_body_in_place() {
    let dir = channel();
    let body = dir.path().join(".router/notify/owner.md");
    std::fs::create_dir_all(body.parent().unwrap()).unwrap();
    std::fs::write(&body, "Keep me\n").unwrap();
    let preview = run(&dir, &["--dry-run"], &closed_base());
    assert!(preview.status.success(), "a bare dry run failed: {}", stderr(&preview));
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "Keep me\n", "a preview truncated the body");
    let (base, request, server) = mock("500 Internal Server Error", r#"{"ok":false,"error_code":500,"description":"boom"}"#);
    let failed = run(&dir, &["--send"], &base);
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(!failed.status.success(), "a 500 send must fail");
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "Keep me\n", "a failed send truncated the body");
    assert_no_credential(&preview);
    assert_no_credential(&failed);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn a_read_only_body_fails_before_sending_and_names_the_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = channel();
    let body = dir.path().join(".router/notify/owner.md");
    std::fs::create_dir_all(body.parent().unwrap()).unwrap();
    std::fs::write(&body, "Owner action\n").unwrap();
    std::fs::set_permissions(&body, std::fs::Permissions::from_mode(0o444)).unwrap();
    let out = run(&dir, &["--send"], &closed_base());
    assert!(!out.status.success(), "a read-only body must fail");
    assert!(stdout(&out).is_empty(), "a failure writes stdout");
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("owner.md") && problem.contains("not writable") && problem.contains("not sent"), "{problem}");
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "Owner action\n", "the body was changed");
    std::fs::set_permissions(&body, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_no_credential(&out);
}

#[test]
fn a_missing_credential_is_one_line_naming_the_file_only() {
    let dir = channel();
    let missing = dir.path().join("absent.env");
    let out = sender(dir.path())
        .arg("--check")
        .env("KEEL_NOTIFY_ENV", &missing)
        .env("KEEL_NOTIFY_ACCESS", dir.path().join("access.json"))
        .env("KEEL_NOTIFY_API_BASE", "http://127.0.0.1:9")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "a missing credential must fail");
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "a failure is one line: {problem:?}");
    assert!(problem.contains("absent.env"), "{problem}");
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn bare_send_without_a_body_is_one_line_and_sends_nothing() {
    let dir = channel();
    let out = run(&dir, &["--send"], &closed_base());
    assert!(!out.status.success(), "a missing body must fail");
    assert!(stdout(&out).is_empty(), "a failure writes stdout");
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("owner.md"), "the failure names the body file: {problem}");
    assert_no_credential(&out);
}

#[test]
fn cleartext_http_is_refused_for_a_non_loopback_host() {
    let dir = channel();
    let out = run(&dir, &["--check"], "http://evil.invalid");
    assert!(!out.status.success(), "cleartext http to another host must be refused");
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("https"), "{problem}");
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn a_malformed_token_never_echoes_the_request_target() {
    let dir = channel();
    std::fs::write(dir.path().join(".env"), "TELEGRAM_BOT_TOKEN=bad token:secret\n").unwrap();
    let out = run(&dir, &["--check"], &closed_base());
    assert!(!out.status.success(), "a malformed token must fail");
    let shown = stdout(&out) + &stderr(&out);
    assert_eq!(shown.lines().count(), 1, "{shown:?}");
    assert!(!shown.contains("bad token") && !shown.contains("secret"), "{shown}");
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn a_stalled_server_times_out_in_one_line() {
    let dir = channel();
    let base = stalled_server();
    let started = Instant::now();
    let out = run_with(&dir, &["--check"], &base, &[("KEEL_NOTIFY_TIMEOUT", "0.2")]);
    assert!(!out.status.success(), "a stalled server must fail");
    assert!(started.elapsed() < Duration::from_secs(5), "the timeout was not injectable: {:?}", started.elapsed());
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("Telegram API"), "{problem}");
    assert_no_credential(&out);
}

// E-APPLIST.W3 (K10, K12): the alert and question lines, written before Telegram, and the link.

fn write_body(dir: &TempDir, text: &str) -> PathBuf {
    let body = dir.path().join(".router/notify/owner.md");
    std::fs::create_dir_all(body.parent().unwrap()).unwrap();
    std::fs::write(&body, text).unwrap();
    body
}

/// The one-line failure of a send that never reached Telegram: it names the API.
fn assert_send_was_tried(out: &Output) {
    assert!(!out.status.success(), "a send to {UNREACHED} must fail");
    assert!(stderr(out).contains("Telegram API"), "the sender never tried the send: {}", stderr(out));
}

fn alert_lines(dir: &TempDir) -> Vec<serde_json::Value> {
    written(dir).into_iter().filter(|l| l["op"] == "alert").collect()
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn a_send_writes_one_alert_line_before_the_message() {
    let dir = started();
    write_body(&dir, "wu: E-X.W2\nPlan to read: E-X.W2\nOpen the plan\n");
    let (base, request, server) = mock_seeing("200 OK", r#"{"ok":true,"result":{"message_id":5}}"#, log_path(&dir));
    let out = run(&dir, &["--send"], &base);
    let (_, held) = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "send failed: {}", stderr(&out));
    assert_eq!(stdout(&out), "desk alert n=2\nid 5\n");
    let held = held.expect("the log was absent when Telegram was called");
    let at_send: Vec<serde_json::Value> = held.lines().skip(1).map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(at_send.len(), 1, "the log held {} lines past start when Telegram was called: {held}", at_send.len());
    assert_eq!(at_send[0]["op"], "alert");
    assert_eq!(
        at_send[0]["value"],
        serde_json::json!({"headline": "Plan to read: E-X.W2", "lines": ["Open the plan"], "wu": "E-X.W2", "link": "keel://wu/E-X.W2"})
    );
    assert_eq!(written(&dir).len(), 1, "a send writes one line");
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn the_message_ends_with_the_link_when_the_body_names_a_work_unit() {
    let dir = started();
    let (base, request, server) = mock("200 OK", r#"{"ok":true,"result":{"message_id":6}}"#);
    let out = run(&dir, &["--send", "wu: E-X.W2\nSign-off ready: E-X.W2\ncd /keel && keel mr confirm wu abc --as vuluu\n"], &base);
    let seen = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "send failed: {}", stderr(&out));
    let payload = sent_payload(&seen);
    assert_eq!(
        payload["text"].as_str(),
        Some("Sign-off ready: E-X.W2\ncd /keel && keel mr confirm wu abc --as vuluu\nkeel://wu/E-X.W2"),
        "the header was sent or the link is not the last line"
    );
    assert_no_credential(&out);
}

#[test]
fn a_body_naming_no_work_unit_has_no_link() {
    let dir = started();
    let preview = run(&dir, &["--dry-run", "Main is not pushed\ncd /keel && git push origin main"], UNREACHED);
    assert!(preview.status.success(), "dry run failed: {}", stderr(&preview));
    assert!(!stdout(&preview).contains("keel://"), "a body with no work unit got a link: {}", stdout(&preview));
    let out = run(&dir, &["--send", "Main is not pushed\ncd /keel && git push origin main"], UNREACHED);
    assert_send_was_tried(&out);
    let alerts = alert_lines(&dir);
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert_eq!(alerts[0]["value"], serde_json::json!({"headline": "Main is not pushed", "lines": ["cd /keel && git push origin main"]}));
}

#[test]
fn a_question_header_writes_the_question_line_before_the_alert() {
    let dir = started();
    let out = run(&dir, &["--send", "question: link-scheme\nwu: E-X.W2\nKeep keel:// as the link?\nReply yes or no\n"], UNREACHED);
    assert_send_was_tried(&out);
    let lines = written(&dir);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["op"], "set");
    assert_eq!(lines[0]["key"], "question/link-scheme");
    assert_eq!(lines[0]["value"], serde_json::json!({"headline": "Keep keel:// as the link?", "lines": ["Reply yes or no"], "wu": "E-X.W2"}));
    assert_eq!(lines[1]["op"], "alert", "the alert line comes after the question line");
    assert_eq!(lines[1]["value"]["link"], "keel://wu/E-X.W2");
}

#[test]
fn a_failed_send_still_leaves_the_alert_line() {
    let dir = started();
    let body = write_body(&dir, "wu: E-X.W2\nResult to sign: E-X.W2\n");
    let out = run(&dir, &["--send"], UNREACHED);
    assert_send_was_tried(&out);
    assert!(stderr(&out).contains("desk alert was written"), "the failure does not say the desk alert stands: {}", stderr(&out));
    assert!(stdout(&out).is_empty(), "a failed send printed {}", stdout(&out));
    let alerts = alert_lines(&dir);
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert_eq!(alerts[0]["value"]["wu"], "E-X.W2");
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "wu: E-X.W2\nResult to sign: E-X.W2\n", "a failed send changed the body");
    assert_no_credential(&out);
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn a_refused_or_stalled_send_still_leaves_the_alert_line() {
    let dir = started();
    let (base, request, server) = mock("500 Internal Server Error", r#"{"ok":false,"error_code":500,"description":"boom"}"#);
    let refused = run(&dir, &["--send", "wu: E-X.W2\nPlan to read\n"], &base);
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(!refused.status.success(), "a 500 send must fail");
    let stalled = run_with(&dir, &["--send", "wu: E-X.W2\nBuilding\n"], &stalled_server(), &[("KEEL_NOTIFY_TIMEOUT", "0.2")]);
    assert!(!stalled.status.success(), "a stalled send must fail");
    assert_eq!(alert_lines(&dir).len(), 2, "each failed send keeps its alert line");
    assert_no_credential(&refused);
    assert_no_credential(&stalled);
}

#[test]
fn a_log_that_cannot_be_written_still_tries_the_send_and_says_so() {
    // no run started
    let none = channel();
    // no router-app.sh beside the sender
    let alone = started();
    std::fs::remove_file(alone.path().join("scripts/router-app.sh")).unwrap();
    // a lock a killed writer left
    let locked = started();
    std::fs::create_dir(locked.path().join(".router/app/.lock")).unwrap();
    for (dir, why) in [(&none, "no run"), (&alone, "router-app.sh"), (&locked, "LOCKED")] {
        let out = run(dir, &["--send", "wu: E-X.W2\nPlan to read\n"], UNREACHED);
        assert_send_was_tried(&out);
        let problem = stderr(&out);
        assert!(problem.contains("no desk alert") && problem.contains(why), "the stderr does not say why no desk alert ({why}): {problem}");
        assert!(alert_lines(dir).is_empty(), "an alert line was written");
        assert_no_credential(&out);
    }
}

#[test]
#[ignore = "binds a loopback port; worker sandboxes refuse it (owner ruling 2026-11-15)"]
fn a_log_that_cannot_be_written_still_sends() {
    let dir = channel();
    let (base, request, server) = mock("200 OK", r#"{"ok":true,"result":{"message_id":9}}"#);
    let out = run(&dir, &["--send", "wu: E-X.W2\nPlan to read\n"], &base);
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "a log failure blocked the send: {}", stderr(&out));
    assert_eq!(stdout(&out), "id 9\n");
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("no desk alert"), "{problem}");
    assert_no_credential(&out);
}

#[test]
fn a_bad_header_sends_nothing_and_writes_nothing() {
    let dir = started();
    let before = std::fs::read(log_path(&dir)).unwrap();
    for (text, line) in [
        ("wu: nope\nPlan to read\n", "line 1"),
        ("wu: E-X.W2\nquestion: Bad_Id\nPlan to read\n", "line 2"),
        ("wu: E-X.W2\nwu: E-X.W3\nPlan to read\n", "line 2"),
    ] {
        let body = write_body(&dir, text);
        let out = run(&dir, &["--send"], UNREACHED);
        assert!(!out.status.success(), "a bad header must fail: {text:?}");
        let problem = stderr(&out);
        assert_eq!(problem.lines().count(), 1, "{problem:?}");
        assert!(problem.contains(line), "the failure does not name {line}: {problem}");
        assert!(!problem.contains("Telegram API"), "a bad header reached the send: {problem}");
        assert!(stdout(&out).is_empty());
        assert_eq!(std::fs::read(log_path(&dir)).unwrap(), before, "a bad header wrote to the log");
        assert_eq!(std::fs::read_to_string(&body).unwrap(), text, "a bad header changed the body");
        assert_no_credential(&out);
    }
}

#[test]
fn dry_run_writes_no_line_and_shows_the_link() {
    let dir = started();
    let before = std::fs::read(log_path(&dir)).unwrap();
    let out = run(&dir, &["--dry-run", "wu: E-X.W2\nquestion: q1\nPlan to read: E-X.W2\nOpen the plan\n"], UNREACHED);
    assert!(out.status.success(), "dry run failed: {}", stderr(&out));
    let shown = stdout(&out);
    assert!(shown.lines().any(|l| l == "keel://wu/E-X.W2"), "the message's link line is not shown: {shown}");
    assert!(!shown.lines().any(|l| l == "wu: E-X.W2" || l == "question: q1"), "the header lines are in the message: {shown}");
    assert!(shown.contains(r#""link":"keel://wu/E-X.W2""#), "the alert line is not shown: {shown}");
    assert!(shown.contains("question/q1"), "the question line is not shown: {shown}");
    assert_eq!(std::fs::read(log_path(&dir)).unwrap(), before, "a dry run wrote to the log");
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));
    assert_no_credential(&out);
}

#[test]
fn a_long_multibyte_body_fits_the_line() {
    let dir = started();
    let text = format!("question: long\nwu: E-X.W2\n{}\n{}\n{}\n", "ệ".repeat(120), "ữ".repeat(300), "ở".repeat(300));
    let out = run(&dir, &["--send", &text], UNREACHED);
    assert_send_was_tried(&out);
    let raw = std::fs::read_to_string(log_path(&dir)).unwrap();
    for line in raw.lines() {
        assert!(line.len() <= 1024, "a line is {} bytes", line.len());
    }
    let lines = written(&dir);
    assert_eq!(lines.len(), 2, "the question and the alert were not both written: {lines:?}\n{}", stderr(&out));
    let headline = lines[1]["value"]["headline"].as_str().unwrap();
    assert!(headline.starts_with('ệ') && headline.ends_with('…'), "the headline was not cut with an ellipsis: {headline}");
}

#[test]
fn no_line_carries_the_credential_or_the_chat() {
    let dir = started();
    let out = run(&dir, &["--send", "question: q1\nwu: E-X.W2\nSign-off ready\ncd /keel\n"], UNREACHED);
    assert_send_was_tried(&out);
    assert_eq!(written(&dir).len(), 2, "{}", stderr(&out));
    for f in ["log.jsonl", "state.json"] {
        let raw = std::fs::read_to_string(dir.path().join(".router/app").join(f)).unwrap();
        assert!(!raw.contains(TOKEN), "{f} holds the credential");
        assert!(!raw.contains(CHAT), "{f} holds the chat id");
    }
}

// The body's header parse, as a unit: python3 loads the checkout's script by path (its __main__
// guard keeps main from running) and prints what parse_body and its builders make of a body. The
// bytes are decoded as Python decodes an argument (surrogateescape), and any lone surrogate left
// in the result is shown as U+FFFD, so the JSON stays valid for serde.

const DRIVE: &str = r#"
import importlib.util, json, sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("notify_owner", sys.argv[1])
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
text = sys.stdin.buffer.read().decode("utf-8", "surrogateescape")
def shown(v):
    if isinstance(v, str):
        return v.encode("utf-8", "surrogateescape").decode("utf-8", "replace")
    if isinstance(v, dict):
        return {k: shown(x) for k, x in v.items()}
    if isinstance(v, list):
        return [shown(x) for x in v]
    return v
try:
    p = m.parse_body(text)
    out = {"parsed": p, "alert": m.alert_value(p), "question": m.question_value(p), "message": m.message_text(p)}
except m.NotifyError as error:
    out = {"error": str(error)}
print(json.dumps(shown(out)))
"#;

fn parse(text: &str) -> serde_json::Value {
    parse_bytes(text.as_bytes())
}

fn parse_bytes(text: &[u8]) -> serde_json::Value {
    let mut child = common::cmd("python3")
        .args(["-c", DRIVE])
        .arg(root().join("scripts/notify-owner.py"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(text).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "python3 could not drive parse_body: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn parse_reads_a_wu_header_and_builds_the_link() {
    let got = parse("wu: E-APPLIST.W3\nPlan to read: W3\nOpen the plan\ncd /x && open p.html\n");
    assert_eq!(got["parsed"]["wu"], "E-APPLIST.W3");
    assert_eq!(got["parsed"]["question"], serde_json::Value::Null);
    assert_eq!(got["parsed"]["text"], "Plan to read: W3\nOpen the plan\ncd /x && open p.html\n");
    assert_eq!(
        got["alert"],
        serde_json::json!({"headline": "Plan to read: W3", "lines": ["Open the plan", "cd /x && open p.html"], "wu": "E-APPLIST.W3", "link": "keel://wu/E-APPLIST.W3"})
    );
    assert_eq!(got["question"], serde_json::Value::Null);
    assert_eq!(got["message"], "Plan to read: W3\nOpen the plan\ncd /x && open p.html\nkeel://wu/E-APPLIST.W3");
}

#[test]
fn parse_reads_a_question_header_alone_or_with_a_wu_in_either_order() {
    let alone = parse("question: link-scheme\nKeep keel://?\nYes or no\n");
    assert_eq!(alone["parsed"]["question"], "link-scheme");
    assert_eq!(alone["question"], serde_json::json!({"headline": "Keep keel://?", "lines": ["Yes or no"]}));
    assert_eq!(alone["alert"], serde_json::json!({"headline": "Keep keel://?", "lines": ["Yes or no"]}));
    assert_eq!(alone["message"], "Keep keel://?\nYes or no\n");
    for text in ["wu: E-A.W1\nquestion: q1\nHead\n", "question: q1\nwu: E-A.W1\nHead\n"] {
        let got = parse(text);
        assert_eq!((got["parsed"]["wu"].as_str(), got["parsed"]["question"].as_str()), (Some("E-A.W1"), Some("q1")), "{text:?}");
        assert_eq!(got["question"]["wu"], "E-A.W1", "{text:?}");
        assert_eq!(got["message"], "Head\nkeel://wu/E-A.W1", "{text:?}");
    }
}

#[test]
fn parse_without_a_header_keeps_the_body_byte_equal() {
    let text = "Sign-off ready\ncd /keel && keel mr confirm wu abc --as vuluu\n\n";
    let got = parse(text);
    assert_eq!(got["parsed"]["wu"], serde_json::Value::Null);
    assert_eq!(got["parsed"]["text"], text);
    assert_eq!(got["message"], text, "a body with no header must be sent as it is");
    assert_eq!(got["alert"], serde_json::json!({"headline": "Sign-off ready", "lines": ["cd /keel && keel mr confirm wu abc --as vuluu"]}));
}

#[test]
fn parse_reads_a_header_like_line_after_the_text_as_text() {
    let got = parse("Headline\nwu: E-A.W1\nquestion: nope_\n");
    assert_eq!(got["parsed"]["wu"], serde_json::Value::Null);
    assert_eq!(got["parsed"]["question"], serde_json::Value::Null);
    assert_eq!(got["alert"]["lines"], serde_json::json!(["wu: E-A.W1", "question: nope_"]));
}

#[test]
fn parse_refuses_a_bad_or_repeated_header_naming_the_line() {
    for (text, line) in [
        ("wu: nope\nHead\n", "line 1"),
        ("wu: E-a.W1\nHead\n", "line 1"),
        ("wu: E-A.W1\nquestion: Bad_Id\nHead\n", "line 2"),
        ("question:\nHead\n", "line 1"),
        ("wu: E-A.W1\nwu: E-A.W2\nHead\n", "line 2"),
        ("question: q1\nwu: E-A.W1\nquestion: q2\nHead\n", "line 3"),
    ] {
        let got = parse(text);
        let error = got["error"].as_str().unwrap_or_else(|| panic!("{text:?} was not refused: {got}"));
        assert!(error.contains(line), "{text:?}: the refusal does not name {line}: {error}");
    }
    let empty = parse("wu: E-A.W1\n\n");
    assert!(empty["error"].as_str().is_some_and(|e| e.contains("empty")), "a header with no message must be refused as empty: {empty}");
}

#[test]
fn parse_gives_a_headline_only_body_its_default_line() {
    let got = parse("wu: E-A.W1\nMerged into the epic.\n");
    assert_eq!(got["alert"]["headline"], "Merged into the epic.");
    assert_eq!(got["alert"]["lines"], serde_json::json!(["See Telegram."]));
}

#[test]
fn parse_reads_crlf_and_blanks_control_characters() {
    let got = parse("wu: E-A.W1\r\nHead\u{7}line\r\nnext\tline\u{1b}[31m\r\n");
    assert_eq!(got["parsed"]["wu"], "E-A.W1", "a CRLF header was not read: {got}");
    assert_eq!(got["alert"]["headline"], "Head line");
    assert_eq!(got["alert"]["lines"], serde_json::json!(["next line [31m"]));
    let shown = got["alert"].to_string() + &got["question"].to_string();
    assert!(!shown.chars().any(|c| c.is_control()), "a control character reached a line: {shown}");
}

#[test]
fn parse_cuts_a_long_headline_and_line_with_an_ellipsis() {
    let got = parse(&format!("{}\n{}\n", "H".repeat(100), "L".repeat(300)));
    let headline = got["alert"]["headline"].as_str().unwrap();
    let line = got["alert"]["lines"][0].as_str().unwrap();
    assert_eq!(headline.chars().count(), 80, "{headline}");
    assert!(headline.ends_with('…') && headline.starts_with("HHH"), "{headline}");
    assert_eq!(line.chars().count(), 200, "{line}");
    assert!(line.ends_with('…'), "{line}");
}

#[test]
fn parse_shrinks_a_multibyte_body_to_its_byte_budgets() {
    let got = parse(&format!("question: q1\nwu: E-A.W1\n{}\n{}\n{}\n", "ệ".repeat(80), "ữ".repeat(200), "ở".repeat(200)));
    for (part, budget) in [("alert", 800), ("question", 480)] {
        let value = &got[part];
        let bytes = serde_json::to_string(value).unwrap().len();
        assert!(bytes <= budget, "the {part} value is {bytes} bytes, over {budget}");
        let headline = value["headline"].as_str().unwrap();
        assert!(!headline.is_empty() && headline.chars().count() <= 80 && headline.starts_with('ệ'), "{part}: {headline}");
        let lines = value["lines"].as_array().unwrap();
        assert!(!lines.is_empty() && lines.len() <= 2, "{part}: {lines:?}");
        for l in lines {
            let l = l.as_str().unwrap();
            assert!(!l.is_empty() && l.chars().count() <= 200, "{part}: {l}");
        }
    }
    assert_eq!(got["alert"]["link"], "keel://wu/E-A.W1");
}

// Review round of rule commit A (c47f692): a byte that is not UTF-8, a writer that hangs, and what
// the writer's environment holds.

#[test]
fn parse_blanks_a_byte_that_is_not_utf8() {
    let got = parse_bytes(b"question: q1\nwu: E-X.W1\nHead\xff line\nnext\xfe\n");
    assert!(got.get("error").is_none(), "a byte that is not UTF-8 was refused: {got}");
    assert_eq!(got["alert"]["headline"], "Head line");
    assert_eq!(got["alert"]["lines"], serde_json::json!(["next"]));
    assert_eq!(got["question"]["headline"], "Head line");
}

#[test]
fn a_byte_that_is_not_utf8_in_the_argument_still_previews_and_writes() {
    use std::os::unix::ffi::OsStrExt;
    let dir = started();
    let text = std::ffi::OsStr::from_bytes(b"wu: E-X.W1\nHead\xff line\n");
    // a strict UTF-8 locale: Python's stdout then refuses a lone surrogate unless the sender escapes it
    let strict = |args: &[&std::ffi::OsStr], base: &str| {
        sender(dir.path())
            .args(args)
            .env("KEEL_NOTIFY_ENV", dir.path().join(".env"))
            .env("KEEL_NOTIFY_ACCESS", dir.path().join("access.json"))
            .env("KEEL_NOTIFY_API_BASE", base)
            .env("LC_ALL", "en_US.UTF-8")
            // strict even where en_US.UTF-8 is not installed (Python would fall back to UTF-8 mode)
            .env("PYTHONIOENCODING", "utf-8:strict")
            .env_remove("PYTHONUTF8")
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let preview = strict(&[std::ffi::OsStr::new("--dry-run"), text], UNREACHED);
    assert!(preview.status.success(), "a non-UTF-8 byte broke the dry run: {}", stderr(&preview));
    assert!(stdout(&preview).contains(r#""headline":"Head line""#), "{}", stdout(&preview));
    let out = strict(&[std::ffi::OsStr::new("--send"), text], UNREACHED);
    assert_send_was_tried(&out);
    let alerts = alert_lines(&dir);
    assert_eq!(alerts.len(), 1, "no alert line for a non-UTF-8 byte: {}", stderr(&out));
    assert_eq!(alerts[0]["value"]["headline"], "Head line");
}

/// Every pid (or -group) a test recorded is killed when the test ends, pass or fail.
struct Reap(PathBuf);

impl Drop for Reap {
    fn drop(&mut self) {
        for pid in std::fs::read_to_string(&self.0).unwrap_or_default().split_whitespace() {
            let _ = common::cmd("kill").args(["-KILL", "--", pid]).stderr(Stdio::null()).status();
        }
    }
}

fn alive(pid: &str) -> bool {
    common::cmd("kill").args(["-0", pid]).stderr(Stdio::null()).status().unwrap().success()
}

/// Replace the temp repo's router-app.sh with `body` (a /bin/sh script).
fn fake_app(dir: &TempDir, body: &str) {
    std::fs::write(dir.path().join("scripts/router-app.sh"), body).unwrap();
}

#[test]
fn a_writer_that_hangs_is_killed_with_its_children_and_its_lock_released() {
    let dir = started();
    let pids = dir.path().join(".router/app/pids");
    let _reap = Reap(pids.clone());
    // holds the lock, starts a child, and waits; its TERM trap releases the lock as router-app.sh's does
    fake_app(
        &dir,
        r#"d=$(cd "$(dirname "$0")/.." && pwd -P)/.router/app
mkdir "$d/.lock" || exit 1
trap 'rmdir "$d/.lock"; exit 1' TERM
echo $$ > "$d/pids"
sleep 300 &
echo $! >> "$d/pids"
wait
"#,
    );
    let started_at = Instant::now();
    let out = run(&dir, &["--send", "wu: E-X.W2\nPlan to read\n"], UNREACHED);
    assert!(started_at.elapsed() < Duration::from_secs(20), "the sender waited {:?}", started_at.elapsed());
    assert_send_was_tried(&out);
    assert!(stderr(&out).contains("no desk alert") && stderr(&out).contains("did not answer"), "{}", stderr(&out));
    let recorded = std::fs::read_to_string(&pids).unwrap();
    assert_eq!(recorded.split_whitespace().count(), 2, "the fake did not record its pids: {recorded:?}");
    let deadline = Instant::now() + Duration::from_secs(3);
    while recorded.split_whitespace().any(alive) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let left: Vec<&str> = recorded.split_whitespace().filter(|p| alive(p)).collect();
    assert!(left.is_empty(), "the stalled writer's processes outlived the sender: {left:?}");
    assert!(!dir.path().join(".router/app/.lock").exists(), "the stalled writer's lock was left behind");
}

#[test]
fn the_writer_never_sees_the_credential_or_the_test_seam() {
    let dir = started();
    fake_app(
        &dir,
        r#"env > "$(cd "$(dirname "$0")/.." && pwd -P)/.router/app/env"
echo "OK run=fake n=2"
"#,
    );
    let out = run_with(&dir, &["--send", "wu: E-X.W2\nPlan to read\n"], UNREACHED, &[("TELEGRAM_BOT_TOKEN", TOKEN), ("TELEGRAM_STATE_DIR", "/x")]);
    assert_send_was_tried(&out);
    let env = std::fs::read_to_string(dir.path().join(".router/app/env")).unwrap();
    let leaked: Vec<&str> = env.lines().filter(|l| l.starts_with("TELEGRAM_") || l.starts_with("KEEL_NOTIFY_")).map(|l| l.split('=').next().unwrap()).collect();
    assert!(leaked.is_empty(), "router-app.sh was handed {leaked:?}");
    assert!(!env.contains(TOKEN), "router-app.sh was handed the credential");
}

// Re-review of f435eb1: the sender ended by TERM or HUP while a writer hangs, and a closed stdout.

/// A fake router-app.sh that takes the lock, starts a child and waits; its TERM trap releases the
/// lock as the real one's does. It records its pid and its child's in `<log dir>/pids`.
const HANGING_WRITER: &str = r#"d=$(cd "$(dirname "$0")/.." && pwd -P)/.router/app
mkdir "$d/.lock" || exit 1
trap 'rmdir "$d/.lock"; exit 1' TERM
sleep 300 &
echo "$$ $!" > "$d/pids.tmp" && mv "$d/pids.tmp" "$d/pids"
wait
"#;

fn ended_by(signal: &str) {
    use std::os::unix::process::CommandExt;
    let dir = started();
    let pids = dir.path().join(".router/app/pids");
    fake_app(&dir, HANGING_WRITER);
    // TERM and HUP reset to their defaults before the sender's exec, whatever the suite's launch
    // ignores (nohup, cargo test &): an ignored signal is inherited, and the sender keeps it ignored
    let mut command = common::cmd("python3");
    command
        .args(["-c", "import os, signal, sys\nfor s in (signal.SIGTERM, signal.SIGHUP): signal.signal(s, signal.SIG_DFL)\nos.execv(sys.argv[1], sys.argv[1:])"])
        .arg(dir.path().join("scripts/notify-owner.py"))
        .env("HOME", dir.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PATH", common::path_with(dir.path().join(".shim")))
        .args(["--send", "wu: E-X.W2\nPlan to read\n"])
        .env("KEEL_NOTIFY_ENV", dir.path().join(".env"))
        .env("KEEL_NOTIFY_ACCESS", dir.path().join("access.json"))
        .env("KEEL_NOTIFY_API_BASE", UNREACHED)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut sender = command.spawn().unwrap();
    let group = sender.id().to_string();
    // the sender's group is killed on every exit path, after the writer's recorded pids
    let _sender_reap = Reap(dir.path().join(".router/app/sender"));
    std::fs::write(dir.path().join(".router/app/sender"), format!("-{group}")).unwrap();
    let _reap = Reap(pids.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pids.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let recorded = std::fs::read_to_string(&pids).expect("the fake writer never started");
    assert_eq!(recorded.split_whitespace().count(), 2, "{recorded:?}");
    let sent = common::cmd("kill").args([&format!("-{signal}"), "--", &format!("-{group}")]).status().unwrap();
    assert!(sent.success(), "kill -{signal} of the sender's group failed");
    let deadline = Instant::now() + Duration::from_secs(10);
    while sender.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(sender.try_wait().unwrap().is_some(), "the sender outlived {signal}");
    let deadline = Instant::now() + Duration::from_secs(3);
    while recorded.split_whitespace().any(alive) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let left: Vec<&str> = recorded.split_whitespace().filter(|p| alive(p)).collect();
    assert!(left.is_empty(), "{signal} to the sender left its writer's processes: {left:?}");
    assert!(!dir.path().join(".router/app/.lock").exists(), "{signal} to the sender left the writer's lock");
}

#[test]
fn a_sender_ended_by_term_ends_its_hung_writer_and_frees_the_lock() {
    ended_by("TERM");
}

#[test]
fn a_sender_ended_by_hup_ends_its_hung_writer_and_frees_the_lock() {
    ended_by("HUP");
}

#[test]
fn a_dry_run_with_stdout_closed_still_succeeds() {
    let dir = started();
    let script = dir.path().join("scripts/notify-owner.py");
    let out = common::cmd("sh")
        .args(["-c", r#"exec "$0" "$@" >&-"#])
        .arg(&script)
        .args(["--dry-run", "wu: E-X.W2\nPlan to read\n"])
        .env("HOME", dir.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("KEEL_NOTIFY_ENV", dir.path().join(".env"))
        .env("KEEL_NOTIFY_ACCESS", dir.path().join("access.json"))
        .env("KEEL_NOTIFY_API_BASE", UNREACHED)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "a dry run with stdout closed failed: {}", stderr(&out));
    assert_no_credential(&out);
}

// Review round 1 of B and C, M1: a leading line that is a header with a mistyped key (its case, a
// space, a one-letter slip in "question") is refused like a bad value, never sent as body text.

#[test]
fn parse_refuses_a_mistyped_header_key_naming_the_line() {
    for (text, line) in [
        ("Wu: E-A.W1\nHead\n", "line 1"),
        ("WU: E-A.W1\nHead\n", "line 1"),
        ("wu : E-A.W1\nHead\n", "line 1"),
        (" wu: E-A.W1\nHead\n", "line 1"),
        ("Question: q1\nHead\n", "line 1"),
        ("qestion: q1\nHead\n", "line 1"),
        ("wu: E-A.W1\nquestoin: q1\nHead\n", "line 2"),
        ("wu: E-A.W1\nquestion : q1\nHead\n", "line 2"),
    ] {
        let got = parse(text);
        let error = got["error"].as_str().unwrap_or_else(|| panic!("{text:?} was read as body text: {got}"));
        assert!(error.contains(line), "{text:?}: the refusal does not name {line}: {error}");
    }
    // an ordinary headline with a colon is body text, not a header
    for text in ["Plan to read: E-A.W1\nOpen it\n", "Sign-off ready: E-A.W1\n", "Merged: E-A.W1 into the epic\n", "Note: the log is wiped\n"] {
        let got = parse(text);
        assert!(got.get("error").is_none(), "{text:?} was refused: {got}");
        assert_eq!(got["parsed"]["wu"], serde_json::Value::Null, "{text:?}");
    }
}

#[test]
fn a_mistyped_header_key_sends_nothing_and_writes_nothing() {
    let dir = started();
    let before = std::fs::read(log_path(&dir)).unwrap();
    let body = write_body(&dir, "Wu: E-X.W2\nPlan to read: E-X.W2\n");
    let out = run(&dir, &["--send"], UNREACHED);
    assert!(!out.status.success(), "a mistyped header key was sent");
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("line 1"), "{problem}");
    assert!(!problem.contains("Telegram API"), "a mistyped header key reached the send: {problem}");
    assert!(stdout(&out).is_empty());
    assert_eq!(std::fs::read(log_path(&dir)).unwrap(), before, "a mistyped header key wrote to the log");
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "Wu: E-X.W2\nPlan to read: E-X.W2\n");
    assert_no_credential(&out);
}

// Review round 2 of B to E: the mistyped-key refusal holds only when what follows the colon is a
// header's value, so a headline that starts with "Question" stays text; the refusal quotes what
// was typed.

#[test]
fn a_headline_that_starts_with_question_stays_text() {
    for (text, headline) in [
        ("Question: keep keel:// as the link?\nReply yes or no\n", "Question: keep keel:// as the link?"),
        ("Questions: two for you\nOne: the link\n", "Questions: two for you"),
        ("wu: E-A.W1\nquestion: q1\nQuestion: keep keel://?\nReply yes or no\n", "Question: keep keel://?"),
    ] {
        let got = parse(text);
        assert!(got.get("error").is_none(), "{text:?} was refused: {got}");
        assert_eq!(got["alert"]["headline"], headline, "{text:?}");
    }
}

#[test]
fn a_mistyped_header_refusal_quotes_what_was_typed() {
    for (text, typed) in
        [(" wu: E-A.W1\nHead\n", "' wu:'"), ("wu : E-A.W1\nHead\n", "'wu :'"), ("\twu: E-A.W1\nHead\n", r"'\twu:'"), ("Question: q1\nHead\n", "'Question:'")]
    {
        let got = parse(text);
        let error = got["error"].as_str().unwrap_or_else(|| panic!("{text:?} was not refused: {got}"));
        assert!(error.contains(typed), "{text:?}: the refusal does not quote {typed}: {error}");
    }
}
