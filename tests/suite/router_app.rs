//! The router's update log (E-APPLIST.W1, K14): `scripts/router-app.sh`, as the router, its watchdog
//! and its sender run it. Every case copies the shipped script into its own temp git repo and runs it
//! as a process, so the tests are hermetic and parallel. The contract is docs/local-app.html "Two
//! sources": `.router/app/log.jsonl` holds one numbered line per change, `state.json` every value with
//! the last line's number folded in, and only this script writes them, under one lock.
compiled!();
use std::collections::BTreeMap;
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
    let at = a.lines()[0]["at"].clone();
    let mut log = String::new();
    let mut values = serde_json::Map::new();
    log.push_str(&format!("{}\n", serde_json::json!({"run": run, "n": 1, "at": at, "op": "start"})));
    for n in 2..=2000u64 {
        let key = format!("worker/w{}", n % 50);
        log.push_str(&format!("{}\n", serde_json::json!({"run": run, "n": n, "at": at, "op": "set", "key": key, "value": n})));
        values.insert(key, n.into());
    }
    std::fs::write(a.log(), log).unwrap();
    std::fs::write(a.state_path(), serde_json::json!({"run": run, "n": 2000, "at": at, "values": values, "alerts": []}).to_string()).unwrap();

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

    std::fs::create_dir(&lock).unwrap();
    let before = a.bytes();
    let t = Instant::now();
    let (code, out) = a.run(&["set", "worker/w1", "2"]);
    let took = t.elapsed();
    assert_eq!(code, Some(1), "a held lock did not fail the writer: {out}");
    assert!(out.contains("LOCKED") && out.contains(lock.to_str().unwrap()), "the refusal does not name the lock: {out}");
    assert!(took >= Duration::from_secs(4) && took < Duration::from_secs(15), "the writer waited {took:?}, not about 5 s");
    assert!(a.bytes() == before, "a writer that never got the lock wrote");
    assert!(lock.is_dir(), "the writer broke a lock it never held");
}

#[test]
fn a_symlinked_app_dir_or_file_is_refused() {
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
