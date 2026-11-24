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

/// The global git config of every test spawn: `maintenance.auto = false`, nothing else.
pub const GIT_CONFIG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/common/gitconfig");

/// The one home of a test spawn's git hygiene: git's local and hook variables, every GIT_TRACE* and
/// GIT_DEFAULT_HASH (inherited, or set on `c` already) are removed, and git reads the tests' own
/// global config and no system one.
pub fn no_git_env(mut c: Command) -> Command {
    for k in GIT_LOCAL_ENV.iter().chain(&GIT_HOOK_ENV) {
        c.env_remove(k);
    }
    hermetic(&mut c);
    c
}

/// Re-applies the hygiene of `no_git_env` after a test set variables of its own on `c`: a GIT_TRACE*
/// or GIT_DEFAULT_HASH (inherited or set) would change what git prints or creates, and a test-set
/// GIT_CONFIG_GLOBAL or GIT_CONFIG_SYSTEM would reach the developer's config.
pub fn hermetic(c: &mut Command) -> &mut Command {
    let set: Vec<std::ffi::OsString> = c.get_envs().map(|(k, _)| k.to_owned()).collect();
    for k in std::env::vars_os().map(|(k, _)| k).chain(set) {
        if k.to_string_lossy().starts_with("GIT_TRACE") || k == "GIT_DEFAULT_HASH" {
            c.env_remove(k);
        }
    }
    // B10: no fixture reads the developer's global or system config (a signing key, tag.gpgSign,
    // log.showSignature). A repo's own config still applies: it is part of what a test judges.
    // The tests' own global config only turns off git's automatic maintenance (E-TESTOPT.W5).
    c.env("GIT_CONFIG_GLOBAL", GIT_CONFIG).env("GIT_CONFIG_NOSYSTEM", "1")
}

/// The one identity every test commit is made as.
pub const TESTER: [(&str, &str); 4] = [("GIT_AUTHOR_NAME", "t"), ("GIT_AUTHOR_EMAIL", "t@t"), ("GIT_COMMITTER_NAME", "t"), ("GIT_COMMITTER_EMAIL", "t@t")];

/// `c`, committing as `TESTER`.
pub fn as_tester(c: &mut Command) -> &mut Command {
    c.envs(TESTER)
}

/// `<dir>/git`: the real git with `hermetic`'s config isolation, for scripts that clear every
/// inherited GIT_* (the router's watch).
pub fn git_shim(dir: &Path) {
    let real = String::from_utf8(cmd("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("git"), git_shim_script(real.trim(), GIT_CONFIG)).unwrap();
    std::fs::set_permissions(dir.join("git"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// The shim's body: `git` run with no system config and `config` as its global one.
pub fn git_shim_script(git: &str, config: &str) -> String {
    format!("#!/bin/sh\nGIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL={} exec {} \"$@\"\n", sh_quote(config), sh_quote(git))
}

/// `s` as one single-quoted shell word: each `'` becomes `'\''`.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A clone whose main holds `src`'s index - mid-merge the merged result, not HEAD - the tree the
/// build under test embeds (E-SEED2.W14/W15: never read Keel's own tree through HEAD). `--shared`
/// makes the index's objects reachable; the tree is written from a copy of the index, since
/// write-tree rewrites the index it reads and `src` may be the real checkout, shared by parallel tests.
pub fn self_clone(src: &std::path::Path, dst: &std::path::Path) {
    let git = |d: &std::path::Path, a: &[&str], index: Option<&std::path::Path>| {
        let mut c = cmd("git");
        as_tester(c.args(a).current_dir(d));
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
    let rewrites = rewrites(&map);
    for (from, to) in &map {
        copy_tree(from, to, &rewrites).unwrap();
    }
    copies
}

/// The template `key`, built on first use. A run's templates live in one directory, removed when
/// the test process exits; a run that was killed leaves its directory to a later run's sweep.
pub fn template(key: &str, build: impl FnOnce(&Path)) -> PathBuf {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    static SLOTS: OnceLock<Mutex<HashMap<String, Arc<OnceLock<PathBuf>>>>> = OnceLock::new();
    let root = TEMPLATE_ROOT.get_or_init(|| {
        sweep_templates(&std::env::temp_dir(), std::time::Duration::from_secs(24 * 3600));
        let d = tempfile::Builder::new().prefix(TEMPLATES).tempdir().unwrap().keep();
        remove_templates_at_exit();
        d
    });
    let slot = SLOTS.get_or_init(Default::default).lock().unwrap().entry(key.to_string()).or_default().clone();
    slot.get_or_init(|| {
        let d = tempfile::Builder::new().prefix("t").tempdir_in(root).unwrap().keep();
        build(&d);
        d
    })
    .clone()
}

static TEMPLATE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// libtest ends a run by returning from main or calling exit, and neither drops a static, so the
/// templates go through the C library's exit handlers.
fn remove_templates_at_exit() {
    extern "C" fn remove() {
        if let Some(d) = TEMPLATE_ROOT.get() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    extern "C" {
        fn atexit(f: extern "C" fn()) -> std::os::raw::c_int;
    }
    assert_eq!(unsafe { atexit(remove) }, 0, "atexit refused the template cleanup");
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

/// Each (template path, copy path) pair as bytes to replace, in the resolved form too, listed first:
/// a tool that canonicalises (keel hooks install) records `/private/var/…` for `/var/…` on macOS.
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
    std::fs::set_permissions(dst, std::fs::metadata(src)?.permissions())
}

/// `body` with every `from` replaced by its `to` in one pass, the longest `from` matching at a
/// position winning (parts `repo` and `repo.git`); None when no `from` occurs.
pub fn rewrite(body: &[u8], rewrites: &[(Vec<u8>, Vec<u8>)]) -> Option<Vec<u8>> {
    let mut starts = [false; 256];
    for (from, _) in rewrites.iter().filter(|(from, _)| !from.is_empty()) {
        starts[from[0] as usize] = true;
    }
    let mut out = Vec::with_capacity(body.len());
    let mut changed = false;
    let mut i = 0;
    while i < body.len() {
        let gap = body[i..].iter().position(|b| starts[*b as usize]).unwrap_or(body.len() - i);
        out.extend_from_slice(&body[i..i + gap]);
        i += gap;
        if i == body.len() {
            break;
        }
        let hit = rewrites.iter().filter(|(from, _)| !from.is_empty() && body[i..].starts_with(from)).max_by_key(|(from, _)| from.len());
        match hit {
            Some((from, to)) => {
                out.extend_from_slice(to);
                i += from.len();
                changed = true;
            }
            None => {
                out.push(body[i]);
                i += 1;
            }
        }
    }
    changed.then_some(out)
}

/// What every fixture copy holds, asserted once per shape: no file of `copy` names `template`, its
/// hooks are its own, and a commit in `copy` reaches neither `sibling` nor `template`.
pub fn assert_independent(template: &Path, copy: &Path, sibling: &Path) {
    let named = files_naming(copy, &rewrites(&[(template.to_path_buf(), template.to_path_buf())]));
    assert!(named.is_empty(), "the copy still names its template in {named:?}");
    let git = |d: &Path, a: &[&str]| {
        let o = as_tester(cmd("git").args(a).current_dir(d)).output().unwrap();
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

/// A doc as DEC-69 pairs it (E-DOCSPLIT.W3): the page `page` (a path ending `.html`, relative to
/// `dir`) holding `body`, and the data file beside it, `<stem>.yaml`, whose `artifact` is `roles`
/// (space-separated). The sign-off, the report note and the archive find a doc's role here, never on
/// the page. Returns the data file's path, relative to `dir`.
pub fn doc_pair(dir: &Path, page: &str, roles: &str, body: &str) -> String {
    let name = Path::new(page).file_name().unwrap().to_string_lossy().into_owned();
    doc_pair_data(dir, page, body, &format!("artifact: {roles}\nstatus: ready\npage: {name}\n"))
}

/// `doc_pair` with the data file's text given whole.
pub fn doc_pair_data(dir: &Path, page: &str, body: &str, data: &str) -> String {
    let stem = page.strip_suffix(".html").unwrap_or_else(|| panic!("a page ends .html: {page}"));
    let yaml = format!("{stem}.yaml");
    for (rel, text) in [(page, body), (yaml.as_str(), data)] {
        let f = dir.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    yaml
}

/// Preflight's round line for a whole-range round `n` of `of`, on `base..tip`.
pub fn whole(n: u32, of: u32, base: &str, tip: &str) -> String {
    format!("review round {n} of {of} - scope: the whole range, {base}..{tip}")
}

/// Preflight's round line for a last-fix round `n` of `of`, on `since..tip`.
pub fn last_fix(n: u32, of: u32, since: &str, tip: &str) -> String {
    format!("review round {n} of {of} - scope: the last fix, {since}..{tip}")
}

/// The commit of round `n` (1-based) of `tips`, the rounds' commits in order, and of the round
/// before it ("" for round 1).
pub fn round_pair(tips: &[String], n: u32) -> (String, String) {
    let prev = if n > 1 { tips[n as usize - 2].clone() } else { String::new() };
    (prev, tips[n as usize - 1].clone())
}

/// A review schedule as the tests drive it: the target's three `[review]` settings.
#[derive(Clone, Copy, Debug)]
pub struct Sched {
    pub max: u32,
    pub whole: u32,
    pub from: u32,
}

/// The default values: the schedule of a target that sets no key. The one place the tests write them;
/// it must equal keel's `review::Policy::DEFAULT`, which this test crate cannot reach (keel is a binary
/// crate), so a change of the defaults is that line and this one.
pub const DEFAULTS: Sched = Sched { max: 3, whole: 1, from: 2 };

impl Sched {
    /// The `[review]` table's lines that set this schedule.
    pub fn lines(&self) -> String {
        format!("max_rounds = {}\nwhole_range_rounds = {}\nfix_importants_from = {}\n", self.max, self.whole, self.from)
    }

    /// Preflight's line for round `n` on `tip`: the whole range up to `whole`, else the change since
    /// `prev`, the round before.
    pub fn announce(&self, n: u32, base: &str, prev: &str, tip: &str) -> String {
        if n <= self.whole {
            whole(n, self.max, base, tip)
        } else {
            last_fix(n, self.max, prev, tip)
        }
    }

    /// The `scope` a round-`n` note records.
    pub fn scope(&self, n: u32, prev: &str) -> String {
        if n <= self.whole {
            "full".into()
        } else {
            format!("since {prev}")
        }
    }

    /// The sign-off history's tag of round `n`.
    pub fn tag(&self, n: u32, prev: &str) -> String {
        if n <= self.whole {
            format!("[round {n}, full]")
        } else {
            format!("[round {n}, since {}]", &prev[..10])
        }
    }

    /// Whether a round follows round `n` that found `blockers`: keel's rule, below the cap and before
    /// fix_importants_from or on a blocker.
    pub fn follows(&self, n: u32, blockers: u32) -> bool {
        n < self.max && (n < self.from || blockers > 0)
    }

    /// Why no round follows round `n` when it found no blocker.
    pub fn no_round(&self, n: u32) -> String {
        if n >= self.max {
            format!("no review round {} runs: the cap is {} rounds", n + 1, self.max)
        } else {
            format!("review round {} runs only after round {n} found a blocker", n + 1)
        }
    }

    /// The cap's waiver reason.
    pub fn cap(&self, blockers: u32) -> String {
        format!("review cap: {} rounds, {blockers} blockers open", self.max)
    }
}
