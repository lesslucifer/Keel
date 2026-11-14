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

/// The PATH every test hands a child (E-TESTOPT.W1): the inherited one, behind a directory holding only
/// the real git when the inherited PATH's git is a launcher in front of it (macOS's xcrun shim, which
/// costs a few ms a call and, in a sandbox, prints a cache error into git's output). Keel itself is
/// unchanged: it runs whatever git its caller's PATH names.
pub fn path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| real_git_first(&inherited_path()))
}

/// `p` behind the real git's directory, when the test PATH has one: a test that runs with a PATH of its
/// own making (no keel on it, say) still runs the real git.
pub fn real_git_first(p: &str) -> String {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    test_path(p, DIR.get_or_init(|| real_git_dir(&inherited_path(), &Path::new(env!("CARGO_TARGET_TMPDIR")).join("real-git"))).as_deref())
}

/// The PATH this test program was started with; a test reads it only through here.
pub fn inherited_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

/// `dir` in front of the test PATH: keel's build directory, or a test's own shims.
pub fn path_with(dir: impl AsRef<Path>) -> String {
    test_path(path(), Some(dir.as_ref()))
}

pub fn test_path(inherited: &str, first: Option<&Path>) -> String {
    match first {
        Some(d) if inherited.is_empty() => d.display().to_string(),
        Some(d) => format!("{}:{inherited}", d.display()),
        None => inherited.to_string(),
    }
}

/// The git a git with this `--exec-path` runs from: `<prefix>/bin/git` beside `<prefix>/libexec/git-core`
/// (Xcode, the Command Line Tools, Homebrew, `/usr/lib/git-core` on Linux), else the copy most installs
/// keep inside git-core itself.
pub fn real_git(exec_path: &str, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let core = Path::new(exec_path.trim());
    if !core.is_absolute() {
        return None;
    }
    let beside = core.parent().and_then(Path::parent).map(|prefix| prefix.join("bin").join("git"));
    beside.into_iter().chain([core.join("git")]).find(|g| is_file(g))
}

/// The first `prog` along `path`.
pub fn on_path(path: &str, prog: &str, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    path.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(prog)).find(|p| is_file(p))
}

/// `dir`, now holding only a link to the real git, or None when the inherited PATH's git already is it,
/// or git cannot be found or run: then the test PATH is the inherited one. The link is made under
/// another name and renamed into place, so a parallel test program never runs a half-made link.
pub fn real_git_dir(inherited: &str, dir: &Path) -> Option<PathBuf> {
    let on_path_git = on_path(inherited, "git", |p| p.is_file())?;
    let o = no_git_env(Command::new(&on_path_git)).arg("--exec-path").env("PATH", inherited).output().ok()?;
    let real = real_git(&String::from_utf8_lossy(&o.stdout), |p| p.is_file()).filter(|_| o.status.success())?;
    if std::fs::canonicalize(&real).ok()? == std::fs::canonicalize(&on_path_git).ok()? {
        return None;
    }
    std::fs::create_dir_all(dir).ok()?;
    let staged = dir.join(format!("git.{}", std::process::id()));
    let _ = std::fs::remove_file(&staged);
    std::os::unix::fs::symlink(&real, &staged).ok()?;
    std::fs::rename(&staged, dir.join("git")).ok()?;
    Some(dir.to_path_buf())
}
