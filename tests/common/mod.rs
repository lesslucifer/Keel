//! Test-side spawn hygiene (E-SEED2.W13). Git exports an absolute GIT_DIR to its hooks from a linked
//! worktree; a test run under one must never let a fixture's git reach the real repository.
#![allow(dead_code)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// `git rev-parse --local-env-vars` (git 2.x); `the_strip_lists_cover_gits_local_env_vars` guards drift.
pub const GIT_LOCAL_ENV: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// What git exports to its hooks about the running command (E-SEED2.W15); equals keel's GIT_HOOK_ENV.
pub const GIT_HOOK_ENV: [&str; 8] = [
    "GIT_REFLOG_ACTION",
    "GIT_EDITOR",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
];

pub fn no_git_env(mut c: Command) -> Command {
    for k in GIT_LOCAL_ENV.iter().chain(&GIT_HOOK_ENV) {
        c.env_remove(k);
    }
    // B10: no fixture reads the developer's global or system config (a signing key, tag.gpgSign,
    // log.showSignature). A repo's own config still applies: it is part of what a test judges.
    c.env("GIT_CONFIG_GLOBAL", "/dev/null");
    c.env("GIT_CONFIG_NOSYSTEM", "1");
    c
}

/// A clone whose main holds `src`'s index - mid-merge the merged result, not HEAD - the tree the
/// build under test embeds (E-SEED2.W14/W15: never read Keel's own tree through HEAD). `--shared`
/// makes the index's objects reachable; the tree is written from a copy of the index, since
/// write-tree rewrites the index it reads and `src` may be the real checkout, shared by parallel tests.
pub fn self_clone(src: &std::path::Path, dst: &std::path::Path) {
    let git = |d: &std::path::Path, a: &[&str], index: Option<&std::path::Path>| {
        let mut c = cmd("git");
        c.args(a).current_dir(d).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t");
        if let Some(i) = index {
            c.env("GIT_INDEX_FILE", i);
        }
        let o = c.output().unwrap();
        assert!(o.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(src, &["clone", "-q", "--shared", src.to_str().unwrap(), dst.to_str().unwrap()], None);
    let copy = dst.with_extension("index");
    std::fs::copy(git(src, &["rev-parse", "--path-format=absolute", "--git-path", "index"], None), &copy).unwrap();
    let tree = git(src, &["write-tree"], Some(&copy));
    git(dst, &["checkout", "-q", "-B", "main"], None);
    git(dst, &["read-tree", "-u", "--reset", &tree], None);
    git(dst, &["commit", "-q", "--no-verify", "--allow-empty", "-m", "source index"], None);
}

/// The only way a test spawns a process that can reach git (enforced by `every_test_spawn_strips_gits_env`).
/// It runs on the test PATH, so a git it reaches is the real one (`every_test_path_comes_from_common`).
pub fn cmd(prog: impl AsRef<OsStr>) -> Command {
    let mut c = no_git_env(Command::new(prog));
    c.env("PATH", path());
    c
}

/// The PATH every test hands a child (E-TESTOPT.W1): the inherited one, behind its git's own git-core
/// directory - the exec-path git itself puts first for its hooks and subcommands. There `git` runs from
/// inside its installation, so it finds its own prefix (a link elsewhere falls back to a built-in
/// prefix, another install's: round 1), and the xcrun launcher in front of it (macOS's `/usr/bin/git`,
/// which costs a few ms a call and, in a sandbox, prints a cache error into git's output) never runs.
/// Nothing is written: the directory is the installation's own. Keel itself is unchanged: it runs
/// whatever git its caller's PATH names.
pub fn path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| test_path_for(&inherited_path()))
}

/// The test PATH built from `inherited`.
pub fn test_path_for(inherited: &str) -> String {
    test_path(inherited, git_dir(inherited).as_deref())
}

/// `dir` in front of the test PATH: keel's build directory, or a test's own shims.
pub fn path_with(dir: impl AsRef<Path>) -> String {
    test_path(path(), Some(dir.as_ref()))
}

/// `p` behind the git directory: a test that runs with a PATH of its own making (no keel on it, say)
/// still runs the same git as every other test.
pub fn real_git_first(p: &str) -> String {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    test_path(p, DIR.get_or_init(|| git_dir(&inherited_path())).as_deref())
}

/// The PATH this test program was started with; a test reads it only through here.
pub fn inherited_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

pub fn test_path(inherited: &str, first: Option<&Path>) -> String {
    match first {
        Some(d) if inherited.is_empty() => d.display().to_string(),
        Some(d) => format!("{}:{inherited}", d.display()),
        None => inherited.to_string(),
    }
}

/// The git-core directory of the git `inherited` names, or None when there is no git, it fails, or its
/// exec-path holds no `git`: then the test PATH is the inherited one.
pub fn git_dir(inherited: &str) -> Option<PathBuf> {
    let git = on_path(inherited, "git", |p| p.is_file())?;
    let o = no_git_env(Command::new(git)).arg("--exec-path").env("PATH", inherited).output().ok()?;
    git_core(&String::from_utf8_lossy(&o.stdout), |p| p.is_file()).filter(|_| o.status.success())
}

/// A git's `--exec-path`, when it is absolute and holds `git` (Xcode, the Command Line Tools, Homebrew,
/// `/usr/lib/git-core` and `/usr/libexec/git-core` on Linux).
pub fn git_core(exec_path: &str, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let core = Path::new(exec_path.trim());
    (core.is_absolute() && is_file(&core.join("git"))).then(|| core.to_path_buf())
}

/// The first `prog` along `path`.
pub fn on_path(path: &str, prog: &str, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    path.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(prog)).find(|p| is_file(p))
}

/// A fresh copy of the fixture `key` (E-TESTOPT.W4): the first caller in a run builds its template
/// once, with `build` into an empty directory; every caller gets a copy of its own, made in-process
/// (a copy that spawned would put back part of the cost it saves).
pub fn fixture(key: &str, build: impl FnOnce(&Path)) -> tempfile::TempDir {
    fixture_parts(key, &["."], build).pop().unwrap()
}

/// `fixture` for a template of several directories (a repo and its remote, a repo and its key):
/// `build` fills `parts` under the template, and each part is copied to a directory of its own.
/// A path to any part, wherever the template records it (core.hooksPath, a remote, a worktree's
/// gitdir, a signing key), names that part's copy.
pub fn fixture_parts(key: &str, parts: &[&str], build: impl FnOnce(&Path)) -> Vec<tempfile::TempDir> {
    let t = template(key, build);
    let copies: Vec<tempfile::TempDir> = parts.iter().map(|_| tempfile::TempDir::new().unwrap()).collect();
    let map: Vec<(PathBuf, PathBuf)> = parts.iter().zip(&copies).map(|(p, c)| (t.join(p), c.path().to_path_buf())).collect();
    for (from, to) in &map {
        copy_tree(from, to, &rewrites(&map)).unwrap();
    }
    copies
}

/// The template `key`, built on first use. Templates live for the run in one directory; runs older
/// than a day are removed when a run starts, since a test process never drops its statics.
pub fn template(key: &str, build: impl FnOnce(&Path)) -> PathBuf {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    static ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
    static SLOTS: OnceLock<Mutex<HashMap<String, Arc<OnceLock<PathBuf>>>>> = OnceLock::new();
    let root = ROOT.get_or_init(|| {
        sweep_templates(&std::env::temp_dir(), std::time::Duration::from_secs(24 * 3600));
        tempfile::Builder::new().prefix(TEMPLATES).tempdir().unwrap()
    });
    let slot = SLOTS.get_or_init(Default::default).lock().unwrap().entry(key.to_string()).or_default().clone();
    slot.get_or_init(|| {
        let d = tempfile::Builder::new().prefix("t").tempdir_in(root.path()).unwrap().keep();
        build(&d);
        d
    })
    .clone()
}

const TEMPLATES: &str = "keel-fixtures-";

/// Removes the template directories of earlier runs that are older than `age`.
pub fn sweep_templates(tmp: &Path, age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(tmp) else { return };
    for e in entries.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|a| a > age));
        if old && e.file_name().to_string_lossy().starts_with(TEMPLATES) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Each (template path, copy path) pair as bytes to replace, the resolved form first: a tool that
/// canonicalises (keel hooks install) records `/private/var/…` for `/var/…` on macOS.
pub fn rewrites(map: &[(PathBuf, PathBuf)]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let bytes = |p: &Path| p.as_os_str().as_encoded_bytes().to_vec();
    let mut r = vec![];
    for (from, to) in map {
        let (from, to) = (from.components().collect::<PathBuf>(), to.components().collect::<PathBuf>());
        if let (Ok(cf), Ok(ct)) = (from.canonicalize(), to.canonicalize()) {
            if cf != from {
                r.push((bytes(&cf), bytes(&ct)));
            }
        }
        r.push((bytes(&from), bytes(&to)));
    }
    r
}

/// Copies the tree `src` into `dst` (created if missing) with `std::fs` only: directories (empty ones
/// too: git needs `refs/`), files with their modes (an executable hook, a 0600 key), symlinks as
/// links. Every file or link target holding a `from` of `rewrites` gets its `to` instead.
pub fn copy_tree(src: &Path, dst: &Path, rewrites: &[(Vec<u8>, Vec<u8>)]) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    std::fs::create_dir_all(dst)?;
    std::fs::set_permissions(dst, std::fs::metadata(src)?.permissions())?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let (from, to) = (e.path(), dst.join(e.file_name()));
        let kind = e.file_type()?;
        if kind.is_dir() {
            copy_tree(&from, &to, rewrites)?;
        } else if kind.is_symlink() {
            let target = std::fs::read_link(&from)?;
            let target = rewrite(target.as_os_str().as_bytes(), rewrites).unwrap_or_else(|| target.as_os_str().as_bytes().to_vec());
            std::os::unix::fs::symlink(OsStr::from_bytes(&target), &to)?;
        } else {
            let body = std::fs::read(&from)?;
            match rewrite(&body, rewrites) {
                Some(new) => {
                    std::fs::write(&to, new)?;
                    std::fs::set_permissions(&to, e.metadata()?.permissions())?;
                }
                None => {
                    std::fs::copy(&from, &to)?;
                }
            }
        }
    }
    Ok(())
}

/// `body` with every `from` replaced by its `to`, in order; None when no `from` occurs.
pub fn rewrite(body: &[u8], rewrites: &[(Vec<u8>, Vec<u8>)]) -> Option<Vec<u8>> {
    let mut out = body.to_vec();
    let mut changed = false;
    for (from, to) in rewrites {
        if from.is_empty() {
            continue;
        }
        let mut next = Vec::with_capacity(out.len());
        let mut i = 0;
        while i < out.len() {
            if out[i..].starts_with(from) {
                next.extend_from_slice(to);
                i += from.len();
                changed = true;
            } else {
                next.push(out[i]);
                i += 1;
            }
        }
        out = next;
    }
    changed.then_some(out)
}

/// What every fixture copy holds, asserted once per shape: no file of `copy` names `template`, its
/// hooks are its own, and a commit in `copy` reaches neither `sibling` nor `template`.
pub fn assert_independent(template: &Path, copy: &Path, sibling: &Path) {
    let named = files_naming(copy, &rewrites(&[(template.to_path_buf(), PathBuf::new())]));
    assert!(named.is_empty(), "the copy still names its template in {named:?}");
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
        (o.status.success(), String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    if let (true, hooks) = git(copy, &["config", "--get", "core.hooksPath"]) {
        assert!(Path::new(&hooks).starts_with(copy.canonicalize().unwrap()), "core.hooksPath {hooks} is not the copy's own");
    }
    let (sibling_head, template_head) = (git(sibling, &["rev-parse", "HEAD"]), git(template, &["rev-parse", "HEAD"]));
    std::fs::write(copy.join("independence-probe"), "x").unwrap();
    assert!(git(copy, &["add", "independence-probe"]).0);
    assert!(git(copy, &["commit", "-q", "--no-verify", "-m", "probe"]).0);
    assert_ne!(git(copy, &["rev-parse", "HEAD"]), sibling_head, "the commit did not land in the copy");
    assert_eq!(git(sibling, &["rev-parse", "HEAD"]), sibling_head, "a commit in one copy moved another");
    assert_eq!(git(template, &["rev-parse", "HEAD"]), template_head, "a commit in a copy moved the template");
    assert!(!sibling.join("independence-probe").exists() && !template.join("independence-probe").exists());
}

/// Every file under `dir` (links not followed) whose bytes hold a `from` of `rewrites`.
pub fn files_naming(dir: &Path, rewrites: &[(Vec<u8>, Vec<u8>)]) -> Vec<PathBuf> {
    let mut found = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let (p, kind) = (e.path(), e.file_type().unwrap());
        let body = if kind.is_dir() {
            found.extend(files_naming(&p, rewrites));
            continue;
        } else if kind.is_symlink() {
            std::fs::read_link(&p).unwrap().as_os_str().as_encoded_bytes().to_vec()
        } else {
            std::fs::read(&p).unwrap()
        };
        if rewrite(&body, rewrites).is_some() {
            found.push(p);
        }
    }
    found
}
