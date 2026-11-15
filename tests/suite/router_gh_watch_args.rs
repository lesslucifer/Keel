//! E-GHFLOW.W3, unit layer: scripts/router-gh-watch.sh's argument check, alone (an empty home and a
//! curl that records any call: a refused argument is exit 2 naming its reason, an accepted one goes
//! on to router-gh.sh, which stops at the missing App key, and nothing reaches curl), and the closed
//! command grammar of scripts/router-gh-command.jq, through jq, against a reference written here.
compiled!();

use std::io::Write as _;
use std::path::Path;

use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;

fn scripts() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts")
}

/// Exit code and stderr of the watch on `args`; panics if curl was ever called or anything reached
/// stdout. The guard's agreement test in router.rs reads the watch's verdict through this too.
pub(crate) fn check(args: &[&str]) -> (Option<i32>, String) {
    let d = TempDir::new().unwrap();
    let curl = d.path().join("curl");
    std::fs::write(&curl, format!("#!/bin/sh\ntouch {}\n", common::sh_quote(&d.path().join("called").to_string_lossy()))).unwrap();
    std::fs::set_permissions(&curl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let o = common::cmd("sh")
        .arg(scripts().join("router-gh-watch.sh"))
        .args(args)
        .env("HOME", d.path())
        .env("ROUTER_GH_CURL", &curl)
        .env("ROUTER_WATCH_MAX", "0")
        .output()
        .unwrap();
    assert!(!d.path().join("called").exists(), "{args:?} reached curl");
    assert!(o.stdout.is_empty(), "{args:?} printed {}", String::from_utf8_lossy(&o.stdout));
    (o.status.code(), String::from_utf8_lossy(&o.stderr).into_owned())
}

/// Whether the watch accepts `args`: past its check, router-gh.sh stops at the missing App key.
pub(crate) fn accepts(args: &[&str]) -> bool {
    let (code, err) = check(args);
    assert_eq!(code, Some(2), "{args:?}: {err}");
    err.starts_with("router-gh: no App key at ")
}

fn refused(args: &[&str], why: &str) {
    let (code, err) = check(args);
    assert_eq!(code, Some(2), "{args:?}: {err}");
    assert!(err.starts_with("router-gh-watch: ") && err.contains(why), "{args:?}: wanted {why:?} in {err:?}");
    assert_eq!(err.lines().count(), 1, "{args:?}: one line, got {err:?}");
}

#[test]
fn kinds_and_word_counts_are_fixed() {
    for ok in [
        &["review", "31"][..],
        &["review", "31", "since", "4242"],
        &["comment", "1"],
        &["comment", "9999999", "since", "999999999999999"],
        &["merged", "12"],
        &["checks", "12"],
    ] {
        assert!(accepts(ok), "{ok:?}");
    }
    let usage = "usage: review <pr> [since <id>]";
    for bad in [
        &[][..],
        &["frob", "1"],
        &["REVIEW", "1"],
        &["watch-read", "comment", "1"],
        &["review"],
        &["review", "1", "since"],
        &["review", "1", "after", "2"],
        &["review", "1", "since", "2", "x"],
        &["merged", "1", "since", "2"],
        &["checks", "1", "since", "2"],
        &["merged", "1", "2"],
    ] {
        refused(bad, usage);
    }
}

#[test]
fn numbers_are_plain_and_bounded() {
    for n in ["0", "012", "12345678", "1a", "", " 1", "1.0", "+1", "¹"] {
        refused(&["comment", n], "must be a number");
    }
    for id in ["0", "01", "1234567890123456", "x", "1e3"] {
        refused(&["comment", "1", "since", id], "since must be a comment or review id");
    }
    for a in [&["-h"][..], &["review", "-1"], &["comment", "1", "since", "-2"], &["comment", "--since", "1"]] {
        let (code, err) = check(a);
        assert_eq!(code, Some(2), "{a:?}: {err}");
        assert!(err.contains("may not start with -") || err.contains("usage:"), "{a:?}: {err}");
    }
    for (k, v) in [("ROUTER_WATCH_POLL", "0"), ("ROUTER_WATCH_POLL", "1x"), ("ROUTER_WATCH_MAX", "-1"), ("ROUTER_WATCH_MAX", "1234567890")] {
        let o = common::cmd("sh").arg(scripts().join("router-gh-watch.sh")).args(["comment", "1"]).env(k, v).output().unwrap();
        assert_eq!(o.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&o.stderr).contains(k), "{k}={v}");
    }
}

/// The grammar's verdicts on `bodies`, in one jq run of the module.
fn parse(bodies: &[String]) -> Vec<Value> {
    let mut c = common::cmd("jq")
        .args(["-c", "-L"])
        .arg(scripts())
        .arg("include \"router-gh-command\"; .[] | keel_command")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(serde_json::to_string(bodies).unwrap().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// The grammar, written again from its spec, as the reference the generated corpus is held to.
fn reference(body: &str) -> Value {
    let body = body.replace("\r\n", "\n");
    let lines: Vec<&str> = body.split('\n').collect();
    let first = lines[0].trim_matches(|c| c == ' ' || c == '\t');
    let rest: Vec<&str> = lines[1..].to_vec();
    if !first.to_ascii_lowercase().starts_with("/keel") {
        return json!({"kind": "text", "rest": lines});
    }
    let w: Vec<&str> = first.split([' ', '\t']).filter(|w| !w.is_empty()).collect();
    let bad = |why: &str| json!({"kind": "bad", "why": why, "rest": rest});
    if !w[0].eq_ignore_ascii_case("/keel") {
        return bad("not-keel");
    }
    let verb = w.get(1).map(|v| v.to_ascii_lowercase()).unwrap_or_default();
    match verb.as_str() {
        "" => bad("no-verb"),
        "go" | "hold" | "retry" if w.len() == 2 => json!({"kind": "command", "verb": verb, "rest": rest}),
        "go" | "hold" | "retry" => bad("extra-words"),
        "answer" if w.len() < 4 => bad("missing-words"),
        "answer" if w.len() > 4 => bad("extra-words"),
        "answer" => {
            let n = w[2];
            if !(1..=3).contains(&n.len()) || !n.bytes().all(|b| b.is_ascii_digit()) || n.starts_with('0') {
                bad("bad-number")
            } else if !["A", "B"].contains(&w[3].to_ascii_uppercase().as_str()) {
                bad("bad-choice")
            } else {
                json!({"kind": "command", "verb": "answer", "n": n.parse::<u64>().unwrap(), "choice": w[3].to_ascii_uppercase(), "rest": rest})
            }
        }
        _ => bad("unknown-verb"),
    }
}

#[test]
fn the_grammar_names_each_command_and_each_bad_form() {
    let cases: Vec<(&str, Value)> = vec![
        ("/keel go", json!({"kind": "command", "verb": "go", "rest": []})),
        ("/keel hold\nwait", json!({"kind": "command", "verb": "hold", "rest": ["wait"]})),
        ("  /KEEL\tRetry  \r\nagain\r\n", json!({"kind": "command", "verb": "retry", "rest": ["again", ""]})),
        ("/keel answer 2 a", json!({"kind": "command", "verb": "answer", "n": 2, "choice": "A", "rest": []})),
        ("/keel  answer  999  B", json!({"kind": "command", "verb": "answer", "n": 999, "choice": "B", "rest": []})),
        ("/keel", json!({"kind": "bad", "why": "no-verb", "rest": []})),
        ("/keelgo", json!({"kind": "bad", "why": "not-keel", "rest": []})),
        ("/keel go now", json!({"kind": "bad", "why": "extra-words", "rest": []})),
        ("/keel answer 2", json!({"kind": "bad", "why": "missing-words", "rest": []})),
        ("/keel answer 2 A B", json!({"kind": "bad", "why": "extra-words", "rest": []})),
        ("/keel answer 0 A", json!({"kind": "bad", "why": "bad-number", "rest": []})),
        ("/keel answer 02 A", json!({"kind": "bad", "why": "bad-number", "rest": []})),
        ("/keel answer 1000 A", json!({"kind": "bad", "why": "bad-number", "rest": []})),
        ("/keel answer x A", json!({"kind": "bad", "why": "bad-number", "rest": []})),
        ("/keel answer 1 C", json!({"kind": "bad", "why": "bad-choice", "rest": []})),
        ("/keel answer 1 AB", json!({"kind": "bad", "why": "bad-choice", "rest": []})),
        ("/keel merge", json!({"kind": "bad", "why": "unknown-verb", "rest": []})),
        ("/keel tried pass", json!({"kind": "bad", "why": "unknown-verb", "rest": []})),
        ("/keel\u{a0}go", json!({"kind": "bad", "why": "not-keel", "rest": []})),
        ("please /keel go", json!({"kind": "text", "rest": ["please /keel go"]})),
        ("> /keel go\nquoted", json!({"kind": "text", "rest": ["> /keel go", "quoted"]})),
        ("", json!({"kind": "text", "rest": [""]})),
        ("\n/keel go", json!({"kind": "text", "rest": ["", "/keel go"]})),
    ];
    let got = parse(&cases.iter().map(|(b, _)| b.to_string()).collect::<Vec<_>>());
    for ((body, want), got) in cases.iter().zip(got) {
        assert_eq!(&got, want, "{body:?}");
        assert_eq!(&reference(body), want, "the reference disagrees on {body:?}");
    }
}

/// Invariant, over a generated corpus of near-miss first lines: the module agrees with the reference,
/// and a first line that does not start with /keel is never a command, its every line the message.
#[test]
fn the_grammar_agrees_with_its_reference_on_generated_comments() {
    let mut seed: u64 = 0x6b65_656c;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    let words = [
        "/keel",
        "/KEEL",
        "/Keel",
        "/keelx",
        "keel",
        "/kee",
        "go",
        "GO",
        "Go",
        "hold",
        "retry",
        "answer",
        "Answer",
        "tried",
        "waive-review",
        "1",
        "2",
        "02",
        "999",
        "1000",
        "0",
        "a",
        "A",
        "b",
        "B",
        "c",
        "AB",
        "x",
        "\u{e9}",
        "/",
        "\u{212a}eel",
    ];
    let seps = [" ", "  ", "\t", " \t "];
    let mut bodies: Vec<String> = Vec::new();
    for _ in 0..400 {
        let mut b = String::new();
        if next(4) == 0 {
            b.push_str(seps[next(seps.len())]);
        }
        // mostly near-misses of a command: a keyword, a verb, then arguments; else any words
        let shaped = next(3) != 0;
        let lead = ["/keel", "/KEEL", "/Keel", "/keelx", "keel"];
        let verbs = ["go", "GO", "hold", "retry", "answer", "Answer", "tried", "x"];
        let argw = ["1", "2", "02", "999", "1000", "0", "a", "A", "b", "B", "c", "AB"];
        for i in 0..if shaped { 2 + next(4) } else { next(6) } {
            if i > 0 {
                b.push_str(seps[next(seps.len())]);
            }
            b.push_str(match (shaped, i) {
                (true, 0) => lead[next(lead.len())],
                (true, 1) => verbs[next(verbs.len())],
                (true, _) => argw[next(argw.len())],
                _ => words[next(words.len())],
            });
        }
        if next(3) == 0 {
            b.push_str(seps[next(seps.len())]);
        }
        for _ in 0..next(3) {
            b.push_str(if next(2) == 0 { "\n" } else { "\r\n" });
            b.push_str(words[next(words.len())]);
        }
        bodies.push(b);
    }
    bodies.sort();
    bodies.dedup();
    let got = parse(&bodies);
    let mut commands = 0;
    for (b, g) in bodies.iter().zip(&got) {
        assert_eq!(g, &reference(b), "{b:?}");
        let first = b.split('\n').next().unwrap().trim_matches(|c| c == ' ' || c == '\t' || c == '\r');
        if !first.to_ascii_lowercase().starts_with("/keel") {
            assert_eq!(g["kind"], "text", "{b:?}");
            assert_eq!(g["rest"].as_array().unwrap().len(), b.replace("\r\n", "\n").split('\n').count(), "{b:?}");
        }
        commands += usize::from(g["kind"] == "command");
    }
    assert!(commands >= 20, "the corpus must hold commands too ({commands})");
}
