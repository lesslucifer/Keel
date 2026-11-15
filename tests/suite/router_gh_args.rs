//! E-GHFLOW.W2, unit layer: `scripts/router-gh.sh`'s argument check, alone. Each case runs this
//! checkout's script with an empty home and a curl that records any call: a refused argument is exit
//! 2 naming its reason, an accepted one goes on to the missing App key, and nothing reaches curl.
compiled!();

use std::path::Path;
use tempfile::TempDir;

use crate::common;

/// Exit code and stderr of the script on `args`; panics if curl was ever called. The guard's
/// agreement test in router.rs reads the script's verdict through this too.
pub(crate) fn check(args: &[&str]) -> (Option<i32>, String) {
    let d = TempDir::new().unwrap();
    let curl = d.path().join("curl");
    std::fs::write(&curl, format!("#!/bin/sh\ntouch {}\n", common::sh_quote(&d.path().join("called").to_string_lossy()))).unwrap();
    std::fs::set_permissions(&curl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/router-gh.sh");
    let o = common::cmd("sh").arg(script).args(args).env("HOME", d.path()).env("ROUTER_GH_CURL", &curl).output().unwrap();
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
/// anything else runs (seeded generator over the valid argument lists).
#[test]
fn every_argument_starting_with_a_dash_is_refused_in_every_position() {
    let valid: [&[&str]; 6] =
        [&["push", "wu/E/x"], &["pr-open", "wu/E/x"], &["pr-open", "wu/E/x", "--base", "main"], &["pr-ready", "7"], &["post", "7", "a"], &["sync", "epic/E"]];
    let mut seed: u64 = 7;
    let tails = ["", "x", "-", "wu/E/x", "main", "7", "force", "-"];
    for args in valid {
        for i in 1..args.len() {
            if args[i] == "--base" {
                continue;
            }
            for _ in 0..3 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                a[i] = format!("-{}", tails[(seed >> 33) as usize % tails.len()]);
                let a: Vec<&str> = a.iter().map(String::as_str).collect();
                let (code, err) = check(&a);
                assert_eq!(code, Some(2), "{a:?}: {err}");
                assert!(!err.contains("no App key"), "{a:?} passed the argument check");
            }
        }
    }
}
