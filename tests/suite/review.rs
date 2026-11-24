//! E-CRLIMIT epic e2e (E-CRCAP's before it): the review schedule - the target's `[review]` settings,
//! the 3-round schedule by default - ends in a fix-PASS or an owner-accepted waiver, and both merge
//! through the real hooks of a repo `keel init` set up (UC-REV-2, UC-REV-3, UC-GATE-1).
compiled!();

use serde_json::Value;
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common;

fn run(prog: &str, d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    let mut c = common::cmd(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", common::path_with(bin))
        .env_remove("SSH_AUTH_SOCK")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn ok(o: Output) -> Output {
    assert!(o.status.success(), "{}", text(&o));
    o
}

fn refused(o: &Output, needles: &[&str]) {
    let t = text(o);
    assert!(!o.status.success() && t.contains("fix ->"), "expected a refusal: {t}");
    for n in needles {
        assert!(t.contains(n), "expected '{n}': {t}");
    }
}

fn git(d: &Path, a: &[&str]) -> String {
    let o = ok(run("git", d, a, ""));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn keel(d: &Path, a: &[&str]) -> Output {
    run(env!("CARGO_BIN_EXE_keel"), d, a, "")
}

fn write(d: &Path, rel: &str, body: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, body).unwrap();
}

fn commit(d: &Path, msg: &str) -> String {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", msg]);
    git(d, &["rev-parse", "HEAD"])
}

fn note(p: &Path, kind: &str, rev: &str) -> Value {
    serde_json::from_str(&git(p, &["notes", &format!("--ref={kind}"), "show", rev])).unwrap()
}

fn cr(p: &Path, args: &[&str]) -> Output {
    keel(p, &[&["notes", "write", "cr"][..], args].concat())
}

/// The test and doc notes on HEAD, as the workflow writes them before each review.
fn notes(p: &Path, judgment: &str, extra: &[&str]) {
    ok(run(env!("CARGO_BIN_EXE_keel"), p, &[&["notes", "write", "test"][..], extra].concat(), judgment));
    ok(keel(p, &[&["notes", "write", "doc"][..], extra].concat()));
}

/// Red only on the missing sign-off.
fn red_only_on_8(o: &Output) {
    let t = text(o);
    assert!(!o.status.success() && t.contains("8·manual-review") && t.contains("- 1 failed"), "{t}");
}

fn code(o: &Output, flag: &str) -> String {
    let re = regex::Regex::new(&format!(r"{flag} ([0-9a-f]{{12}})")).unwrap();
    re.captures(&text(o)).unwrap_or_else(|| panic!("no {flag} code: {}", text(o)))[1].to_string()
}

fn whole(n: u32, of: u32, base: &str, tip: &str) -> String {
    format!("review round {n} of {of} - scope: the whole range, {base}..{tip}")
}

fn last_fix(n: u32, of: u32, since: &str, tip: &str) -> String {
    format!("review round {n} of {of} - scope: the last fix, {since}..{tip}")
}

const RESULT: &str = "<html><p>done</p></html>";

/// WU `name` cut off the epic: its code, its test, its result report, and the test and doc notes on
/// its first commit. Returns that commit and the WU's test judgment.
fn start_wu(p: &Path, name: &str) -> (String, String) {
    git(p, &["switch", "-q", "-c", &format!("wu/e/{name}")]);
    write(p, &format!("src/{name}.py"), &format!("def {name}():\n    return 1\n"));
    write(p, &format!("tests/test_{name}.py"), &format!("def test_{name}():\n    pass\n"));
    common::doc_pair(p, &format!("docs/work/{name}-result.html"), "wu-result", RESULT);
    let first = commit(p, &format!("wu {name}"));
    let tests = format!(r#"{{"integration":{{"files":["tests/test_{name}.py"],"covers":"{name} works"}}}}"#);
    notes(p, &tests, &[]);
    (first, tests)
}

/// A fix of WU `name`'s last round, with its notes: returns the fix commit.
fn fix(p: &Path, name: &str, n: u32, tests: &str) -> String {
    write(p, &format!("src/{name}.py"), &format!("def {name}():\n    return {n}\n"));
    let c = commit(p, &format!("fix round {}", n - 1));
    notes(p, tests, &[]);
    c
}

fn preflight(p: &Path, name: &str) -> Output {
    keel(p, &["preflight", "wu", &format!("wu/e/{name}")])
}

fn merge(p: &Path, name: &str) {
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", &format!("merge wu {name}"), &format!("wu/e/{name}")], ""));
}

#[test]
fn epic_e2e_the_review_schedule_ends_in_a_fix_pass_or_an_owner_accepted_waiver() {
    let (d, k) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    ok(keel(p, &["init"]));
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    let cfg: String = cfg
        .lines()
        .map(|l| {
            if l.starts_with("lint = ") {
                "lint = \"true\""
            } else if l.starts_with("test = ") {
                "test = \"true\""
            } else {
                l
            }
        })
        .map(|l| format!("{l}\n"))
        .collect();
    write(p, ".keel/config.toml", &cfg);
    let key = k.path().join("owner");
    ok(run("ssh-keygen", k.path(), &["-q", "-t", "ed25519", "-N", "", "-f", "owner"], ""));
    ok(keel(p, &["mr", "setup", "--as", "vu", "--key", key.to_str().unwrap()]));
    write(p, "README", "x");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "keel init"]);

    git(p, &["switch", "-q", "-c", "epic/e"]);
    common::doc_pair(p, "docs/work/e-plan.html", "epic-plan", "<html><p>plan</p></html>");
    commit(p, "epic plan");

    // The defaults (no schedule key): WU A - round 1 finds only nits; the fix gets a fix-PASS, no new round.
    let (a1, a_tests) = start_wu(p, "a");
    let a_base = git(p, &["merge-base", "epic/e", "HEAD"]);
    assert!(text(&ok(preflight(p, "a"))).contains(&whole(1, 3, &a_base, &a1)));
    ok(cr(p, &["--verdict", "PASS", "--nits", r#"[{"location": "src/a.py:1", "finding": "a() is one()", "fixed": "renamed, in a fix-PASS"}]"#]));
    write(p, "src/a.py", "def one():\n    return 1\n");
    let a2 = commit(p, "fix the nit");
    notes(p, &a_tests, &[]);
    ok(cr(p, &["--verdict", "PASS", "--fixes", &a1]));
    red_only_on_8(&keel(p, &["verify-dod", "wu", "wu/e/a"]));
    let o = ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"]));
    assert!(text(&o).contains(&format!("fix-PASS of round 1 on {}", &a1[..10])) && text(&o).contains("changed: src/a.py"), "{}", text(&o));
    assert_eq!(note(p, "mr", &a2)["tip"], a2.as_str());
    merge(p, "a");

    // WU B: round 1 fails with a blocker; round 2 reads only the fix and finds importants, no blocker;
    // no round 3 runs, and a fix-PASS answers round 2.
    let (b1, b_tests) = start_wu(p, "b");
    let b_base = git(p, &["merge-base", "epic/e", "HEAD"]);
    assert!(text(&ok(preflight(p, "b"))).contains(&whole(1, 3, &b_base, &b1)));
    ok(cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
    let b2 = fix(p, "b", 2, &b_tests);
    assert!(text(&ok(preflight(p, "b"))).contains(&last_fix(2, 3, &b1, &b2)));
    ok(cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    fix(p, "b", 3, &b_tests);
    refused(&preflight(p, "b"), &["0·review-cap", "review round 3 runs only after round 2 found a blocker", &format!("--fixes {b2}")]);
    refused(&cr(p, &["--verdict", "FAIL", "--blockers", "1"]), &["round 3"]);
    ok(cr(p, &["--verdict", "PASS", "--fixes", &b2]));
    red_only_on_8(&keel(p, &["verify-dod", "wu", "wu/e/b"]));
    let o = ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"]));
    assert!(
        text(&o).contains(&format!("fix-PASS of round 2 on {}", &b2[..10])) && text(&o).contains(&format!("[round 2, since {}]", &b1[..10])),
        "{}",
        text(&o)
    );
    merge(p, "b");

    // WU C: rounds 1 and 2 find blockers; round 3 reads only the last fix and ends at the cap.
    let (c1, c_tests) = start_wu(p, "c");
    let c_base = git(p, &["merge-base", "epic/e", "HEAD"]);
    let mut tips = vec![c1];
    for n in 1..=3u32 {
        let tip = tips.last().unwrap().clone();
        let scope = if n == 1 { whole(1, 3, &c_base, &tip) } else { last_fix(n, 3, &tips[tips.len() - 2], &tip) };
        let o = ok(preflight(p, "c"));
        assert!(text(&o).contains(&scope), "round {n}: {}", text(&o));
        if n == 3 {
            let blocker = r#"[{"location": "src/c.py:2", "claim": "c() returns 3", "severity": "blocker", "evidence": "e"}]"#;
            let o = ok(cr(p, &["--verdict", "FAIL", "--findings", blocker]));
            assert!(text(&o).contains("review cap reached"), "{}", text(&o));
            break;
        }
        ok(cr(p, &["--verdict", "FAIL", "--blockers", "1"]));
        tips.push(fix(p, "c", n + 1, &c_tests));
    }
    let c3 = tips[2].clone();
    ok(cr(p, &["--verdict", "WAIVED"]));
    let cap = "review cap: 3 rounds, 1 blockers open";
    assert_eq!(note(p, "cr", &c3)["waiver"], cap);

    // Round 4 is refused: a further fix, tried off the branch.
    git(p, &["switch", "-q", "--detach"]);
    write(p, "src/c.py", "def c():\n    return 4\n");
    commit(p, "fix round 3");
    notes(p, &c_tests, &["--target", "epic/e"]);
    refused(&keel(p, &["preflight", "wu", "HEAD", "--target", "epic/e"]), &["0·review-cap", "round 4"]);
    git(p, &["switch", "-q", "wu/e/c"]);

    let o = keel(p, &["verify-dod", "wu", "wu/e/c"]);
    red_only_on_8(&o);
    assert!(text(&o).contains(&format!("WAIVED 3·review: {cap}")), "{}", text(&o));
    let o = keel(p, &["mr", "confirm", "wu", "--as", "vu"]);
    refused(&o, &[cap, "--accept-waiver", "blocker src/c.py:2: c() returns 3", "[round 3, since", "[round 1, full]"]);
    assert!(run("git", p, &["notes", "--ref=mr", "show", &c3], "").status.code() != Some(0), "a refused sign-off writes nothing");
    let accept = code(&o, "--accept-waiver");
    ok(keel(p, &["mr", "confirm", "wu", "--as", "vu", "--accept-waiver", &accept]));
    ok(keel(p, &["verify-dod", "wu", "wu/e/c"]));

    // Rewording the waiver after the sign-off is red; the signed wording is green again.
    let signed = git(p, &["notes", "--ref=cr", "show", &c3]);
    let mut n: Value = serde_json::from_str(&signed).unwrap();
    n["waiver"] = Value::from("review cap: 3 rounds, 0 blockers open");
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &n.to_string(), &c3]);
    let o = keel(p, &["verify-dod", "wu", "wu/e/c"]);
    assert!(!o.status.success() && text(&o).contains("8·manual-review") && text(&o).contains("changed after"), "{}", text(&o));
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &signed, &c3]);
    ok(keel(p, &["verify-dod", "wu", "wu/e/c"]));
    merge(p, "c");

    // A configured schedule: a bad one is refused at its commit, a good one lands on the epic.
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    assert!(cfg.lines().any(|l| l.starts_with("[review]")), "keel init writes a [review] table: {cfg}");
    let with = |keys: &str| -> String {
        cfg.lines().flat_map(|l| if l.starts_with("[review]") { vec![l, keys] } else { vec![l] }).map(|l| format!("{l}\n")).collect()
    };
    let before = git(p, &["rev-parse", "HEAD"]);
    write(p, ".keel/config.toml", &with("max_rounds = 3\nwhole_range_rounds = 4"));
    git(p, &["add", "-A"]);
    let o = run("git", p, &["commit", "-qm", "a bad schedule"], "");
    assert!(!o.status.success() && text(&o).contains("whole_range_rounds = 4") && text(&o).contains("fix ->"), "{}", text(&o));
    assert_eq!(git(p, &["rev-parse", "HEAD"]), before, "nothing landed");
    write(p, ".keel/config.toml", &with("max_rounds = 4\nwhole_range_rounds = 2\nfix_importants_from = 3"));
    commit(p, "the epic's review schedule");

    // WU D, cut after it: round 2 reads the whole range, round-2 importants earn round 3, and a round 3
    // with importants only gets no round 4 and merges on a fix-PASS.
    let (d1, d_tests) = start_wu(p, "d");
    let d_base = git(p, &["merge-base", "epic/e", "HEAD"]);
    assert!(text(&ok(preflight(p, "d"))).contains(&whole(1, 4, &d_base, &d1)));
    ok(cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    let d2 = fix(p, "d", 2, &d_tests);
    assert!(text(&ok(preflight(p, "d"))).contains(&whole(2, 4, &d_base, &d2)));
    ok(cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    let d3 = fix(p, "d", 3, &d_tests);
    assert!(text(&ok(preflight(p, "d"))).contains(&last_fix(3, 4, &d2, &d3)));
    ok(cr(p, &["--verdict", "FAIL", "--importants", "1"]));
    fix(p, "d", 4, &d_tests);
    refused(&preflight(p, "d"), &["0·review-cap", "review round 4 runs only after round 3 found a blocker"]);
    ok(cr(p, &["--verdict", "PASS", "--fixes", &d3]));
    red_only_on_8(&keel(p, &["verify-dod", "wu", "wu/e/d"]));
    let o = ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"]));
    let t = text(&o);
    assert!(t.contains("[round 2, full]") && t.contains(&format!("fix-PASS of round 3 on {}", &d3[..10])), "{t}");
    merge(p, "d");

    // The epic: its own round 1 on main's defaults (its WUs' rounds are theirs), the walk shows WU C
    // waived, then main.
    let wus = ["a", "b", "c", "d"];
    let mut archived: Vec<String> = vec!["docs/work/e-plan.html".into(), "docs/work/e-plan.yaml".into()];
    for w in wus {
        archived.push(format!("docs/work/{w}-result.html"));
        archived.push(format!("docs/work/{w}-result.yaml"));
    }
    git(p, &[&["rm", "-q"][..], &archived.iter().map(String::as_str).collect::<Vec<_>>()].concat());
    write(p, "docs/use-cases/review.html", "<p>UC-REV-3: the review schedule</p>");
    let e = commit(p, "distill and archive");
    common::doc_pair(p, "docs/.archive/e-result.html", "epic-result", "<html><p>done</p></html>");
    ok(run(
        env!("CARGO_BIN_EXE_keel"),
        p,
        &["notes", "write", "test"],
        r#"{"e2e":{"files":["tests/test_a.py","tests/test_b.py","tests/test_c.py","tests/test_d.py"],"covers":"a to d, through the schedule"}}"#,
    ));
    let mut distill: Vec<String> = vec!["docs/work/e-plan.html=docs/use-cases/review.html".into()];
    distill.extend(wus.iter().map(|w| format!("docs/work/{w}-result.html=docs/use-cases/review.html")));
    ok(keel(p, &[&["notes", "write", "doc"][..], &distill.iter().flat_map(|d| ["--distill", d.as_str()]).collect::<Vec<_>>()].concat()));
    let e_base = git(p, &["merge-base", "main", "HEAD"]);
    assert!(text(&ok(keel(p, &["preflight", "epic", "epic/e"]))).contains(&whole(1, 3, &e_base, &e)), "main sets no schedule: the defaults");
    ok(cr(p, &["--verdict", "PASS"]));
    assert_eq!(note(p, "cr", &e)["round"], 1);
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    red_only_on_8(&o);
    assert!(text(&o).contains("WAIVED 7·walk") && text(&o).contains(&format!("3·review: {cap}")), "{}", text(&o));
    ok(keel(p, &["mr", "confirm", "epic", "--as", "vu", "--report", "docs/.archive/e-result.html"]));
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge epic e", "epic/e"], ""));
    assert_eq!(note(p, "dod", "epic/e")["level"], "epic");
}
