//! Test-side spawn hygiene (E-SEED2.W13). Git exports an absolute GIT_DIR to its hooks from a linked
//! worktree; a test run under one must never let a fixture's git reach the real repository.
#![allow(dead_code)]

use std::ffi::OsStr;
use std::process::Command;

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
pub fn cmd(prog: impl AsRef<OsStr>) -> Command {
    no_git_env(Command::new(prog))
}
