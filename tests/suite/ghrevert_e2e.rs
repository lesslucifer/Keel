//! E-GHREVERT epic e2e: Keel back to before GitHub. A project whose main still carries a roster with
//! a github line and a policy with the github tier (as E-DOCSPLIT's do until it is on main) works
//! the pre-GitHub way: a WU signed by typing (the github id is refused, the username signs), merged
//! locally into its epic through the real merge hook; the epic closed, signed by typing, merged into
//! main and pushed to a bare local remote through the real push hook, the notes riding along. The
//! GitHub flow's commands are gone. A scratch repo in a temp dir, no network.
compiled!();

use std::path::Path;
use std::process::{Output, Stdio};
use tempfile::TempDir;

use crate::common;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    common::path_with(bin)
}

fn run(prog: &str, d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut c = common::cmd(prog)
        .args(a)
        .current_dir(d)
        .env("PATH", path_with_keel())
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

fn git(d: &Path, a: &[&str]) -> String {
    let o = run("git", d, a, "");
    assert!(o.status.success(), "git {a:?}: {}", text(&o));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn keel(d: &Path, a: &[&str], input: &str) -> Output {
    run(env!("CARGO_BIN_EXE_keel"), d, a, input)
}

fn ok(o: Output) -> Output {
    assert!(o.status.success(), "{}", text(&o));
    o
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

/// The roster E-DOCSPLIT commits: a typed line and a github line.
const ROSTER: &str = "username: t\ngithub: 5499546 lesslucifer\n";
const WU_PLAN: &str = "docs/work/2026-11-19-e2e-wu-a-plan.html";
const WU_RESULT: &str = "docs/work/2026-11-19-e2e-wu-a-result.html";
const EPIC_RESULT: &str = "docs/.archive/2026-11-19-e2e-epic-result.html";

/// A project set up by `keel init` on main, its policy accepting the username, key and github tiers
/// and its roster above, the base pushed to a bare local remote (returned to keep it alive).
fn init_project() -> (TempDir, TempDir) {
    let d = TempDir::new().unwrap();
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    write(
        p,
        ".keel/config.toml",
        "code_paths = [\"src/\", \"tests/\"]\n[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n[mr]\ntiers = [\"username\", \"key\", \"github\"]\n",
    );
    write(p, ".keel/mr-allowed", ROSTER);
    write(p, "README", "a project keel adopted\n");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "base: the policy and the roster"]);
    ok(keel(p, &["init"], ""));
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "--no-verify", "-m", "keel init"]);
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(p, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    ok(run("git", p, &["push", "-q", "origin", "main"], ""));
    (d, remote)
}

#[test]
fn ghrevert_a_typed_sign_off_a_local_merge_and_a_push_carry_an_epic_to_main() {
    let (d, remote) = init_project();
    let p = d.path();
    assert_eq!(std::fs::read_to_string(p.join(".keel/mr-allowed")).unwrap(), ROSTER, "keel init kept the roster");

    // ── 1. a WU with code, signed by typing: the github id refused, the username written ───────
    git(p, &["switch", "-q", "-c", "epic/e"]);
    git(p, &["switch", "-q", "-c", "wu/e/a"]);
    write(p, "src/a.py", "def a():\n    return 1\n");
    write(p, "tests/test_a.py", "def test_a():\n    assert 1\n");
    common::doc_pair(p, WU_PLAN, "wu-plan", "<html><p>plan</p></html>");
    common::doc_pair(p, WU_RESULT, "wu-result", "<html><p>wu a: done</p></html>");
    let wu = commit(p, "feat(e2e.W1): a returns one");
    ok(keel(p, &["notes", "write", "test"], r#"{"integration":{"files":["tests/test_a.py"],"covers":"a returns one"}}"#));
    ok(keel(p, &["notes", "write", "doc"], ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    let o = keel(p, &["mr", "confirm", "wu", "--as", "5499546"], "");
    assert!(!o.status.success() && text(&o).contains("github tier only"), "the github id signs nothing: {}", text(&o));
    assert!(!run("git", p, &["notes", "--ref=mr", "show", &wu], "").status.success(), "nothing written");
    ok(keel(p, &["mr", "confirm", "wu", "--as", "t"], ""));
    assert!(git(p, &["notes", "--ref=mr", "show", &wu]).contains("\"tier\":\"username\""));
    // the GitHub flow's commands are gone (asked on the WU, where a notes write has a range)
    for a in [&["ci", "gate", "wu", "wu/e/a"][..], &["ci", "sign-off", "wu", "wu/e/a"][..], &["notes", "write", "report", WU_RESULT][..]] {
        let o = keel(p, a, "");
        assert!(!o.status.success() && text(&o).contains("usage"), "{a:?} is unknown: {}", text(&o));
    }

    // ── 2. the local merge into the epic passes the real merge hook ───────────────────────────
    git(p, &["switch", "-q", "epic/e"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge E2E.W1: a returns one", "wu/e/a"], ""));

    // ── 3. the close: archive, epic notes, typed epic sign-off, merge into main, push ──────────
    let data = |page: &str| page.replace(".html", ".yaml");
    git(p, &["rm", "-q", WU_PLAN, &data(WU_PLAN), WU_RESULT, &data(WU_RESULT)]);
    write(p, "docs/decisions/close.html", "<p>what the epic taught</p>");
    common::doc_pair(p, EPIC_RESULT, "epic-result", "<html><p>epic e: done</p></html>");
    git(p, &["add", "docs/decisions/close.html"]);
    git(p, &["commit", "-qm", "docs(E2E close): archive the working docs, distilled to the T2 set"]);
    ok(keel(p, &["notes", "write", "test"], r#"{"e2e":{"files":["tests/test_a.py"],"covers":"the WU, the close and the merge into main"}}"#));
    let mut doc = vec!["notes", "write", "doc"];
    let distill: Vec<String> = [WU_PLAN, WU_RESULT].iter().map(|t| format!("{t}=docs/decisions/close.html")).collect();
    for x in &distill {
        doc.extend(["--distill", x.as_str()]);
    }
    ok(keel(p, &doc, ""));
    ok(keel(p, &["notes", "write", "cr", "--verdict", "PASS"], ""));
    let o = keel(p, &["mr", "confirm", "epic", "--as", "5499546", "--report", EPIC_RESULT], "");
    assert!(!o.status.success() && text(&o).contains("github tier only"), "{}", text(&o));
    ok(keel(p, &["mr", "confirm", "epic", "--as", "t", "--report", EPIC_RESULT], ""));
    let tip = git(p, &["rev-parse", "HEAD"]);
    git(p, &["switch", "-q", "main"]);
    ok(run("git", p, &["merge", "-q", "--no-ff", "-m", "Merge epic E2E", "epic/e"], ""));
    ok(run("git", p, &["push", "-q", "origin", "main"], ""));
    ok(run("git", p, &["push", "-q", "origin", "epic/e"], ""));
    assert_eq!(git(remote.path(), &["rev-parse", "main^2"]), tip, "main's second parent is the signed epic tip");
    for kind in ["test", "doc", "cr", "mr", "dod"] {
        let r = format!("refs/notes/{kind}");
        assert_eq!(git(remote.path(), &["rev-parse", &r]), git(p, &["rev-parse", &r]), "{r} rode the push");
    }
}
