use std::path::Path;
use std::process::Output;
use tempfile::TempDir;

mod common;
use common::cmd;

fn path_with_keel() -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().to_owned();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

fn run(prog: &str, d: &Path, a: &[&str]) -> Output {
    cmd(prog).args(a).current_dir(d).env("PATH", path_with_keel()).output().unwrap()
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
            "[check]\nlint = \"true\"\ntest = \"echo x >> runs.log\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
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
fn second_check_run_on_unchanged_tree_is_skipped() {
    let d = repo(true);
    let p = d.path();
    assert!(keel(p, &["check"]).status.success());
    let second = keel(p, &["check"]);
    assert!(text(&second).contains("skipped"), "{}", text(&second));
    assert_eq!(runs(p), 1);
    std::fs::write(p.join("new.txt"), "change").unwrap();
    keel(p, &["check"]);
    assert_eq!(runs(p), 2);
}

#[test]
fn commit_on_main_is_refused_but_a_branch_is_fine() {
    let d = repo(true);
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    // A no-op hook at the old in-tree path is just content now - it never runs.
    std::fs::create_dir_all(p.join(".keel/hooks")).unwrap();
    std::fs::write(p.join(".keel/hooks/pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(p.join(".keel/hooks/pre-commit"), std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .unwrap();
    std::fs::write(p.join("a"), "a").unwrap();
    git(p, &["add", "a", ".keel/hooks"]);
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
    for cmd in ["lint", "check"] {
        let o = keel(d.path(), &[cmd]);
        assert!(!o.status.success());
        assert!(text(&o).contains("keel init"), "{}", text(&o));
    }
}

fn keel_stdin(d: &Path, a: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut c = cmd(env!("CARGO_BIN_EXE_keel"))
        .args(a)
        .current_dir(d)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

/// main has one commit; branch `work` adds a python test file (2 tests) and a doc.
fn notes_fixture() -> TempDir {
    let d = repo(true);
    let p = d.path();
    std::fs::write(p.join("README"), "x").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "base"]);
    git(p, &["switch", "-q", "-c", "work"]);
    std::fs::create_dir_all(p.join("tests")).unwrap();
    std::fs::create_dir_all(p.join("docs")).unwrap();
    std::fs::write(p.join("tests/test_a.py"), "def test_one():\n    pass\n\ndef test_two():\n    pass\n").unwrap();
    std::fs::write(p.join("docs/a.html"), "<p>a</p>").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "work"]);
    d
}

fn note(p: &Path, kind: &str) -> serde_json::Value {
    let o = run("git", p, &["notes", &format!("--ref={kind}"), "show", "HEAD"]);
    assert!(o.status.success(), "{}", text(&o));
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn notes_round_trip_on_a_fixture_tip() {
    let d = notes_fixture();
    let p = d.path();
    let head = String::from_utf8(run("git", p, &["rev-parse", "HEAD"]).stdout).unwrap();
    let o = keel_stdin(
        p,
        &["notes", "write", "test"],
        r#"{"unit":{"files":["tests/test_a.py"],"covers":"a"},"e2e":{"waived":"no ui"}}"#,
    );
    assert!(o.status.success(), "{}", text(&o));
    let n = note(p, "test");
    assert_eq!(n["schema"], "test/v1");
    assert_eq!(n["tip"].as_str().unwrap(), head.trim());
    assert_eq!(n["branch"], "work");
    assert_eq!(n["unit"]["count"], 2);
    assert_eq!(n["unit"]["languages"]["python"], 2);
    assert_eq!(n["e2e"]["waived"], "no ui");

    assert!(keel(p, &["notes", "write", "doc"]).status.success());
    assert_eq!(note(p, "doc")["changes"], serde_json::json!([{"path": "docs/a.html", "tier": "T1"}]));

    assert!(keel(p, &["notes", "write", "cr", "--verdict", "PASS"]).status.success());
    assert_eq!(note(p, "cr")["verdict"], "PASS");
}

#[test]
fn claiming_a_layer_the_diff_has_no_tests_for_is_refused() {
    let d = notes_fixture();
    let p = d.path();
    for j in [
        r#"{"integration":{"files":["README"],"covers":"x"}}"#, // untouched by the range
        r#"{"integration":{"files":["docs/a.html"],"covers":"x"}}"#, // touched, declares no tests
    ] {
        let o = keel_stdin(p, &["notes", "write", "test"], j);
        assert!(!o.status.success(), "{j}");
        assert!(text(&o).contains("refused"), "{}", text(&o));
    }
    assert!(!run("git", p, &["notes", "--ref=test", "show", "HEAD"]).status.success(), "no note written");
}

#[test]
fn pass_with_blockers_is_refused() {
    let d = notes_fixture();
    let o = keel(d.path(), &["notes", "write", "cr", "--verdict", "PASS", "--blockers", "1"]);
    assert!(!o.status.success() && text(&o).contains("refused"));
}

/// cr blocker 2: a tracked file that .gitignore matches is still part of the tree check vouches for.
#[test]
fn check_cache_key_covers_a_tracked_file_that_gitignore_matches() {
    let d = repo(true);
    let p = d.path();
    std::fs::write(
        p.join(".keel/config.toml"),
        "[check]\nlint = \"true\"\ntest = \"grep -q ok conf.txt\"\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n",
    )
    .unwrap();
    std::fs::write(p.join(".gitignore"), "conf.txt\n").unwrap();
    std::fs::write(p.join("conf.txt"), "ok").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["add", "-f", "conf.txt"]);
    git(p, &["commit", "-qm", "base"]);
    assert!(keel(p, &["check"]).status.success());
    std::fs::write(p.join("conf.txt"), "bad").unwrap();
    let o = keel(p, &["check"]);
    assert!(!o.status.success() && !text(&o).contains("skipped"), "{}", text(&o));
}

#[test]
fn version_works_outside_any_repo() {
    let d = TempDir::new().unwrap();
    let o = keel(d.path(), &["--version"]);
    assert!(o.status.success());
    let t = text(&o);
    assert!(t.starts_with(&format!("keel {}\n", env!("CARGO_PKG_VERSION"))), "{t}");
    assert!(t.contains("Keel:"), "{t}");
    assert!(keel(d.path(), &["-V"]).status.success());
}

fn hooks_path(p: &Path) -> String {
    String::from_utf8(run("git", p, &["config", "--get", "core.hooksPath"]).stdout).unwrap().trim().to_string()
}

/// I8: another tool's hooks are never replaced silently; --force replaces them; samples are not hooks.
#[test]
fn hooks_install_refuses_to_clobber_another_hook_setup_without_force() {
    let d = repo(false);
    let p = d.path();
    git(p, &["config", "core.hooksPath", ".husky"]);
    let o = keel(p, &["hooks", "install"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(t.contains("core.hooksPath") && t.contains(".husky") && t.contains("keel hooks install --force"), "{t}");
    assert_eq!(hooks_path(p), ".husky", "a refused install changes nothing");
    assert!(!p.join(".git/keel-hooks").exists(), "a refused install writes nothing");
    assert!(keel(p, &["hooks", "install", "--force"]).status.success());
    assert!(hooks_path(p).ends_with("keel-hooks"), "{}", hooks_path(p));

    let d = repo(false);
    let p = d.path();
    std::fs::write(p.join(".git/hooks/pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
    let o = keel(p, &["hooks", "install"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(t.contains("pre-commit") && t.contains("--force"), "{t}");
    assert_eq!(hooks_path(p), "", "a refused install changes nothing");
    assert!(keel(p, &["hooks", "install", "--force"]).status.success());
    assert!(hooks_path(p).ends_with("keel-hooks"));

    let d = repo(false);
    let p = d.path();
    assert!(std::fs::read_dir(p.join(".git/hooks")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".sample")));
    assert!(keel(p, &["hooks", "install"]).status.success(), "only *.sample: nothing to clobber");
    let again = keel(p, &["hooks", "install"]);
    assert!(again.status.success(), "a re-install over Keel's own path: {}", text(&again));
}

/// R1 F1: `0·hooks` and `hooks install` agree on what Keel's own path is - a trailing `/` included.
#[test]
fn a_trailing_slash_on_keels_hooks_path_is_a_reinstall_not_a_foreign_setup() {
    let d = repo(false);
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    git(p, &["config", "core.hooksPath", &format!("{}/", hooks_path(p))]);
    let check = keel(p, &["check"]);
    assert!(!text(&check).contains("core.hooksPath is"), "guard D accepts it: {}", text(&check));
    let again = keel(p, &["hooks", "install"]);
    assert!(again.status.success(), "a re-install over Keel's own path: {}", text(&again));
}

#[test]
fn installed_git_hook_without_keel_on_path_refuses_and_points_to_install_channels() {
    let d = repo(false);
    let p = d.path();
    assert!(keel(p, &["hooks", "install"]).status.success());
    let hooks = String::from_utf8(run("git", p, &["config", "core.hooksPath"]).stdout).unwrap();
    let o = cmd(Path::new(hooks.trim()).join("pre-commit")).current_dir(p).env("PATH", "/usr/bin:/bin").output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("cargo install --git https://github.com/lesslucifer/Keel"), "{t}");
    assert!(t.contains("https://github.com/lesslucifer/Keel/blob/main/docs/install.html"), "{t}");
}

fn run_env(prog: &str, d: &Path, a: &[&str], envs: &[(&str, &str)], input: &str) -> Output {
    use std::io::Write;
    let mut c = cmd(prog);
    c.args(a).current_dir(d).env("PATH", path_with_keel()).envs(envs.iter().copied());
    let mut c = c
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

fn out(d: &Path, a: &[&str]) -> String {
    String::from_utf8(run("git", d, a).stdout).unwrap().trim().to_string()
}

/// Every file under `dir` (sorted relative path + bytes), so any write into it shows as a difference.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, d: &Path, acc: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, acc);
            } else {
                acc.push((p.strip_prefix(root).unwrap().display().to_string(), std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut acc = vec![];
    walk(dir, dir, &mut acc);
    acc.sort();
    acc
}

fn probe_repo(test_cmd: &str) -> TempDir {
    let d = repo(false);
    let p = d.path();
    std::fs::create_dir(p.join(".keel")).unwrap();
    std::fs::write(
        p.join(".keel/config.toml"),
        format!("[check]\nlint = \"true\"\ntest = '{test_cmd}'\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n"),
    )
    .unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "base"]);
    d
}

/// W13 regression (incident 2026-11-10): a GIT_DIR inherited from a hook never redirects keel or its
/// test command - `keel check` runs against its own checkout and the foreign repo stays byte-identical.
#[test]
fn check_under_a_foreign_git_dir_uses_its_own_repo_and_leaves_the_other_untouched() {
    let a = probe_repo(r#"git rev-parse --absolute-git-dir > "$KEEL_PROBE" && git config keel.probe ran && git add -A"#);
    let b = repo(false);
    std::fs::write(b.path().join("f"), "f").unwrap();
    git(b.path(), &["add", "f"]);
    git(b.path(), &["commit", "-qm", "b"]);
    let probe_dir = TempDir::new().unwrap();
    let probe = probe_dir.path().join("probe");
    let b_git = b.path().join(".git");
    let before = snapshot(&b_git);

    let o = run_env(
        env!("CARGO_BIN_EXE_keel"),
        a.path(),
        &["check"],
        &[
            ("GIT_DIR", b_git.to_str().unwrap()),
            ("GIT_INDEX_FILE", b_git.join("index").to_str().unwrap()),
            ("KEEL_PROBE", probe.to_str().unwrap()),
        ],
        "",
    );

    assert!(o.status.success(), "{}", text(&o));
    let a_git = out(a.path(), &["rev-parse", "--absolute-git-dir"]);
    assert_eq!(std::fs::read_to_string(&probe).unwrap().trim(), a_git);
    assert_eq!(out(a.path(), &["config", "keel.probe"]), "ran");
    assert!(Path::new(&a_git).join("keel-check-green").exists());
    assert!(before == snapshot(&b_git), "the foreign repo's .git was written to");
}

/// W13: pre-push from a linked worktree (git exports its absolute GIT_DIR) proves a non-HEAD sha in
/// the scratch worktree at that sha, not in the hook's own worktree.
#[test]
fn pre_push_from_a_linked_worktree_checks_the_pushed_sha_in_scratch() {
    let a = probe_repo(r#"git rev-parse HEAD > "$KEEL_PROBE""#);
    let p = a.path();
    git(p, &["switch", "-q", "-c", "x"]);
    std::fs::write(p.join("new"), "new").unwrap();
    git(p, &["add", "new"]);
    git(p, &["commit", "-qm", "x"]);
    let pushed = out(p, &["rev-parse", "x"]);
    git(p, &["switch", "-q", "main"]);
    let wt = TempDir::new().unwrap();
    let w = wt.path().join("w");
    git(p, &["worktree", "add", "-q", "--detach", w.to_str().unwrap(), "main"]);
    let w_git = out(&w, &["rev-parse", "--absolute-git-dir"]);
    let probe_dir = TempDir::new().unwrap();
    let probe = probe_dir.path().join("probe");

    let o = run_env(
        env!("CARGO_BIN_EXE_keel"),
        &w,
        &["hook", "pre-push", "origin"],
        &[("GIT_DIR", w_git.as_str()), ("KEEL_PROBE", probe.to_str().unwrap())],
        &format!("refs/heads/x {pushed} refs/heads/x 0000000000000000000000000000000000000000\n"),
    );

    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(&probe).unwrap().trim(), pushed);
}

#[test]
fn the_strip_lists_cover_gits_local_env_vars() {
    let o = run("git", Path::new("/"), &["rev-parse", "--local-env-vars"]);
    assert!(o.status.success(), "{}", text(&o));
    for v in String::from_utf8(o.stdout).unwrap().lines() {
        assert!(common::GIT_LOCAL_ENV.contains(&v), "tests/common GIT_LOCAL_ENV misses {v}");
    }
}

/// Every spawn under `dir` (recursively, *.rs) not built inside `no_git_env(..)` on its own line,
/// unless it runs one of `git_free` - programs that never read a repository.
fn raw_spawns(dir: &Path, git_free: &[&str]) -> Vec<String> {
    let needle = ["Command", "::new("].concat();
    let mut bad = vec![];
    for e in std::fs::read_dir(dir).unwrap() {
        let f = e.unwrap().path();
        if f.is_dir() {
            bad.extend(raw_spawns(&f, git_free));
        } else if f.extension().is_some_and(|x| x == "rs") {
            for (i, line) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
                let wrapped = |at: usize| {
                    let rest = &line[at + needle.len()..];
                    line[..at].trim_end_matches("std::process::").ends_with("no_git_env(")
                        || git_free.iter().any(|p| rest.starts_with(&format!("\"{p}\")")))
                };
                if line.match_indices(&needle).any(|(at, _)| !wrapped(at)) {
                    bad.push(format!("{}:{}: {}", f.display(), i + 1, line.trim()));
                }
            }
        }
    }
    bad
}

/// W13: a test spawns anything that can reach git through `common::cmd`, never a bare `Command`.
#[test]
fn every_test_spawn_strips_gits_env() {
    let bad = raw_spawns(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests"), &["jq", "ps", "kill"]);
    assert!(bad.is_empty(), "spawn through common::cmd:\n{}", bad.join("\n"));
}

/// W13: every process keel spawns is built inside `no_git_env(..)`; ssh-keygen never reads a repository.
#[test]
fn every_keel_spawn_strips_gits_env() {
    let bad = raw_spawns(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &["ssh-keygen"]);
    assert!(bad.is_empty(), "spawn through no_git_env:\n{}", bad.join("\n"));
}

/// W13: the test-side copy of the strip list equals keel's (a binary crate cannot be imported).
#[test]
fn the_test_strip_list_equals_keels() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    let start = src.find("const GIT_LOCAL_ENV").unwrap();
    let body = &src[start..start + src[start..].find("];").unwrap()];
    let keels: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
    assert_eq!(keels, common::GIT_LOCAL_ENV);
}
