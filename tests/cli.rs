use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

fn run(prog: &str, d: &Path, a: &[&str]) -> Output {
    Command::new(prog).args(a).current_dir(d).env("PATH", path_with_keel()).output().unwrap()
}

fn git(d: &Path, a: &[&str]) {
    assert!(run("git", d, a).status.success(), "git {a:?}");
}

fn keel(d: &Path, a: &[&str]) -> Output {
    run(env!("CARGO_BIN_EXE_keel"), d, a)
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn repo(with_config: bool) -> TempDir {
    let d = TempDir::new().unwrap();
    let p = d.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "t@t"]);
    git(p, &["config", "user.name", "t"]);
    if with_config {
        std::fs::create_dir(p.join(".keel")).unwrap();
        // the test command appends a line to runs.log so we can count real runs
        std::fs::write(
            p.join(".keel/config.toml"),
            "[gate0]\nlint = \"true\"\ntest = \"echo x >> runs.log\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
        )
        .unwrap();
        std::fs::write(p.join(".gitignore"), "runs.log\n").unwrap();
    }
    d
}

fn runs(p: &Path) -> usize {
    std::fs::read_to_string(p.join("runs.log")).unwrap().lines().count()
}

#[test]
fn second_gate0_run_on_unchanged_tree_is_skipped() {
    let d = repo(true);
    let p = d.path();
    assert!(keel(p, &["gate0"]).status.success());
    let second = keel(p, &["gate0"]);
    assert!(text(&second).contains("skipped"), "{}", text(&second));
    assert_eq!(runs(p), 1);
    std::fs::write(p.join("new.txt"), "change").unwrap();
    keel(p, &["gate0"]);
    assert_eq!(runs(p), 2);
}

#[test]
fn commit_on_main_is_refused_but_a_branch_is_fine() {
    let d = repo(true);
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    std::fs::write(p.join("a"), "a").unwrap();
    git(p, &["add", "a"]);
    let c = run("git", p, &["commit", "-qm", "x"]);
    assert!(!c.status.success());
    assert!(text(&c).contains("merge-only"), "{}", text(&c));
    git(p, &["switch", "-q", "-c", "work"]);
    assert!(run("git", p, &["commit", "-qm", "x"]).status.success());
}

#[test]
fn new_worktree_is_seeded_from_gitworktreecopy() {
    let d = repo(true);
    let p = d.path();
    std::fs::write(p.join(".gitworktreecopy"), "# comment\n\nsecret.env\nnested/trust.txt\n").unwrap();
    std::fs::write(p.join("secret.env"), "TOKEN=abc").unwrap();
    std::fs::create_dir(p.join("nested")).unwrap();
    std::fs::write(p.join("nested/trust.txt"), "trusted").unwrap();
    std::fs::write(p.join("a"), "a").unwrap();
    git(p, &["add", "a"]);
    git(p, &["commit", "-qm", "base"]);
    assert!(keel(p, &["hooks", "install"]).status.success());

    let w = TempDir::new().unwrap();
    assert!(run("git", p, &["worktree", "add", "-q", "-b", "feature", w.path().to_str().unwrap()])
        .status
        .success());

    assert_eq!(std::fs::read_to_string(w.path().join("secret.env")).unwrap(), "TOKEN=abc");
    assert_eq!(std::fs::read_to_string(w.path().join("nested/trust.txt")).unwrap(), "trusted");
    assert_eq!(
        run("git", w.path(), &["config", "--local", "--get", "merge.ff"]).stdout.as_slice(),
        b"false\n"
    );
}

#[test]
fn missing_config_points_at_init() {
    let d = repo(false);
    for cmd in ["lint", "gate0"] {
        let o = keel(d.path(), &[cmd]);
        assert!(!o.status.success());
        assert!(text(&o).contains("keel init"), "{}", text(&o));
    }
}
