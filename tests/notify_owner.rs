//! E-RGUARD.W2: the router's owner-notify sender, spawned as the router runs it.
//! The script posts one message to the paired Telegram bot and prints its id; these tests
//! pin its three modes against a local mock API, its failure line, the no-redirection rule,
//! the chat it actually addresses, and that neither the credential nor the chat id reaches
//! stdout or stderr.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempfile::TempDir;

mod common;

/// A credential-shaped fake. The sender must never print it; a test failure never formats it.
const TOKEN: &str = "123456789:AAHfakeTOKENfakeTOKENfakeTOKENfake00";
const CHAT: &str = "4242";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn script() -> PathBuf {
    root().join("scripts/notify-owner.py")
}

/// A temp home holding the channel state the sender reads: a token file and the paired chat.
fn channel() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env"),
        format!("TELEGRAM_BOT_TOKEN={TOKEN}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("access.json"),
        format!(r#"{{"dmPolicy":"pairing","allowFrom":["{CHAT}"],"groups":{{}},"pending":{{}}}}"#),
    )
    .unwrap();
    dir
}

/// Run the sender as the router does: the file itself, no interpreter named on the command line.
/// The KEEL_NOTIFY_* variables are the only redirection, and only this test sets them.
fn run_with(dir: &TempDir, args: &[&str], api_base: &str, extra: &[(&str, &str)]) -> Output {
    let mut command = common::cmd(script());
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
    let body = request
        .split("\r\n\r\n")
        .nth(1)
        .expect("the request has no body");
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
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
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
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if buf.len() >= head + 4 + length {
                    break;
                }
            }
        }
        tx.send(String::from_utf8_lossy(&buf).to_string()).unwrap();
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
fn dry_run_prints_the_request_without_the_chat_or_a_send() {
    let dir = channel();
    let base = closed_base();
    let out = run(
        &dir,
        &[
            "--dry-run",
            "Owner action\ncd /keel && keel mr confirm wu abc --as vuluu",
        ],
        &base,
    );
    assert!(out.status.success(), "dry run failed: {}", stderr(&out));
    let shown = stdout(&out);
    assert!(
        shown.contains("dry-run:") && shown.contains("no request sent"),
        "{shown}"
    );
    assert!(
        shown.contains("cd /keel && keel mr confirm wu abc --as vuluu"),
        "{shown}"
    );
    assert!(!shown.contains(CHAT), "the chat id is in the dry-run line");
    assert_no_credential(&out);
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));
}

#[test]
fn check_names_the_bot_and_sends_no_message() {
    let dir = channel();
    let (base, request, server) = mock(
        "200 OK",
        r#"{"ok":true,"result":{"id":42,"is_bot":true,"first_name":"Keel Test","username":"keel_test_bot"}}"#,
    );
    let out = run(&dir, &["--check"], &base);
    let seen = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "check failed: {}", stderr(&out));
    let shown = stdout(&out);
    assert!(shown.contains("@keel_test_bot"), "{shown}");
    assert!(
        seen.contains("/getMe") && !seen.contains("/sendMessage"),
        "check called the wrong method"
    );
    assert_no_credential(&out);
}

#[test]
fn check_failure_is_one_line_and_never_prints_the_credential() {
    let dir = channel();
    let (base, request, server) = mock(
        "401 Unauthorized",
        r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#,
    );
    let out = run(&dir, &["--check"], &base);
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(!out.status.success(), "a 401 check must fail");
    let shown = stdout(&out);
    let problem = stderr(&out);
    assert!(shown.is_empty(), "a failure writes stdout: {shown}");
    assert_eq!(
        problem.lines().count(),
        1,
        "a failure is one line: {problem:?}"
    );
    assert!(problem.contains("401"), "{problem}");
    assert_no_credential(&out);
}

#[test]
fn send_prints_the_id_and_never_the_credential() {
    let dir = channel();
    let (base, request, server) = mock("200 OK", r#"{"ok":true,"result":{"message_id":1731}}"#);
    let out = run(&dir, &["--send", "hello owner"], &base);
    let seen = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "send failed: {}", stderr(&out));
    assert_eq!(stdout(&out), "id 1731\n");
    let payload = sent_payload(&seen);
    assert_eq!(
        payload["chat_id"].as_str(),
        Some(CHAT),
        "the wrong chat was addressed"
    );
    assert_eq!(payload["text"].as_str(), Some("hello owner"));
    assert_no_credential(&out);
}

#[test]
fn bare_send_reads_and_clears_the_body_so_it_cannot_resend_a_stale_message() {
    let dir = channel();
    let body = dir.path().join(".router/notify/owner.md");
    std::fs::create_dir_all(body.parent().unwrap()).unwrap();
    std::fs::write(
        &body,
        "Sign-off ready\ncd /keel && keel mr confirm wu 1a2b3c --as vuluu\n",
    )
    .unwrap();
    let (base, request, server) = mock("200 OK", r#"{"ok":true,"result":{"message_id":7}}"#);
    let out = run(&dir, &["--send"], &base);
    let seen = request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "send failed: {}", stderr(&out));
    assert_eq!(stdout(&out), "id 7\n");
    let payload = sent_payload(&seen);
    assert_eq!(
        payload["chat_id"].as_str(),
        Some(CHAT),
        "the wrong chat was addressed"
    );
    assert!(
        payload["text"]
            .as_str()
            .is_some_and(|t| t.contains("Sign-off ready") && t.contains("keel mr confirm")),
        "send carried the wrong body"
    );
    assert_no_credential(&out);
    assert!(
        std::fs::read_to_string(&body).unwrap().is_empty(),
        "the sent body was left in place and a later bare --send would resend it"
    );
    let again = run(&dir, &["--send"], &closed_base());
    assert!(!again.status.success(), "a cleared body must not send");
    let problem = stderr(&again);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(
        problem.contains("empty"),
        "a cleared body must fail as empty: {problem}"
    );
    assert_no_credential(&again);
}

#[test]
fn preview_and_failed_send_leave_the_body_in_place() {
    let dir = channel();
    let body = dir.path().join(".router/notify/owner.md");
    std::fs::create_dir_all(body.parent().unwrap()).unwrap();
    std::fs::write(&body, "Keep me\n").unwrap();
    let preview = run(&dir, &["--dry-run"], &closed_base());
    assert!(
        preview.status.success(),
        "a bare dry run failed: {}",
        stderr(&preview)
    );
    assert_eq!(
        std::fs::read_to_string(&body).unwrap(),
        "Keep me\n",
        "a preview truncated the body"
    );
    let (base, request, server) = mock(
        "500 Internal Server Error",
        r#"{"ok":false,"error_code":500,"description":"boom"}"#,
    );
    let failed = run(&dir, &["--send"], &base);
    request.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    assert!(!failed.status.success(), "a 500 send must fail");
    assert_eq!(
        std::fs::read_to_string(&body).unwrap(),
        "Keep me\n",
        "a failed send truncated the body"
    );
    assert_no_credential(&preview);
    assert_no_credential(&failed);
}

#[test]
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
    assert!(
        problem.contains("owner.md")
            && problem.contains("not writable")
            && problem.contains("not sent"),
        "{problem}"
    );
    assert_eq!(
        std::fs::read_to_string(&body).unwrap(),
        "Owner action\n",
        "the body was changed"
    );
    std::fs::set_permissions(&body, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_no_credential(&out);
}

#[test]
fn a_missing_credential_is_one_line_naming_the_file_only() {
    let dir = channel();
    let missing = dir.path().join("absent.env");
    let out = common::cmd(script())
        .arg("--check")
        .env("KEEL_NOTIFY_ENV", &missing)
        .env("KEEL_NOTIFY_ACCESS", dir.path().join("access.json"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "a missing credential must fail");
    let problem = stderr(&out);
    assert_eq!(
        problem.lines().count(),
        1,
        "a failure is one line: {problem:?}"
    );
    assert!(problem.contains("absent.env"), "{problem}");
    assert_no_credential(&out);
}

#[test]
fn bare_send_without_a_body_is_one_line_and_sends_nothing() {
    let dir = channel();
    let out = run(&dir, &["--send"], &closed_base());
    assert!(!out.status.success(), "a missing body must fail");
    assert!(stdout(&out).is_empty(), "a failure writes stdout");
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(
        problem.contains("owner.md"),
        "the failure names the body file: {problem}"
    );
    assert_no_credential(&out);
}

#[test]
fn cleartext_http_is_refused_for_a_non_loopback_host() {
    let dir = channel();
    let out = run(&dir, &["--check"], "http://evil.invalid");
    assert!(
        !out.status.success(),
        "cleartext http to another host must be refused"
    );
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("https"), "{problem}");
    assert_no_credential(&out);
}

#[test]
fn a_malformed_token_never_echoes_the_request_target() {
    let dir = channel();
    std::fs::write(
        dir.path().join(".env"),
        "TELEGRAM_BOT_TOKEN=bad token:secret\n",
    )
    .unwrap();
    let out = run(&dir, &["--check"], &closed_base());
    assert!(!out.status.success(), "a malformed token must fail");
    let shown = stdout(&out) + &stderr(&out);
    assert_eq!(shown.lines().count(), 1, "{shown:?}");
    assert!(
        !shown.contains("bad token") && !shown.contains("secret"),
        "{shown}"
    );
    assert_no_credential(&out);
}

#[test]
fn a_stalled_server_times_out_in_one_line() {
    let dir = channel();
    let base = stalled_server();
    let started = Instant::now();
    let out = run_with(&dir, &["--check"], &base, &[("KEEL_NOTIFY_TIMEOUT", "0.2")]);
    assert!(!out.status.success(), "a stalled server must fail");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the timeout was not injectable: {:?}",
        started.elapsed()
    );
    let problem = stderr(&out);
    assert_eq!(problem.lines().count(), 1, "{problem:?}");
    assert!(problem.contains("Telegram API"), "{problem}");
    assert_no_credential(&out);
}
