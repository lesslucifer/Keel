//! E-CRLIMIT epic e2e (E-CRCAP's before it): the review schedule - the target's `[review]` settings,
//! the default schedule when it sets none - ends in a fix-PASS or an owner-accepted waiver, and both merge
//! through the real hooks of a repo `keel init` set up (UC-REV-2, UC-REV-3, UC-GATE-1).
compiled!();

use serde_json::Value;
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common::{self, round_pair, Sched, DEFAULTS};

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

const RESULT: &str = "<html><p>done</p></html>";

/// The schedule the epic commits for its later WUs: each setting one above its default, so none is a
/// default value whatever the defaults are, and a valid default schedule (no key above max_rounds)
/// gives a valid one.
const SET: Sched = Sched { max: DEFAULTS.max + 1, whole: DEFAULTS.whole + 1, from: DEFAULTS.from + 1 };

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

/// WU `name` under schedule `s`, every round finding importants and no blocker: rounds run up to
/// fix_importants_from, no round follows it, and a fix-PASS answers it; signed and merged.
fn importants_wu(p: &Path, name: &str, s: Sched) {
    let (first, tests) = start_wu(p, name);
    let base = git(p, &["merge-base", "epic/e", "HEAD"]);
    let mut tips = vec![first];
    for n in 1..=s.from {
        let (prev, tip) = round_pair(&tips, n);
        assert!(text(&ok(preflight(p, name))).contains(&s.announce(n, &base, &prev, &tip)), "{s:?} round {n}");
        ok(cr(p, &["--verdict", "FAIL", "--importants", "1"]));
        tips.push(fix(p, name, n + 1, &tests));
        if n < s.from {
            refused(&cr(p, &["--verdict", "PASS", "--fixes", &tip]), &[&format!("round {n}"), "importants"]);
        }
    }
    let (prev, rf) = round_pair(&tips, s.from);
    refused(&preflight(p, name), &["0·review-cap", &s.no_round(s.from), &format!("--fixes {rf}")]);
    refused(&cr(p, &["--verdict", "FAIL", "--blockers", "1"]), &[&format!("round {}", s.from + 1)]);
    ok(cr(p, &["--verdict", "PASS", "--fixes", &rf]));
    red_only_on_8(&keel(p, &["verify-dod", "wu", &format!("wu/e/{name}")]));
    let t = text(&ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"])));
    assert!(t.contains(&format!("fix-PASS of round {} on {}", s.from, &rf[..10])) && t.contains(&s.tag(s.from, &prev)), "{s:?}: {t}");
    merge(p, name);
}

/// WU `name` under schedule `s`, every round finding a blocker: rounds run up to max_rounds and the
/// last ends in the cap's waiver, which the owner accepts; signed and merged. Returns the waiver.
fn blockers_wu(p: &Path, name: &str, s: Sched) -> String {
    let (first, tests) = start_wu(p, name);
    let base = git(p, &["merge-base", "epic/e", "HEAD"]);
    let blocker = format!(r#"[{{"location": "src/{name}.py:2", "claim": "{name}() is wrong", "severity": "blocker", "evidence": "e"}}]"#);
    let mut tips = vec![first];
    for n in 1..=s.max {
        let (prev, tip) = round_pair(&tips, n);
        assert!(text(&ok(preflight(p, name))).contains(&s.announce(n, &base, &prev, &tip)), "{s:?} round {n}");
        let o = ok(cr(p, &["--verdict", "FAIL", "--findings", &blocker]));
        if n == s.max {
            assert!(text(&o).contains("review cap reached") && text(&o).contains(&s.cap(1)), "{}", text(&o));
        } else {
            tips.push(fix(p, name, n + 1, &tests));
        }
    }
    let last = tips[s.max as usize - 1].clone();
    ok(cr(p, &["--verdict", "WAIVED"]));
    let cap = s.cap(1);
    assert_eq!(note(p, "cr", &last)["waiver"], cap.as_str());

    // No round past the cap: a further fix, tried off the branch.
    let branch = format!("wu/e/{name}");
    git(p, &["switch", "-q", "--detach"]);
    write(p, &format!("src/{name}.py"), &format!("def {name}():\n    return 0\n"));
    commit(p, "a fix past the cap");
    notes(p, &tests, &["--target", "epic/e"]);
    refused(&keel(p, &["preflight", "wu", "HEAD", "--target", "epic/e"]), &["0·review-cap", &s.no_round(s.max)]);
    git(p, &["switch", "-q", &branch]);

    let o = keel(p, &["verify-dod", "wu", &branch]);
    red_only_on_8(&o);
    assert!(text(&o).contains(&format!("WAIVED 3·review: {cap}")), "{}", text(&o));
    let o = keel(p, &["mr", "confirm", "wu", "--as", "vu"]);
    let mut needles = vec![cap.clone(), "--accept-waiver".into(), format!("blocker src/{name}.py:2: {name}() is wrong")];
    needles.extend((1..=s.max).map(|n| s.tag(n, &round_pair(&tips, n).0)));
    refused(&o, &needles.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(run("git", p, &["notes", "--ref=mr", "show", &last], "").status.code() != Some(0), "a refused sign-off writes nothing");
    let accept = code(&o, "--accept-waiver");
    ok(keel(p, &["mr", "confirm", "wu", "--as", "vu", "--accept-waiver", &accept]));
    ok(keel(p, &["verify-dod", "wu", &branch]));

    // Rewording the waiver after the sign-off is red; the signed wording is green again.
    let signed = git(p, &["notes", "--ref=cr", "show", &last]);
    let mut n: Value = serde_json::from_str(&signed).unwrap();
    n["waiver"] = Value::from(s.cap(0));
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &n.to_string(), &last]);
    let o = keel(p, &["verify-dod", "wu", &branch]);
    assert!(!o.status.success() && text(&o).contains("8·manual-review") && text(&o).contains("changed after"), "{}", text(&o));
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &signed, &last]);
    ok(keel(p, &["verify-dod", "wu", &branch]));
    merge(p, name);
    cap
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

    // The default values (no schedule key). WU A - round 1 finds only nits; the fix gets a fix-PASS.
    let (a1, a_tests) = start_wu(p, "a");
    let a_base = git(p, &["merge-base", "epic/e", "HEAD"]);
    assert!(text(&ok(preflight(p, "a"))).contains(&DEFAULTS.announce(1, &a_base, "", &a1)));
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
    // WU B: importants only; WU C: blockers to the cap.
    importants_wu(p, "b", DEFAULTS);
    let cap_c = blockers_wu(p, "c", DEFAULTS);

    // A schedule the epic sets: a bad one is refused at its commit, a good one lands.
    let cfg = std::fs::read_to_string(p.join(".keel/config.toml")).unwrap();
    assert!(cfg.lines().any(|l| l.starts_with("[review]")), "keel init writes a [review] table: {cfg}");
    let with = |keys: &str| -> String {
        cfg.lines().flat_map(|l| if l.starts_with("[review]") { vec![l, keys] } else { vec![l] }).map(|l| format!("{l}\n")).collect()
    };
    let before = git(p, &["rev-parse", "HEAD"]);
    let bad = format!("max_rounds = {}\nwhole_range_rounds = {}", SET.max, SET.max + 1);
    write(p, ".keel/config.toml", &with(&bad));
    git(p, &["add", "-A"]);
    let o = run("git", p, &["commit", "-qm", "a bad schedule"], "");
    assert!(!o.status.success() && text(&o).contains(&format!("whole_range_rounds = {}", SET.max + 1)) && text(&o).contains("fix ->"), "{}", text(&o));
    assert_eq!(git(p, &["rev-parse", "HEAD"]), before, "nothing landed");
    write(p, ".keel/config.toml", &with(SET.lines().trim_end()));
    commit(p, "the epic's review schedule");
    // WUs D and E, cut after it, follow it.
    importants_wu(p, "d", SET);
    let cap_e = blockers_wu(p, "e", SET);

    // The epic: its own round 1 on main's default values (its WUs' rounds are theirs), the walk shows
    // WUs C and E waived, then main.
    let wus = ["a", "b", "c", "d", "e"];
    let mut archived: Vec<String> = vec!["docs/work/e-plan.html".into(), "docs/work/e-plan.yaml".into()];
    for w in wus {
        archived.push(format!("docs/work/{w}-result.html"));
        archived.push(format!("docs/work/{w}-result.yaml"));
    }
    git(p, &[&["rm", "-q"][..], &archived.iter().map(String::as_str).collect::<Vec<_>>()].concat());
    write(p, "docs/use-cases/review.html", "<p>UC-REV-3: the review schedule</p>");
    let e = commit(p, "distill and archive");
    common::doc_pair(p, "docs/.archive/e-result.html", "epic-result", "<html><p>done</p></html>");
    let e2e_files: Vec<String> = wus.iter().map(|w| format!("\"tests/test_{w}.py\"")).collect();
    ok(run(
        env!("CARGO_BIN_EXE_keel"),
        p,
        &["notes", "write", "test"],
        &format!(r#"{{"e2e":{{"files":[{}],"covers":"a to e, through the schedule"}}}}"#, e2e_files.join(",")),
    ));
    let mut distill: Vec<String> = vec!["docs/work/e-plan.html=docs/use-cases/review.html".into()];
    distill.extend(wus.iter().map(|w| format!("docs/work/{w}-result.html=docs/use-cases/review.html")));
    ok(keel(p, &[&["notes", "write", "doc"][..], &distill.iter().flat_map(|d| ["--distill", d.as_str()]).collect::<Vec<_>>()].concat()));
    let e_base = git(p, &["merge-base", "main", "HEAD"]);
    assert!(text(&ok(keel(p, &["preflight", "epic", "epic/e"]))).contains(&DEFAULTS.announce(1, &e_base, "", &e)), "main sets no schedule");
    ok(cr(p, &["--verdict", "PASS"]));
    assert_eq!(note(p, "cr", &e)["round"], 1);
    let o = keel(p, &["verify-dod", "epic", "epic/e"]);
    red_only_on_8(&o);
    let t = text(&o);
    assert!(t.contains("WAIVED 7·walk") && t.contains(&format!("3·review: {cap_c}")) && t.contains(&format!("3·review: {cap_e}")), "{t}");
    ok(keel(p, &["mr", "confirm", "epic", "--as", "vu", "--report", "docs/.archive/e-result.html"]));
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge epic e", "epic/e"], ""));
    assert_eq!(note(p, "dod", "epic/e")["level"], "epic");
}
