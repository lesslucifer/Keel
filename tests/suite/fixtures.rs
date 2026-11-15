//! E-TESTOPT.W4: the fixture copy. A template is built once per run and copied per test with
//! `std::fs`, so the copy must keep what git needs and name no path of its template.
compiled!();

use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use crate::common::{copy_tree, files_naming, rewrite, rewrites, sweep_templates};

fn mode(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
}

fn file(p: &Path, body: &str, mode: u32) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn the_copy_keeps_empty_directories_links_and_the_modes_git_needs() {
    let (src, dst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (s, d) = (src.path(), dst.path().join("copy"));
    std::fs::create_dir_all(s.join(".git/refs/tags")).unwrap();
    file(&s.join(".git/hooks/pre-commit"), "#!/bin/sh\nexit 0\n", 0o755);
    file(&s.join("key/id"), "secret", 0o600);
    file(&s.join("a/b/c.txt"), "c", 0o644);
    symlink("a/b/c.txt", s.join("rel")).unwrap();
    symlink(s.join("a/b/c.txt"), s.join("abs")).unwrap();
    symlink("nowhere", s.join("dangling")).unwrap();
    std::fs::set_permissions(s.join("a"), std::fs::Permissions::from_mode(0o750)).unwrap();

    copy_tree(s, &d, &rewrites(&[(s.to_path_buf(), d.clone())])).unwrap();

    assert!(d.join(".git/refs/tags").is_dir(), "an empty directory is copied: git needs refs/");
    assert_eq!(std::fs::read_dir(d.join(".git/refs/tags")).unwrap().count(), 0);
    assert_eq!(mode(&d.join(".git/hooks/pre-commit")), 0o755, "a hook stays executable");
    assert_eq!(mode(&d.join("key/id")), 0o600, "a key stays private: ssh-keygen refuses one others can read");
    assert_eq!(mode(&d.join("a")), 0o750, "a directory keeps its mode");
    assert_eq!(std::fs::read_to_string(d.join("a/b/c.txt")).unwrap(), "c");
    assert_eq!(std::fs::read_link(d.join("rel")).unwrap(), Path::new("a/b/c.txt"), "a relative link is kept as written");
    assert_eq!(std::fs::read_link(d.join("abs")).unwrap(), d.join("a/b/c.txt"), "a link into the template names the copy");
    assert_eq!(std::fs::read_link(d.join("dangling")).unwrap(), Path::new("nowhere"), "a dangling link is copied, not followed");
    assert!(std::fs::symlink_metadata(d.join("abs")).unwrap().file_type().is_symlink());
}

#[test]
fn a_file_naming_the_template_names_the_copy_and_keeps_its_mode() {
    let (src, dst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (s, d) = (src.path().join("t"), dst.path().join("copy"));
    let resolved = src.path().canonicalize().unwrap().join("t");
    file(&s.join(".git/config"), &format!("[core]\n\thooksPath = {}/.git/keel-hooks\n", resolved.display()), 0o644);
    file(&s.join("wt/.git"), &format!("gitdir: {}/.git/worktrees/wt\n", s.display()), 0o644);
    file(&s.join("run.sh"), &format!("#!/bin/sh\ncd {}\n", s.display()), 0o755);

    copy_tree(&s, &d, &rewrites(&[(s.clone(), d.clone())])).unwrap();

    let d_resolved = d.canonicalize().unwrap();
    assert_eq!(std::fs::read_to_string(d.join(".git/config")).unwrap(), format!("[core]\n\thooksPath = {}/.git/keel-hooks\n", d_resolved.display()));
    assert_eq!(std::fs::read_to_string(d.join("wt/.git")).unwrap(), format!("gitdir: {}/.git/worktrees/wt\n", d.display()));
    assert_eq!(mode(&d.join("run.sh")), 0o755, "a rewritten file keeps its mode");
    assert!(files_naming(&d, &rewrites(&[(s.clone(), PathBuf::new())])).is_empty());
    assert_eq!(std::fs::read_to_string(s.join("wt/.git")).unwrap(), format!("gitdir: {}/.git/worktrees/wt\n", s.display()), "the template is untouched");
}

#[test]
fn rewrite_replaces_every_occurrence_in_order_and_reports_no_change() {
    let r = vec![(b"/private/var/t".to_vec(), b"/private/var/c".to_vec()), (b"/var/t".to_vec(), b"/var/c".to_vec())];
    assert_eq!(rewrite(b"a /var/t/x /private/var/t/y /var/t", &r).unwrap(), b"a /var/c/x /private/var/c/y /var/c".to_vec());
    assert_eq!(rewrite(b"nothing here", &r), None);
    assert_eq!(rewrite(b"", &r), None);
    assert_eq!(rewrite(b"/var/t", &[(vec![], b"x".to_vec())]), None, "an empty pattern matches nothing");
}

#[test]
fn rewrites_put_the_resolved_form_first() {
    let (src, dst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (s, d) = (src.path().to_path_buf(), dst.path().to_path_buf());
    let r = rewrites(&[(s.clone(), d.clone())]);
    let (sc, dc) = (s.canonicalize().unwrap(), d.canonicalize().unwrap());
    if sc == s {
        assert_eq!(r, [(s.as_os_str().as_encoded_bytes().to_vec(), d.as_os_str().as_encoded_bytes().to_vec())]);
    } else {
        assert_eq!(r[0], (sc.as_os_str().as_encoded_bytes().to_vec(), dc.as_os_str().as_encoded_bytes().to_vec()));
        assert_eq!(r[1], (s.as_os_str().as_encoded_bytes().to_vec(), d.as_os_str().as_encoded_bytes().to_vec()));
    }
}

#[test]
fn a_sweep_removes_only_old_template_directories() {
    let tmp = TempDir::new().unwrap();
    let t = tmp.path();
    for n in ["keel-fixtures-old", "keel-fixtures-new", "other-old"] {
        std::fs::create_dir_all(t.join(n).join("t1")).unwrap();
    }
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 24 * 3600);
    for n in ["keel-fixtures-old", "other-old"] {
        std::fs::File::open(t.join(n)).unwrap().set_modified(past).unwrap();
    }
    sweep_templates(t, std::time::Duration::from_secs(24 * 3600));
    assert!(!t.join("keel-fixtures-old").exists(), "an earlier run's templates are removed");
    assert!(t.join("keel-fixtures-new").exists(), "a running run's templates are kept");
    assert!(t.join("other-old").exists(), "nothing else is touched");
}

/// The copy helper's own shape, every kind of recorded path at once: hooks, a linked worktree and a
/// remote, in a repo built here.
#[test]
fn a_copy_with_hooks_a_worktree_and_a_remote_is_independent() {
    use crate::common::{cmd, fixture, template};
    let git = |d: &Path, a: &[&str]| {
        let o = cmd("git")
            .args(a)
            .current_dir(d)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    let build = |t: &Path| {
        let (repo, remote) = (t.join("repo"), t.join("remote.git"));
        std::fs::create_dir_all(&repo).unwrap();
        git(t, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(&repo, &["init", "-q", "-b", "main"]);
        file(&repo.join(".git/my-hooks/pre-commit"), "#!/bin/sh\nexit 0\n", 0o755);
        git(&repo, &["config", "core.hooksPath", t.canonicalize().unwrap().join("repo/.git/my-hooks").to_str().unwrap()]);
        std::fs::write(repo.join("f"), "f").unwrap();
        git(&repo, &["add", "f"]);
        git(&repo, &["commit", "-qm", "base"]);
        git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["worktree", "add", "-q", "../wt", "-b", "w"]);
    };
    let (a, b) = (fixture("fixtures every recorded path", build), fixture("fixtures every recorded path", build));
    let t = template("fixtures every recorded path", |_| unreachable!("built above"));
    assert!(files_naming(a.path(), &rewrites(&[(t.clone(), PathBuf::new())])).is_empty());
    crate::common::assert_independent(&t.join("repo"), &a.path().join("repo"), &b.path().join("repo"));
    let (repo, wt) = (a.path().join("repo"), a.path().join("wt"));
    assert_eq!(Path::new(&git(&wt, &["rev-parse", "--show-toplevel"])).canonicalize().unwrap(), wt.canonicalize().unwrap());
    assert!(
        git(&repo, &["worktree", "list"]).contains(wt.canonicalize().unwrap().to_str().unwrap())
            || git(&repo, &["worktree", "list"]).contains(wt.to_str().unwrap())
    );
    git(&wt, &["commit", "-q", "--allow-empty", "-m", "in the worktree"]);
    assert_eq!(git(&repo, &["rev-parse", "w"]), git(&wt, &["rev-parse", "HEAD"]), "the copy's worktree belongs to the copy's repo");
    git(&repo, &["push", "-q", "origin", "w"]);
    git(&a.path().join("remote.git"), &["rev-parse", "--verify", "-q", "w"]);
    assert!(cmd("git").args(["rev-parse", "--verify", "-q", "w"]).current_dir(b.path().join("remote.git")).output().unwrap().stdout.is_empty());
    assert!(cmd("git").args(["rev-parse", "--verify", "-q", "w"]).current_dir(t.join("remote.git")).output().unwrap().stdout.is_empty());
}

/// A run removes its templates when it exits, so neither cargo test nor nextest (one process per
/// test) leaves one behind.
#[test]
fn a_test_process_leaves_no_template_behind() {
    let tmp = TempDir::new().unwrap();
    let exe = std::env::current_exe().unwrap();
    let o = crate::common::cmd(&exe)
        .args(["fixtures::a_copy_with_hooks_a_worktree_and_a_remote_is_independent", "--exact", "--test-threads=1"])
        .env("TMPDIR", tmp.path())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains("1 passed"), "{out}{}", String::from_utf8_lossy(&o.stderr));
    let left: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "the run left {left:?}");
}

/// Unit: a part whose path extends another's (`repo` and `repo.git`) names its own copy, not the
/// shorter part's copy with the rest of its name kept.
#[test]
fn a_part_whose_path_extends_another_names_its_own_copy() {
    let t = Path::new("/tmp/keel-fixtures-x/t1");
    let (repo, remote) = (PathBuf::from("/tmp/c-repo"), PathBuf::from("/tmp/c-remote"));
    let r = rewrites(&[(t.join("repo"), repo), (t.join("repo.git"), remote)]);
    let body = b"url = /tmp/keel-fixtures-x/t1/repo.git\nhooks = /tmp/keel-fixtures-x/t1/repo/.git/hooks\n";
    assert_eq!(String::from_utf8(rewrite(body, &r).unwrap()).unwrap(), "url = /tmp/c-remote\nhooks = /tmp/c-repo/.git/hooks\n");
    let r = rewrites(&[(t.join("repo.git"), PathBuf::from("/tmp/c-remote")), (t.join("repo"), PathBuf::from("/tmp/c-repo"))]);
    assert_eq!(
        String::from_utf8(rewrite(body, &r).unwrap()).unwrap(),
        "url = /tmp/c-remote\nhooks = /tmp/c-repo/.git/hooks\n",
        "the order of the parts does not matter"
    );
}

/// Unit: a read-only directory is copied with its children and keeps its mode.
#[test]
fn a_read_only_directory_is_copied_with_its_children() {
    let (src, dst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (s, d) = (src.path(), dst.path().join("copy"));
    file(&s.join("ro/sub/f"), "f", 0o444);
    let read_only = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(s.join("ro/sub"), read_only.clone()).unwrap();
    std::fs::set_permissions(s.join("ro"), read_only).unwrap();

    let copied = copy_tree(s, &d, &[]);

    for dir in [s.join("ro"), s.join("ro/sub"), d.join("ro"), d.join("ro/sub")] {
        let mode = dir.exists().then(|| mode(&dir));
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
        if dir.starts_with(&d) {
            assert_eq!(mode, Some(0o555), "{} keeps its mode", dir.display());
        }
    }
    copied.unwrap();
    assert_eq!(std::fs::read_to_string(d.join("ro/sub/f")).unwrap(), "f");
}

/// Integration (E-TESTOPT.W5): a test's git reads one global setting, `maintenance.auto = false`,
/// so a commit in a fixture starts no `git maintenance run --auto`: through `common::cmd`, through
/// the router tests' `hermetic()` layered on it, and through their `git_shim` for scripts that
/// clear git's environment.
#[test]
fn a_test_spawn_runs_no_automatic_git_maintenance() {
    use crate::common::cmd;
    use std::process::Command;
    let shim = TempDir::new().unwrap();
    crate::router::git_shim(shim.path());
    type Spawn<'a> = Box<dyn Fn() -> Command + 'a>;
    let spawns: [(&str, Spawn); 3] = [
        ("common::cmd", Box::new(|| cmd("git"))),
        (
            "router hermetic()",
            Box::new(|| {
                let mut c = cmd("git");
                crate::router::hermetic(&mut c);
                c
            }),
        ),
        ("router git_shim", Box::new(|| cmd(shim.path().join("git")))),
    ];
    for (name, spawn) in &spawns {
        let o = spawn().args(["config", "--global", "--list"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "maintenance.auto=false\n", "{name}: {}", String::from_utf8_lossy(&o.stderr));
        let (repo, trace) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let git = |a: &[&str]| {
            let o = spawn()
                .args(a)
                .current_dir(repo.path())
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .env("GIT_TRACE2", trace.path())
                .output()
                .unwrap();
            assert!(o.status.success(), "{name}: git {a:?}: {}", String::from_utf8_lossy(&o.stderr));
        };
        git(&["init", "-q"]);
        git(&["commit", "-q", "--allow-empty", "-m", "c"]);
        let traces: Vec<String> = std::fs::read_dir(trace.path()).unwrap().map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap()).collect();
        assert!(traces.iter().any(|t| t.contains(" commit -q")), "{name}: git traced nothing: {traces:?}");
        assert!(!traces.iter().any(|t| t.contains("maintenance run")), "{name}: a commit started git maintenance");
    }
}
