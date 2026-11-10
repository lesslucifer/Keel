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

pub fn no_git_env(mut c: Command) -> Command {
    for k in GIT_LOCAL_ENV {
        c.env_remove(k);
    }
    c
}

/// The only way a test spawns a process that can reach git (enforced by `every_test_spawn_strips_gits_env`).
pub fn cmd(prog: impl AsRef<OsStr>) -> Command {
    no_git_env(Command::new(prog))
}
