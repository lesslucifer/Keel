//! E-CRCAP epic e2e: the 5-round review schedule ends in a fix-PASS or an owner-accepted waiver, and
//! both merge through the real hooks of a repo `keel init` set up (UC-REV-2, UC-REV-3).
use serde_json::Value;
use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

mod common;

fn run(prog: &str, d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    let mut c = common::cmd(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
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

fn whole(n: u32, base: &str, tip: &str) -> String {
    format!("review round {n} of 5 - scope: the whole range, {base}..{tip}")
}

fn last_fix(n: u32, since: &str, tip: &str) -> String {
    format!("review round {n} of 5 - scope: the last fix, {since}..{tip}")
}

const RESULT: &str = "<html data-artifact=\"wu-result\"><p>done</p></html>";

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
    write(p, "docs/work/e-plan.html", "<html data-artifact=\"epic-plan\"><p>plan</p></html>");
    commit(p, "epic plan");

    // WU A: round 1 finds only nits; the fix gets a fix-PASS, no new round.
    git(p, &["switch", "-q", "-c", "wu/e/a"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    pass\n");
    write(p, "docs/work/a-result.html", RESULT);
    let a1 = commit(p, "wu a");
    let a_tests = r#"{"integration":{"files":["tests/test_a.py"],"covers":"a works"}}"#;
    notes(p, a_tests, &[]);
    let a_base = git(p, &["merge-base", "epic/e", "HEAD"]);
    assert!(text(&ok(keel(p, &["preflight", "wu", "wu/e/a"]))).contains(&whole(1, &a_base, &a1)));
    ok(cr(p, &["--verdict", "PASS", "--nits", r#"[{"location": "src/a.py:1", "finding": "a() is one()", "fixed": "renamed, in a fix-PASS"}]"#]));
    write(p, "src/a.py", "def one():\n    return 1\n");
    let a2 = commit(p, "fix the nit");
    notes(p, a_tests, &[]);
    ok(cr(p, &["--verdict", "PASS", "--fixes", &a1]));
    red_only_on_8(&keel(p, &["verify-dod", "wu", "wu/e/a"]));
    let o = ok(keel(p, &["mr", "confirm", "wu", "--as", "vu"]));
    assert!(text(&o).contains(&format!("fix-PASS of round 1 on {}", &a1[..10])) && text(&o).contains("changed: src/a.py"), "{}", text(&o));
    assert_eq!(note(p, "mr", &a2)["tip"], a2.as_str());
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu a", "wu/e/a"], ""));

    // WU B: five rounds; round 2's importants are never fixed without a round; round 5 ends at the cap.
    git(p, &["switch", "-q", "-c", "wu/e/b"]);
    write(p, "src/b.py", "def b():\n    return 1\n");
    write(p, "tests/test_b.py", "def test_b():\n    pass\n");
    write(p, "docs/work/b-result.html", RESULT);
    let mut tips = vec![commit(p, "wu b")];
    let b_tests = r#"{"integration":{"files":["tests/test_b.py"],"covers":"b works"}}"#;
    notes(p, b_tests, &[]);
    let b_base = git(p, &["merge-base", "epic/e", "HEAD"]);
    for n in 1..=5u32 {
        let tip = tips.last().unwrap().clone();
        let scope = if n <= 2 { whole(n, &b_base, &tip) } else { last_fix(n, &tips[tips.len() - 2], &tip) };
        let o = ok(keel(p, &["preflight", "wu", "wu/e/b"]));
        assert!(text(&o).contains(&scope), "round {n}: {}", text(&o));
        let o = match n {
            2 => ok(cr(p, &["--verdict", "FAIL", "--importants", "1"])),
            5 => ok(cr(
                p,
                &["--verdict", "FAIL", "--findings", r#"[{"location": "src/b.py:2", "claim": "b() returns 6", "severity": "blocker", "evidence": "e"}]"#],
            )),
            _ => ok(cr(p, &["--verdict", "FAIL", "--blockers", "1"])),
        };
        if n == 5 {
            assert!(text(&o).contains("review cap reached"), "{}", text(&o));
            break;
        }
        write(p, "src/b.py", &format!("def b():\n    return {}\n", n + 1));
        tips.push(commit(p, &format!("fix round {n}")));
        notes(p, b_tests, &[]);
        if n == 2 {
            refused(&cr(p, &["--verdict", "PASS", "--fixes", &tips[1]]), &["round 2", "importants"]);
        }
    }
    let b5 = tips[4].clone();
    ok(cr(p, &["--verdict", "WAIVED"]));
    let cap = "review cap: 5 rounds, 1 blockers open";
    assert_eq!(note(p, "cr", &b5)["waiver"], cap);

    // Round 6 is refused: a further fix, tried off the branch.
    git(p, &["switch", "-q", "--detach"]);
    write(p, "src/b.py", "def b():\n    return 7\n");
    commit(p, "fix round 5");
    notes(p, b_tests, &["--target", "epic/e"]);
    let o = keel(p, &["preflight", "wu", "HEAD", "--target", "epic/e"]);
    refused(&o, &["0·review-cap", "round 6"]);
    git(p, &["switch", "-q", "wu/e/b"]);

    let o = keel(p, &["verify-dod", "wu", "wu/e/b"]);
    red_only_on_8(&o);
    assert!(text(&o).contains(&format!("WAIVED 3·review: {cap}")), "{}", text(&o));
    let o = keel(p, &["mr", "confirm", "wu", "--as", "vu"]);
    refused(&o, &[cap, "--accept-waiver", "blocker src/b.py:2: b() returns 6", "[round 5, since", "[round 1, full]"]);
    assert!(run("git", p, &["notes", "--ref=mr", "show", &b5], "").status.code() != Some(0), "a refused sign-off writes nothing");
    let accept = code(&o, "--accept-waiver");
    ok(keel(p, &["mr", "confirm", "wu", "--as", "vu", "--accept-waiver", &accept]));
    ok(keel(p, &["verify-dod", "wu", "wu/e/b"]));

    // Rewording the waiver after the sign-off is red; the signed wording is green again.
    let signed = git(p, &["notes", "--ref=cr", "show", &b5]);
    let mut n: Value = serde_json::from_str(&signed).unwrap();
    n["waiver"] = Value::from("review cap: 5 rounds, 0 blockers open");
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &n.to_string(), &b5]);
    let o = keel(p, &["verify-dod", "wu", "wu/e/b"]);
    assert!(!o.status.success() && text(&o).contains("8·manual-review") && text(&o).contains("changed after"), "{}", text(&o));
    git(p, &["notes", "--ref=cr", "add", "-f", "-m", &signed, &b5]);
    ok(keel(p, &["verify-dod", "wu", "wu/e/b"]));
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "--no-ff", "-m", "merge wu b", "wu/e/b"], ""));

    // The epic: its own round 1 (its WUs' rounds are theirs), the walk shows WU B waived, then main.
    git(p, &["rm", "-q", "docs/work/e-plan.html", "docs/work/a-result.html", "docs/work/b-result.html"]);
    write(p, "docs/use-cases/review.html", "<p>UC-REV-3: the review schedule</p>");
    let e = commit(p, "distill and archive");
    write(p, "docs/.archive/e-result.html", "<html data-artifact=\"epic-result\"><p>done</p></html>");
    ok(run(
        env!("CARGO_BIN_EXE_keel"),
        p,
        &["notes", "write", "test"],
        r#"{"e2e":{"files":["tests/test_a.py","tests/test_b.py"],"covers":"a and b, through the schedule"}}"#,
    ));
    let distill: Vec<String> = ["e-plan", "a-result", "b-result"].iter().map(|f| format!("docs/work/{f}.html=docs/use-cases/review.html")).collect();
    ok(keel(p, &[&["notes", "write", "doc"][..], &distill.iter().flat_map(|d| ["--distill", d.as_str()]).collect::<Vec<_>>()].concat()));
    let e_base = git(p, &["merge-base", "main", "HEAD"]);
    assert!(text(&ok(keel(p, &["preflight", "epic", "epic/e"]))).contains(&whole(1, &e_base, &e)));
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
