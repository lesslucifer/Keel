//! E-RGUARD.W2: the router's owner-notify sender, spawned as the router runs it.
//! The script posts one message to the paired Telegram bot and prints its id; these tests
//! pin its three modes against a local mock API, its failure line, and that the channel
//! credential never reaches stdout or stderr.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

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

fn run(dir: &TempDir, args: &[&str], api_base: &str) -> Output {
    common::cmd("python3")
        .arg(script())
        .args(args)
        .env("KEEL_NOTIFY_ENV", dir.path().join(".env"))
        .env("KEEL_NOTIFY_ACCESS", dir.path().join("access.json"))
        .env("KEEL_NOTIFY_API_BASE", api_base)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
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

#[test]
fn dry_run_prints_the_request_and_sends_nothing() {
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
    assert!(shown.contains("dry-run:"), "{shown}");
    assert!(shown.contains(CHAT), "{shown}");
    assert!(
        shown.contains("cd /keel && keel mr confirm wu abc --as vuluu"),
        "{shown}"
    );
    assert!(
        !shown.contains(TOKEN) && !stderr(&out).contains(TOKEN),
        "the credential reached the output"
    );
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
    assert!(
        !shown.contains(TOKEN) && !stderr(&out).contains(TOKEN),
        "the credential reached the output"
    );
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
    assert!(
        !shown.contains(TOKEN) && !problem.contains(TOKEN),
        "the credential reached the output"
    );
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
    assert!(
        seen.contains("/sendMessage"),
        "send called the wrong method"
    );
    assert!(
        seen.contains("hello owner") && seen.contains(CHAT),
        "send carried the wrong payload"
    );
    assert!(
        !stdout(&out).contains(TOKEN) && !stderr(&out).contains(TOKEN),
        "the credential reached the output"
    );
}

#[test]
fn bare_send_reads_the_body_the_router_wrote() {
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
    assert!(
        seen.contains("Sign-off ready") && seen.contains("keel mr confirm"),
        "send carried the wrong body"
    );
}

#[test]
fn a_missing_credential_is_one_line_naming_the_file_only() {
    let dir = channel();
    let missing = dir.path().join("absent.env");
    let out = common::cmd("python3")
        .arg(script())
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
    assert!(
        !stdout(&out).contains(TOKEN) && !problem.contains(TOKEN),
        "the credential reached the output"
    );
}
