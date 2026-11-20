//! The router's update log (E-APPLIST.W1, K14): `scripts/router-app.sh`, as the router, its watchdog
//! and its sender run it. Every case copies the shipped script into its own temp git repo and runs it
//! as a process, so the tests are hermetic and parallel. The contract is docs/local-app.html "Two
//! sources": `.router/app/log.jsonl` holds one numbered line per change, `state.json` every value with
//! the last line's number folded in, and only this script writes them, under one lock.
compiled!();
use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

use crate::common;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// One temp repo holding a copy of the shipped script; its update log lives in `<main>/.router/app`.
struct App {
    d: TempDir,
}

fn git(dir: &Path, args: &[&str]) {
    let o = common::cmd("git").args(args).current_dir(dir).envs(common::TESTER).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

impl App {
    fn new() -> Self {
        let d = TempDir::new().unwrap();
        let main = d.path().join("main");
        std::fs::create_dir_all(main.join("scripts")).unwrap();
        std::fs::copy(root().join("scripts/router-app.sh"), main.join("scripts/router-app.sh")).unwrap();
        // the script clears every GIT_*, so a shim keeps its git off the developer's config
        common::git_shim(&d.path().join("shim"));
        git(&main, &["init", "-q", "-b", "main"]);
        App { d }
    }

    fn main(&self) -> PathBuf {
        self.d.path().join("main").canonicalize().unwrap()
    }

    fn dir(&self) -> PathBuf {
        self.main().join(".router/app")
    }

    fn log(&self) -> PathBuf {
        self.dir().join("log.jsonl")
    }

    fn state_path(&self) -> PathBuf {
        self.dir().join("state.json")
    }

    fn cmd_at(&self, checkout: &Path, args: &[&str]) -> Command {
        let mut c = common::cmd("sh");
        c.arg(checkout.join("scripts/router-app.sh")).args(args).env("PATH", common::path_with(self.d.path().join("shim"))).current_dir(self.d.path());
        c
    }

    fn cmd(&self, args: &[&str]) -> Command {
        self.cmd_at(&self.main(), args)
    }

    /// `cmd` with the programs in `shims` (a `mv`, a `sleep`) found before the real ones.
    fn cmd_shimmed(&self, shims: &Path, args: &[&str]) -> Command {
        let mut c = self.cmd(args);
        c.env("PATH", common::test_path(&common::path_with(self.d.path().join("shim")), Some(shims)));
        c
    }

    /// One call: its exit code and both streams.
    fn run(&self, args: &[&str]) -> (Option<i32>, String) {
        let o = self.cmd(args).output().unwrap();
        (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
    }

    /// A call that must succeed; returns its `OK run=<run> n=<n>` line.
    fn ok(&self, args: &[&str]) -> String {
        let (code, out) = self.run(args);
        assert_eq!(code, Some(0), "{args:?} failed: {out}");
        assert!(out.starts_with("OK run=") && out.contains(" n="), "{args:?} printed no OK line: {out:?}");
        out
    }

    fn start(&self, name: &str) -> String {
        let out = self.ok(&["start", name]);
        out.trim().strip_prefix("OK run=").unwrap().split(' ').next().unwrap().to_string()
    }

    fn lines(&self) -> Vec<serde_json::Value> {
        let raw = std::fs::read_to_string(self.log()).unwrap();
        assert!(raw.ends_with('\n'), "the log's last line is not whole: {raw:?}");
        raw.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not one JSON line ({e}): {l:?}"))).collect()
    }

    fn state(&self) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(self.state_path()).unwrap()).unwrap()
    }

    /// Both files' bytes, or None for one that is absent.
    fn bytes(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        (std::fs::read(self.log()).ok(), std::fs::read(self.state_path()).ok())
    }

    /// Every name in the app's directory: a writer leaves its two files and nothing else (no temp
    /// file, no lock).
    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = match std::fs::read_dir(self.dir()) {
            Ok(d) => d.map(|e| e.unwrap().file_name().into_string().unwrap()).collect(),
            Err(_) => Vec::new(),
        };
        v.sort();
        v
    }
}

/// `<dir>/<prog>`: a shell script of `body` that a writer run by `cmd_shimmed(dir, ..)` finds first.
fn shim(dir: &Path, prog: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(prog), format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(dir.join(prog), PermissionsExt::from_mode(0o755)).unwrap();
}

/// The real `prog` on the test PATH, for a shim that hands most calls on.
fn real(prog: &str) -> String {
    let o = common::cmd("sh").args(["-c", &format!("command -v {prog}")]).output().unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// A log of lines 1..=`last` in run `run` (a start, then sets of 50 worker keys) and the state that
/// folds it, written in place of the ones `start` made; returns the state's values.
fn seed(a: &App, run: &str, last: u64) -> serde_json::Map<String, serde_json::Value> {
    let at = a.lines()[0]["at"].clone();
    let mut log = String::new();
    let mut values = serde_json::Map::new();
    log.push_str(&format!("{}\n", serde_json::json!({"run": run, "n": 1, "at": at, "op": "start"})));
    for n in 2..=last {
        let key = format!("worker/w{}", n % 50);
        log.push_str(&format!("{}\n", serde_json::json!({"run": run, "n": n, "at": at, "op": "set", "key": key, "value": n})));
        values.insert(key, n.into());
    }
    std::fs::write(a.log(), log).unwrap();
    std::fs::write(a.state_path(), serde_json::json!({"run": run, "n": last, "at": at, "values": values, "alerts": []}).to_string()).unwrap();
    values
}

/// The date the app reads: local time with the offset as +hh:mm.
fn is_at(v: &serde_json::Value) -> bool {
    regex::Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d$").unwrap().is_match(v.as_str().unwrap_or(""))
}

/// The state the app would build from the log: every line folded in order (alerts: the last 20).
fn fold(lines: &[serde_json::Value]) -> (BTreeMap<String, serde_json::Value>, Vec<serde_json::Value>) {
    let mut values = BTreeMap::new();
    let mut alerts = Vec::new();
    for l in lines {
        match l["op"].as_str().unwrap() {
            "start" => {
                values.clear();
                alerts.clear();
            }
            "set" => {
                values.insert(l["key"].as_str().unwrap().to_string(), l["value"].clone());
            }
            "del" => {
                values.remove(l["key"].as_str().unwrap());
            }
            "alert" => {
                let mut a = serde_json::json!({"n": l["n"], "at": l["at"]});
                a.as_object_mut().unwrap().extend(l["value"].as_object().unwrap().clone());
                alerts.push(a);
            }
            op => panic!("unknown op {op}"),
        }
    }
    let keep = alerts.len().saturating_sub(20);
    (values, alerts.split_off(keep))
}

/// The state's n, run, values and alerts are the log folded in order, its n the last line's.
fn assert_state_is_the_log_folded(a: &App) {
    let lines = a.lines();
    let state = a.state();
    let last = lines.last().unwrap();
    assert_eq!(state["n"], last["n"], "the state's n is not the last line's");
    assert_eq!(state["run"], last["run"]);
    assert_eq!(state["at"], last["at"]);
    let (values, alerts) = fold(&lines);
    let got: BTreeMap<String, serde_json::Value> = serde_json::from_value(state["values"].clone()).unwrap();
    assert_eq!(got, values, "the state's values are not the log folded");
    assert_eq!(state["alerts"].as_array().unwrap(), &alerts, "the state's alerts are not the log's last 20");
}

#[test]
fn start_wipes_both_files_and_begins_a_new_run() {
    let a = App::new();
    let run = a.start("keel-1c");
    assert!(regex::Regex::new(r"^keel-1c\.\d{8}-\d{6}\.\d+$").unwrap().is_match(&run), "run id {run:?}");
    let lines = a.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["run"], run.as_str());
    assert_eq!(lines[0]["n"], 1);
    assert_eq!(lines[0]["op"], "start");
    assert!(is_at(&lines[0]["at"]), "{:?}", lines[0]);
    assert_eq!(lines[0].as_object().unwrap().keys().collect::<Vec<_>>(), ["run", "n", "at", "op"]);
    let state = a.state();
    assert_eq!(state["run"], run.as_str());
    assert_eq!(state["n"], 1);
    assert_eq!(state["values"], serde_json::json!({}));
    assert_eq!(state["alerts"], serde_json::json!([]));
    assert!(is_at(&state["at"]), "{state}");

    a.ok(&["stage", "E-X.W1", "building"]);
    a.ok(&["set", "worker/w1", r#"{"state":"busy"}"#]);
    let again = a.start("keel-1c");
    assert_ne!(again, run, "a second start kept the run id");
    let lines = a.lines();
    assert_eq!(lines.len(), 1, "start did not wipe the log: {lines:?}");
    assert_eq!((lines[0]["run"].as_str(), lines[0]["n"].as_u64()), (Some(again.as_str()), Some(1)));
    let state = a.state();
    assert_eq!((state["run"].as_str(), state["n"].as_u64()), (Some(again.as_str()), Some(1)));
    assert_eq!(state["values"], serde_json::json!({}), "start did not wipe the state");
}

#[test]
fn each_verb_writes_one_line_with_the_next_number_and_the_state_folds_it() {
    let a = App::new();
    let run = a.start("r1");
    let mut want = 1;
    let mut step = |args: &[&str]| {
        let out = a.ok(args);
        want += 1;
        assert_eq!(out.trim(), format!("OK run={run} n={want}"), "{args:?}");
        let lines = a.lines();
        assert_eq!(lines.len(), want, "{args:?} did not add exactly one line");
        let last = lines.last().unwrap().clone();
        assert_eq!(last["n"], want);
        assert_eq!(last["run"], run.as_str());
        assert!(is_at(&last["at"]), "{last}");
        assert_state_is_the_log_folded(&a);
        last
    };
    let l = step(&["stage", "E-X.W1", "plan-to-read", "1a2b3c4"]);
    assert_eq!((l["op"].as_str(), l["key"].as_str()), (Some("set"), Some("wu/E-X.W1/stage")));
    assert_eq!(l["value"], serde_json::json!({"stage": "plan to read", "tip": "1a2b3c4"}));
    for (word, shown) in [("plan-in-work", "plan in work"), ("plan-approved", "plan approved"), ("building", "building"), ("in-review", "in review")] {
        let l = step(&["stage", "E-ABC9.W12", word]);
        assert_eq!(l["value"], serde_json::json!({ "stage": shown }), "{word}");
    }
    let l = step(&["set", "worker/w1", r#"{ "state" : "busy", "quiet": 3 }"#]);
    assert_eq!(l["value"], serde_json::json!({"state": "busy", "quiet": 3}));
    for (key, value) in [
        ("router", r#""keel-1c""#),
        ("facts", r#"["one","two"]"#),
        ("wu/E-X.W1/conflict", "true"),
        ("epic/E-X/paused", r#"{"why":"waiting"}"#),
        ("question/q-1", r#""which one?""#),
        ("worker/w1/report", r#""done""#),
    ] {
        let l = step(&["set", key, value]);
        assert_eq!((l["op"].as_str(), l["key"].as_str()), (Some("set"), Some(key)));
    }
    let l = step(&["del", "wu/E-X.W1/stage"]);
    assert_eq!((l["op"].as_str(), l["key"].as_str()), (Some("del"), Some("wu/E-X.W1/stage")));
    assert!(l.get("value").is_none(), "{l}");
    assert!(a.state()["values"].get("wu/E-X.W1/stage").is_none());
    // an absent key still writes its line: the app applies it as a no-op
    step(&["del", "question/never-asked"]);
    for i in 0..21 {
        let v = if i % 2 == 0 {
            format!(r#"{{"headline":"alert {i}","lines":["first"]}}"#)
        } else {
            format!(r#"{{"headline":"alert {i}","lines":["first","second"],"wu":"E-X.W1","link":"keel://wu/E-X.W1"}}"#)
        };
        let l = step(&["alert", &v]);
        assert_eq!(l["op"], "alert");
        assert!(l.get("key").is_none(), "{l}");
    }
    let alerts = a.state()["alerts"].as_array().unwrap().clone();
    assert_eq!(alerts.len(), 20, "the state keeps the last 20 alerts");
    assert_eq!(alerts[0]["headline"], "alert 1");
    assert_eq!(alerts[19]["headline"], "alert 20");
}

/// The line the app parses is a wire contract: snapshot-locked key order and value shape, with only
/// `run` and `at` (asserted present and well-formed) masked.
#[test]
fn a_stage_line_is_the_contract_the_app_reads() {
    let a = App::new();
    let run = a.start("keel-1c");
    a.ok(&["stage", "E-LOCALTOOL.W3", "plan-to-read", "6834699"]);
    a.ok(&["set", "worker/w1", r#"{"state":"busy"}"#]);
    a.ok(&["del", "question/q1"]);
    a.ok(&["alert", r#"{"link":"keel://wu/E-X.W1","lines":["a"],"headline":"h","wu":"E-X.W1"}"#]);
    let raw = std::fs::read_to_string(a.log()).unwrap();
    let at = regex::Regex::new(r#""at":"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d)""#).unwrap();
    assert_eq!(at.find_iter(&raw).count(), 5, "every line carries a well-formed at: {raw}");
    let masked = at.replace_all(&raw.replace(&run, "RUN"), r#""at":"AT""#).into_owned();
    assert_eq!(
        masked,
        concat!(
            r#"{"run":"RUN","n":1,"at":"AT","op":"start"}"#,
            "\n",
            r#"{"run":"RUN","n":2,"at":"AT","op":"set","key":"wu/E-LOCALTOOL.W3/stage","value":{"stage":"plan to read","tip":"6834699"}}"#,
            "\n",
            r#"{"run":"RUN","n":3,"at":"AT","op":"set","key":"worker/w1","value":{"state":"busy"}}"#,
            "\n",
            r#"{"run":"RUN","n":4,"at":"AT","op":"del","key":"question/q1"}"#,
            "\n",
            r#"{"run":"RUN","n":5,"at":"AT","op":"alert","value":{"link":"keel://wu/E-X.W1","lines":["a"],"headline":"h","wu":"E-X.W1"}}"#,
            "\n",
        )
    );
    let state = a.state();
    assert_eq!(state.as_object().unwrap().keys().collect::<Vec<_>>(), ["run", "n", "at", "values", "alerts"]);
    let raw_state = std::fs::read_to_string(a.state_path()).unwrap();
    assert!(raw_state.starts_with(&format!(r#"{{"run":"{run}","n":5,"at":""#)), "the state's keys are not run, n, at first: {raw_state}");
}

#[test]
fn sixteen_writers_at_once_keep_the_numbers_whole() {
    let a = App::new();
    a.start("r1");
    let children: Vec<(usize, Child)> = (0..16)
        .map(|i| {
            let mut c = a.cmd(&["set", &format!("worker/w{i}"), &format!(r#"{{"i":{i}}}"#)]);
            (i, c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap())
        })
        .collect();
    for (i, c) in children {
        let o = c.wait_with_output().unwrap();
        assert!(o.status.success(), "writer {i}: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    }
    let lines = a.lines();
    let ns: Vec<u64> = lines.iter().map(|l| l["n"].as_u64().unwrap()).collect();
    assert_eq!(ns, (1..=17).collect::<Vec<u64>>(), "numbers skipped or repeated");
    assert_state_is_the_log_folded(&a);
    assert_eq!(a.state()["values"].as_object().unwrap().len(), 16);
    assert_eq!(a.names(), ["log.jsonl", "state.json"], "a writer left a temp file or its lock");
}

#[test]
fn the_2001st_line_restarts_the_log_from_the_state() {
    let a = App::new();
    let run = a.start("r1");
    let mut values = seed(&a, &run, 2000);

    a.ok(&["set", "question/q1", r#""new""#]);
    let lines = a.lines();
    assert_eq!(lines.len(), 1, "the log did not restart");
    assert_eq!((lines[0]["n"].as_u64(), lines[0]["key"].as_str()), (Some(2001), Some("question/q1")));
    let state = a.state();
    assert_eq!(state["n"], 2001);
    values.insert("question/q1".into(), "new".into());
    assert_eq!(state["values"], serde_json::Value::Object(values), "the state lost an earlier value");

    a.ok(&["del", "question/q1"]);
    let ns: Vec<u64> = a.lines().iter().map(|l| l["n"].as_u64().unwrap()).collect();
    assert_eq!(ns, [2001, 2002], "the restarted log does not go on");
    assert_eq!(a.state()["n"], 2002);
}

/// Review round 1 (I1): the log restarts only past 2,000 lines. A log of 1,999 takes its 2,000th line
/// as an append; only the line after it restarts the log.
#[test]
fn a_log_of_1999_lines_takes_the_2000th_before_it_restarts() {
    let a = App::new();
    let run = a.start("r1");
    seed(&a, &run, 1999);

    a.ok(&["set", "question/q1", r#""last""#]);
    let ns: Vec<u64> = a.lines().iter().map(|l| l["n"].as_u64().unwrap()).collect();
    assert_eq!(ns, (1..=2000).collect::<Vec<u64>>(), "the 2,000th line did not append to the log");
    assert_state_is_the_log_folded(&a);

    a.ok(&["set", "question/q1", r#""first""#]);
    let lines = a.lines();
    assert_eq!(lines.len(), 1, "the 2,001st line did not restart the log");
    assert_eq!(lines[0]["n"], 2001);
    assert_eq!(a.state()["n"], 2001);
}

#[test]
fn a_refused_call_writes_nothing() {
    let a = App::new();
    // no run yet: refused, and neither file appears
    for args in [&["set", "worker/w1", "1"][..], &["del", "question/q1"], &["stage", "E-X.W1", "building"], &["alert", r#"{"headline":"h","lines":["a"]}"#]] {
        let (code, out) = a.run(args);
        assert_eq!(code, Some(2), "{args:?} before start: {out}");
        assert!(out.contains("no run; start first"), "{args:?}: {out}");
        assert_eq!(a.bytes(), (None, None), "{args:?} before start wrote a file");
        assert_eq!(a.names(), Vec::<String>::new(), "{args:?} before start left a file");
    }
    a.start("r1");
    a.ok(&["stage", "E-X.W1", "building"]);
    let before = a.bytes();
    let big = format!(r#""{}""#, "x".repeat(511));
    let wide = "€".repeat(200);
    let oversize = format!(r#"{{"headline":"h","lines":["{wide}","{wide}"]}}"#);
    let long_head = format!(r#"{{"headline":"{}","lines":["a"]}}"#, "h".repeat(81));
    let long_line = format!(r#"{{"headline":"h","lines":["{}"]}}"#, "l".repeat(201));
    let long_name = "r".repeat(33);
    let long_link = format!(r#"{{"headline":"h","lines":["a"],"link":"keel://{}"}}"#, "a".repeat(201));
    let refused: Vec<Vec<&str>> = vec![
        vec![],
        vec!["frob"],
        vec!["frob", "x"],
        vec!["start"],
        vec!["start", "R1"],
        vec!["start", "-r"],
        vec!["start", &long_name],
        vec!["start", "r1", "extra"],
        vec!["stage"],
        vec!["stage", "E-X.W1"],
        vec!["stage", "E-X.W1", "done"],
        vec!["stage", "E-X.W1", "plan to read"],
        vec!["stage", "E-x.W1", "building"],
        vec!["stage", "E-X.W1", "building", "1A2B3C4"],
        vec!["stage", "E-X.W1", "building", "1a2b3c"],
        vec!["stage", "E-X.W1", "building", "1a2b3c4", "extra"],
        vec!["set", "worker/w1"],
        vec!["set", "worker/w1", "1", "extra"],
        vec!["set", "nokey", "1"],
        vec!["set", "wu/E-X.W1/stagex", "1"],
        vec!["set", "worker/W1", "1"],
        vec!["set", "worker/w1/reports", "1"],
        vec!["set", "question/../x", "1"],
        vec!["set", "worker/w1", "{bad"],
        vec!["set", "worker/w1", ""],
        vec!["set", "worker/w1", "1 2"],
        vec!["set", "worker/w1", "null"],
        vec!["set", "worker/w1", "nan"],
        vec!["set", "worker/w1", &big],
        vec!["del"],
        vec!["del", "nokey"],
        vec!["del", "worker/w1", "extra"],
        vec!["alert"],
        vec!["alert", "[]"],
        vec!["alert", r#"{"headline":"h"}"#],
        vec!["alert", r#"{"headline":"","lines":["a"]}"#],
        vec!["alert", r#"{"headline":"h","lines":[]}"#],
        vec!["alert", r#"{"headline":"h","lines":["a","b","c"]}"#],
        vec!["alert", r#"{"headline":"h","lines":[""]}"#],
        vec!["alert", r#"{"headline":"h","lines":[1]}"#],
        vec!["alert", r#"{"headline":"h","lines":["a"],"extra":1}"#],
        vec!["alert", r#"{"headline":"h","lines":["a"],"wu":"E-X"}"#],
        vec!["alert", r#"{"headline":"h","lines":["a"],"link":"https://x"}"#],
        vec!["alert", r#"{"headline":"h","lines":["a"],"link":"keel://a b"}"#],
        vec!["alert", &long_head],
        vec!["alert", &long_line],
        vec!["alert", &long_link],
        vec!["alert", &oversize],
        vec!["alert", r#"{"headline":"h","lines":["a"]}"#, "extra"],
    ];
    for args in &refused {
        let (code, out) = a.run(args);
        assert_eq!(code, Some(2), "{args:?} was not refused: {out}");
        assert!(out.starts_with("router-app: "), "{args:?} gave no reason: {out:?}");
        assert!(a.bytes() == before, "{args:?} changed a file");
        assert_eq!(a.names(), ["log.jsonl", "state.json"], "{args:?} left a file");
    }
    let (_, out) = a.run(&["alert", &oversize]);
    assert!(out.contains("1024"), "the oversize refusal does not name the cap: {out}");
    let (_, out) = a.run(&["set", "worker/w1", &big]);
    assert!(out.contains("512"), "the value refusal does not name the cap: {out}");
    // the largest value a set takes still goes through
    a.ok(&["set", "worker/w1", &format!(r#""{}""#, "x".repeat(510))]);
}

#[test]
fn a_held_lock_makes_a_writer_wait_and_a_stale_one_refuse() {
    let a = App::new();
    a.start("r1");
    let lock = a.dir().join(".lock");
    std::fs::create_dir(&lock).unwrap();
    let mut w = a.cmd(&["set", "worker/w1", "1"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    assert!(w.try_wait().unwrap().is_none(), "the writer did not wait for the held lock");
    assert_eq!(a.lines().len(), 1, "the writer wrote under another's lock");
    std::fs::remove_dir(&lock).unwrap();
    let o = w.wait_with_output().unwrap();
    assert!(o.status.success(), "the writer failed once the lock was free: {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(a.lines().len(), 2);

    // review round 1 (I5): the wait is pinned by its polls, not by the clock - a `sleep` that only
    // records its argument shows every poll and how long each would have slept
    std::fs::create_dir(&lock).unwrap();
    let before = a.bytes();
    let shims = a.d.path().join("sleep-counts");
    let polls = a.d.path().join("polls");
    shim(&shims, "sleep", &format!("printf '%s\\n' \"$*\" >> {}", common::sh_quote(polls.to_str().unwrap())));
    let t = Instant::now();
    let o = a.cmd_shimmed(&shims, &["set", "worker/w1", "2"]).output().unwrap();
    let took = t.elapsed();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.status.code(), Some(1), "a held lock did not fail the writer: {out}");
    assert!(out.contains("LOCKED") && out.contains(lock.to_str().unwrap()), "the refusal does not name the lock: {out}");
    let polls = std::fs::read_to_string(&polls).unwrap_or_default();
    let polls: Vec<&str> = polls.lines().collect();
    assert!(polls.iter().all(|p| *p == "0.05"), "a poll did not sleep 0.05 s: {polls:?}");
    assert!((99..=100).contains(&polls.len()), "the writer slept {} times 0.05 s, not 5 s in all", polls.len());
    assert!(took < Duration::from_secs(4), "the writer slept in a real sleep, not the shim: {took:?}");
    assert!(a.bytes() == before, "a writer that never got the lock wrote");
    assert!(lock.is_dir(), "the writer broke a lock it never held");
}

#[test]
fn a_symlinked_app_dir_or_file_is_refused() {
    // .router itself (review round 1 nit): nothing is made through it
    let a = App::new();
    let elsewhere = a.d.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, a.main().join(".router")).unwrap();
    let (code, out) = a.run(&["start", "r1"]);
    assert_eq!(code, Some(2), "a symlinked .router was written through: {out}");
    assert!(out.contains("symlink"), "{out}");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "the link's target was written");

    // the directory
    let a = App::new();
    let elsewhere = a.d.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::create_dir_all(a.main().join(".router")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, a.dir()).unwrap();
    let (code, out) = a.run(&["start", "r1"]);
    assert_eq!(code, Some(2), "a symlinked app dir was written through: {out}");
    assert!(out.contains("symlink"), "{out}");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "the link's target was written");

    // each file
    for f in ["log.jsonl", "state.json"] {
        let a = App::new();
        a.start("r1");
        let target = a.d.path().join("target");
        std::fs::rename(a.dir().join(f), &target).unwrap();
        std::os::unix::fs::symlink(&target, a.dir().join(f)).unwrap();
        let other = if f == "log.jsonl" { a.state_path() } else { a.log() };
        let (want, other_bytes) = (std::fs::read(&target).unwrap(), std::fs::read(&other).unwrap());
        for args in [&["set", "worker/w1", "1"][..], &["start", "r1"]] {
            let (code, out) = a.run(args);
            assert_eq!(code, Some(2), "{f} symlinked: {args:?} went through: {out}");
            assert!(out.contains("symlink"), "{out}");
            assert_eq!(std::fs::read(&target).unwrap(), want, "{f}'s target was written");
            assert_eq!(std::fs::read(&other).unwrap(), other_bytes, "{args:?} wrote the other file");
            assert!(a.dir().join(f).is_symlink(), "the link was replaced");
        }
    }
}

/// The production path: a copy in a linked worktree writes the main checkout's log, so the router
/// and its watchdog, wherever they run, feed the one log the app reads.
#[test]
fn a_worktree_copy_writes_the_main_checkouts_log() {
    let a = App::new();
    let main = a.main();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["worktree", "add", "-q", "../wt", "-b", "w"]);
    let wt = a.d.path().join("wt");
    let o = a.cmd_at(&wt, &["start", "r1"]).current_dir(&wt).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = a.cmd_at(&wt, &["stage", "E-X.W1", "building"]).current_dir(&wt).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(a.lines().len(), 2, "the worktree copy did not write the main checkout's log");
    assert!(!wt.join(".router").exists(), "the worktree copy wrote a log of its own");
}

#[test]
fn text_with_a_newline_or_quote_stays_one_escaped_line() {
    let a = App::new();
    a.start("r1");
    let text = "line one\nline \"two\"\t\\ end\u{2028}";
    let json = serde_json::to_string(text).unwrap();
    a.ok(&["set", "question/q1", &json]);
    let alert = serde_json::json!({"headline": "a \"quoted\"\nheadline", "lines": ["one\r\ntwo", "it's"]});
    a.ok(&["alert", &alert.to_string()]);
    let raw = std::fs::read_to_string(a.log()).unwrap();
    assert_eq!(raw.matches('\n').count(), 3, "a value broke its line: {raw:?}");
    let lines = a.lines();
    assert_eq!(lines[1]["value"], text);
    assert_eq!(lines[2]["value"], alert);
    assert_state_is_the_log_folded(&a);
    // a raw newline in a word is never JSON or a key: refused
    for args in [&["set", "question/q1", "\"a\nb\""][..], &["set", "question/q1\n", "1"], &["start", "r1\nx"], &["del", "question/q1\nx"]] {
        let (code, out) = a.run(args);
        assert_eq!(code, Some(2), "{args:?}: {out}");
    }
    assert_eq!(a.lines().len(), 3);
}

/// The line an alert of `alert` makes at its longest, as the script measures it before the lock (it
/// cannot know n or the time yet): the longest run id (a 32-byte name, its 15-byte date and a 10-digit
/// pid: 59 bytes), a 12-digit n and the 25-byte date.
fn longest_alert_line(alert: &str) -> usize {
    format!(r#"{{"run":"{}","n":999999999999,"at":"{}","op":"alert","value":{alert}}}"#, "x".repeat(59), "x".repeat(25)).len()
}

/// Review round 1 (I2): the cap is 1,024 bytes of the whole line, not of the value. An alert of two
/// lines of 140 euro signs (420 bytes each) and a headline sized to make the line exactly 1,024 bytes
/// goes through; one byte more is refused, and nothing written is over the cap.
#[test]
fn a_line_of_exactly_1024_bytes_is_taken_and_one_byte_more_refused() {
    let a = App::new();
    a.start("r1");
    let e = "€".repeat(140);
    let alert = |h: usize| format!(r#"{{"headline":"{}","lines":["{e}","{e}"]}}"#, "h".repeat(h));
    let fit = 1024 - longest_alert_line(&alert(0));
    assert!((1..80).contains(&fit), "the headline that fits is {fit} bytes");
    assert_eq!(longest_alert_line(&alert(fit)), 1024);
    assert!(alert(fit + 1).len() < 1024, "the value alone is under the cap, so only the whole line can refuse it");

    let before = a.bytes();
    let (code, out) = a.run(&["alert", &alert(fit + 1)]);
    assert_eq!(code, Some(2), "a 1,025-byte line was not refused: {out}");
    assert!(out.contains("1025") && out.contains("1024"), "the refusal does not name the size and the cap: {out}");
    assert!(a.bytes() == before, "a refused alert changed a file");

    a.ok(&["alert", &alert(fit)]);
    let lines = a.lines();
    assert_eq!(lines.last().unwrap()["value"], serde_json::from_str::<serde_json::Value>(&alert(fit)).unwrap());
    for l in std::fs::read_to_string(a.log()).unwrap().lines() {
        assert!(l.len() <= 1024, "a written line is {} bytes", l.len());
    }
}

/// Review round 1 (I3): a router or watchdog started from another repository's git hook inherits
/// GIT_DIR, GIT_WORK_TREE and GIT_COMMON_DIR naming that repository. The script clears them, so the
/// log is still the main checkout's and the other repository gets nothing.
#[test]
fn an_inherited_git_dir_never_moves_the_log() {
    let a = App::new();
    let decoy = a.d.path().join("decoy");
    std::fs::create_dir_all(&decoy).unwrap();
    git(&decoy, &["init", "-q", "-b", "main"]);
    std::fs::write(decoy.join("f"), "x").unwrap();
    git(&decoy, &["add", "f"]);
    git(&decoy, &["commit", "-qm", "other"]);
    let o = decoy.display();
    let hostile = [
        ("GIT_DIR", format!("{o}/.git")),
        ("GIT_WORK_TREE", o.to_string()),
        ("GIT_COMMON_DIR", format!("{o}/.git")),
        ("GIT_INDEX_FILE", format!("{o}/.git/index")),
    ];
    for args in [&["start", "r1"][..], &["stage", "E-X.W1", "building"]] {
        let out = a.cmd(args).envs(hostile.iter().map(|(k, v)| (k, v))).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(!decoy.join(".router").exists(), "the inherited GIT_DIR moved the log into the other repository");
    let ns: Vec<u64> = a.lines().iter().map(|l| l["n"].as_u64().unwrap()).collect();
    assert_eq!(ns, [1, 2], "the main checkout's log did not get both lines");
}

/// Review round 1 (I4): the state is written before the log, so a reader that meets line n and reads
/// the state finds n in it, and a writer that fails between its two writes leaves a gap the app reads
/// the state for, never a repeated number. A `mv` that fails on one file stops a writer at that write.
#[test]
fn a_writer_that_fails_between_its_writes_leaves_a_gap_never_a_repeat() {
    let mv_fails_on = |a: &App, file: &str| -> PathBuf {
        let shims = a.d.path().join(format!("mv-fails-{file}"));
        shim(
            &shims,
            "mv",
            &format!("case \"$*\" in */{file}) echo 'mv: the test refuses {file}' >&2; exit 1 ;; esac\nexec {} \"$@\"", common::sh_quote(&real("mv"))),
        );
        shims
    };
    let ns = |a: &App| -> Vec<u64> { a.lines().iter().map(|l| l["n"].as_u64().unwrap()).collect() };
    let no_line_ahead_of_the_state = |a: &App| {
        let n = a.state()["n"].as_u64().unwrap();
        let ahead: Vec<u64> = ns(a).into_iter().filter(|&l| l > n).collect();
        assert!(ahead.is_empty(), "the log holds lines {ahead:?} the state (n={n}) has not folded");
    };

    // the state's rename fails: nothing is written, and the next writer takes the next number
    let a = App::new();
    a.start("r1");
    a.ok(&["set", "worker/w1", "1"]);
    let o = a.cmd_shimmed(&mv_fails_on(&a, "state.json"), &["set", "worker/w1", "2"]).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", String::from_utf8_lossy(&o.stderr));
    no_line_ahead_of_the_state(&a);
    a.ok(&["set", "worker/w1", "3"]);
    a.ok(&["del", "worker/w1"]);
    let got = ns(&a);
    assert!(got.windows(2).all(|w| w[0] < w[1]), "a number repeated after a failed writer: {got:?}");
    assert_eq!(got, [1, 2, 3, 4]);
    assert_state_is_the_log_folded(&a);

    // the log's write fails after the state's (a restart, which renames): the state is ahead by one
    // line, and the next writer leaves that number out
    let a = App::new();
    let run = a.start("r1");
    seed(&a, &run, 2000);
    let o = a.cmd_shimmed(&mv_fails_on(&a, "log.jsonl"), &["set", "worker/w1", r#""x""#]).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", String::from_utf8_lossy(&o.stderr));
    no_line_ahead_of_the_state(&a);
    assert_eq!((a.state()["n"].as_u64(), ns(&a).last().copied()), (Some(2001), Some(2000)), "the state was not written first");
    a.ok(&["del", "worker/w1"]);
    assert_eq!(ns(&a), [2002], "the line after the gap");
    assert_eq!(a.state()["n"], 2002);
}

/// Review round 1 (I6): every write replaces state.json by a rename, so a reader holding the old file
/// reads it whole and the path names a new file; a state rewritten in place would keep its inode.
#[test]
fn the_state_is_replaced_by_a_rename_never_rewritten_in_place() {
    use std::io::Read;
    let a = App::new();
    a.start("r1");
    let alert = r#"{"headline":"h","lines":["a"]}"#;
    for args in [&["set", "worker/w1", "1"][..], &["stage", "E-X.W1", "building"], &["del", "worker/w1"], &["alert", alert], &["start", "r2"]] {
        let mut old = std::fs::File::open(a.state_path()).unwrap();
        let (ino, bytes) = (old.metadata().unwrap().ino(), std::fs::read(a.state_path()).unwrap());
        a.ok(args);
        assert_ne!(std::fs::metadata(a.state_path()).unwrap().ino(), ino, "{args:?} rewrote state.json in place");
        let mut held = Vec::new();
        old.read_to_end(&mut held).unwrap();
        assert_eq!(held, bytes, "{args:?}: a reader holding the old state saw it change");
    }
}

/// Review round 1 nits: the router runs the script by its path, so its shebang and mode are part of the
/// contract; and it sets umask 077 whatever its caller's, so the update log is the owner's alone.
#[test]
fn the_script_runs_by_its_shebang_and_keeps_the_log_private() {
    let shipped = root().join("scripts/router-app.sh");
    assert!(std::fs::read_to_string(&shipped).unwrap().starts_with("#!/bin/sh\n"), "the script lost its shebang");
    assert_eq!(std::fs::metadata(&shipped).unwrap().mode() & 0o111, 0o111, "the script is not executable");
    let a = App::new();
    let script = a.main().join("scripts/router-app.sh");
    for args in [&["start", "r1"][..], &["stage", "E-X.W1", "building"]] {
        let o = common::cmd("sh")
            .args(["-c", "umask 022; exec \"$0\" \"$@\""])
            .arg(&script)
            .args(args)
            .env("PATH", common::path_with(a.d.path().join("shim")))
            .current_dir(a.d.path())
            .output()
            .unwrap();
        assert!(o.status.success(), "{args:?} run by its path: {}", String::from_utf8_lossy(&o.stderr));
    }
    assert_eq!(a.lines().len(), 2);
    for (p, want) in [(a.main().join(".router"), 0o700), (a.dir(), 0o700), (a.log(), 0o600), (a.state_path(), 0o600)] {
        let mode = std::fs::metadata(&p).unwrap().mode() & 0o777;
        assert_eq!(mode, want, "{} is {mode:o} under a caller's umask 022", p.display());
    }
}
