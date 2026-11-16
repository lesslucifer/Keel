//! E-GHFLOW.W2, unit layer: `scripts/router-gh.sh`'s argument check, alone. Each case runs this
//! checkout's script with an empty home and a curl that records any call: a refused argument is exit
//! 2 naming its reason, an accepted one goes on to the missing App key, and nothing reaches curl.
compiled!();

use std::path::Path;

use crate::common;

/// Exit code and stderr of the script on `args`; panics if curl was ever called. The guard's
/// agreement test in router.rs reads the script's verdict through this too.
pub(crate) fn check(args: &[&str]) -> (Option<i32>, String) {
    isolated("router-gh.sh", args, &[])
}

/// `scripts/<script>` on `args` in the argument checks' one harness: an empty temp HOME (no App
/// key), a curl that only records a call, `env` over those, and a 60 s alarm, so a regression past
/// the argument check reaches neither the owner's key nor GitHub and never hangs. Panics if curl
/// was called or anything reached stdout.
pub(crate) fn isolated(script: &str, args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String) {
    // a HOME the script accepts under any TMPDIR (backlog G5, W2), and one curl stub per run
    let d = crate::router_gh::plain_tempdir();
    let curl = common::template("router-gh args curl", |t| crate::router_gh::exe(&t.join("curl"), "#!/bin/sh\ntouch \"$CURL_CALLED\"\n")).join("curl");
    let o = common::cmd("perl")
        .args(["-e", "alarm 60; exec @ARGV", "sh"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts").join(script))
        .args(args)
        .env("HOME", d.path())
        .env("ROUTER_GH_CURL", &curl)
        .env("CURL_CALLED", d.path().join("called"))
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(!d.path().join("called").exists(), "{args:?} reached curl");
    assert!(o.stdout.is_empty(), "{args:?} printed {}", String::from_utf8_lossy(&o.stdout));
    (o.status.code(), String::from_utf8_lossy(&o.stderr).into_owned())
}

fn refused(args: &[&str], why: &str) {
    let (code, err) = check(args);
    assert_eq!(code, Some(2), "{args:?}: {err}");
    assert!(err.contains(why), "{args:?}: wanted {why:?} in {err:?}");
    assert!(!err.contains("no App key"), "{args:?} passed the argument check: {err}");
    assert_eq!(err.lines().count(), 1, "{args:?}: one line, got {err:?}");
}

fn accepted(args: &[&str]) {
    let (code, err) = check(args);
    assert_eq!(code, Some(2), "{args:?}: {err}");
    assert!(err.starts_with("router-gh: no App key at "), "{args:?} was refused: {err}");
}

#[test]
fn modes_and_word_counts_are_fixed() {
    refused(&[], "usage: push <branch>");
    for m in ["frob", "merge", "approve", "--help", "-h", "PUSH"] {
        refused(&[m], "usage: push <branch>");
    }
    refused(&["push"], "usage: push <branch>");
    refused(&["push", "wu/E/x", "wu/E/y"], "usage: push <branch>");
    refused(&["pr-open"], "usage: pr-open <branch>");
    refused(&["pr-open", "wu/E/x", "--base"], "usage: pr-open <branch>");
    refused(&["pr-open", "wu/E/x", "--base", "main", "x"], "usage: pr-open <branch>");
    refused(&["pr-open", "wu/E/x", "--head", "main"], "usage: pr-open <branch>");
    refused(&["pr-ready"], "usage: pr-ready <n>");
    refused(&["pr-ready", "1", "2"], "usage: pr-ready <n>");
    refused(&["post", "1"], "usage: post <n> <body-name>");
    refused(&["post", "1", "a", "b"], "usage: post <n> <body-name>");
    refused(&["sync"], "usage: sync <epic/E>");
    refused(&["sync", "epic/E", "x"], "usage: sync <epic/E>");
    refused(&["credential", "/x"], "usage: credential <file> <op>");
}

#[test]
fn a_branch_is_wu_e_slug_or_epic_e_in_plain_words() {
    for b in ["wu/E/x", "wu/E-GHFLOW/w2-router-bot", "epic/E-GHFLOW", "epic/E.2", "wu/E/x_1"] {
        accepted(&["push", b]);
        accepted(&["pr-open", b]);
    }
    for b in ["-x", "--force", "-fwu/E/x"] {
        refused(&["push", b], "may not start with -");
    }
    for b in [
        "main",
        "v1.0",
        "refs/heads/wu/E/x",
        "refs/tags/v1",
        "wu/E",
        "wu/E/x/y",
        "epic/E/x",
        "epic/",
        "wu//x",
        "wu/E/-x",
        "wu/-E/x",
        "wu/E/.x",
        "wu/E/x y",
        "wu/E/x\n",
        "wu/E/x@{1}",
        "epic/E~1",
        "epic/E^",
        "epic/E:main",
        "+wu/E/x",
        "wu/E/x:main",
        "WU/E/x",
        "epic/É",
    ] {
        refused(&["push", b], "branch must be epic/<E> or wu/<E>/<slug>");
    }
    for b in ["wu/E/x..y", "wu/E/x.lock", "wu/E/x.", "epic/E.lock"] {
        refused(&["push", b], "not a valid branch name");
    }
}

#[test]
fn pr_open_takes_main_or_an_epic_as_its_base() {
    accepted(&["pr-open", "wu/E/x", "--base", "main"]);
    accepted(&["pr-open", "wu/E/x", "--base", "epic/F"]);
    accepted(&["pr-open", "epic/E", "--base", "main"]);
    refused(&["pr-open", "wu/E/x", "--base=main"], "usage: pr-open <branch>");
    refused(&["pr-open", "wu/E/x", "--base", "-main"], "may not start with -");
    for b in ["wu/E/y", "master", "refs/heads/main", "epic/E/x", "v1"] {
        refused(&["pr-open", "wu/E/x", "--base", b], "base must be main or epic/<E>");
    }
    refused(&["pr-open", "wu/E/x", "--base", "epic/E.lock"], "not a valid branch name");
    refused(&["pr-open", "epic/E", "--base", "epic/E"], "cannot be its own base");
}

#[test]
fn a_pull_request_is_a_number_and_a_body_a_plain_name() {
    for n in ["1", "12", "9999999"] {
        accepted(&["pr-ready", n]);
        accepted(&["post", n, "review"]);
    }
    for n in ["0", "012", "12345678", "1a", "#1", "1.0", " 1", ""] {
        refused(&["pr-ready", n], "pull request must be a number");
        refused(&["post", n, "review"], "pull request must be a number");
    }
    refused(&["pr-ready", "-1"], "may not start with -");
    for name in ["a", "round_2-status", "A9", &"a".repeat(64)] {
        accepted(&["post", "7", name]);
    }
    for name in ["a.md", "../a", "a/b", "_a", "a b", "a\n", "", &"a".repeat(65), "é"] {
        refused(&["post", "7", name], "body name must be plain words");
    }
    refused(&["post", "7", "-a"], "may not start with -");
}

#[test]
fn sync_takes_an_epic_branch_only() {
    accepted(&["sync", "epic/E-GHFLOW"]);
    for b in ["main", "wu/E/x", "epic/E/x", "epic"] {
        refused(&["sync", b], "sync takes an epic/<E> branch only");
    }
    refused(&["sync", "-epic/E"], "may not start with -");
    refused(&["sync", "epic/E.lock"], "not a valid branch name");
}

/// Invariant: for every mode, any argument starting with `-` in any position is refused before
/// anything else runs (seeded generator over the valid argument lists): the mode word and the
/// `--base` word as a usage error, every other word for its dash (backlog G5, W2).
#[test]
fn every_argument_starting_with_a_dash_is_refused_in_every_position() {
    let valid: [&[&str]; 6] =
        [&["push", "wu/E/x"], &["pr-open", "wu/E/x"], &["pr-open", "wu/E/x", "--base", "main"], &["pr-ready", "7"], &["post", "7", "a"], &["sync", "epic/E"]];
    let mut next = common::lcg(7);
    let tails = ["", "x", "-", "wu/E/x", "main", "7", "force", "-"];
    for args in valid {
        for i in 0..args.len() {
            for _ in 0..3 {
                let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                a[i] = format!("-{}", tails[next(tails.len())]);
                let a: Vec<&str> = a.iter().map(String::as_str).collect();
                let (code, err) = check(&a);
                assert_eq!(code, Some(2), "{a:?}: {err}");
                assert!(!err.contains("no App key"), "{a:?} passed the argument check");
                let why = if i == 0 || args[i] == "--base" { "usage: " } else { "may not start with -" };
                assert!(err.contains(why), "{a:?}: wanted {why:?} in {err:?}");
            }
        }
    }
}

/// E-GHFLOW.W3: `issue` takes a work branch as push does; `watch-read` (the watch's internal read,
/// never a router shape) takes a kind and a number.
#[test]
fn issue_takes_a_work_branch_and_watch_read_a_kind_and_a_number() {
    for b in ["wu/E/x", "epic/E-GHFLOW"] {
        accepted(&["issue", b]);
    }
    refused(&["issue"], "usage: issue <wu/E/slug|epic/E>");
    refused(&["issue", "wu/E/x", "x"], "usage: issue <wu/E/slug|epic/E>");
    for b in ["main", "wu/E", "refs/heads/wu/E/x", "epic/E/x"] {
        refused(&["issue", b], "branch must be");
    }
    refused(&["issue", "-x"], "may not start with -");
    for k in ["review", "comment", "merged", "checks"] {
        accepted(&["watch-read", k, "31"]);
    }
    refused(&["watch-read", "comment"], "usage: watch-read");
    refused(&["watch-read", "approve", "1"], "usage: watch-read");
    refused(&["watch-read", "comment", "1", "since"], "usage: watch-read");
    refused(&["watch-read", "comment", "0"], "must be a number");
}
