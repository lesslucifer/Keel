mod gate;
mod mr;
mod notes;
mod verify_doc;

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HOOKS_DIR: &str = ".keel/hooks";

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    code_paths: Vec<String>,
    #[serde(default)]
    rule_bearing: Vec<String>,
    gate0: Gate0,
    branches: Branches,
    docs: Docs,
    #[serde(default)]
    tests: TestsCfg,
    #[serde(default)]
    mr: MrCfg,
}
/// Signer tiers this project accepts from .mr-allowed (Q10: configurable, Trawler's three by default).
#[derive(Deserialize)]
struct MrCfg {
    tiers: Vec<String>,
}
impl Default for MrCfg {
    fn default() -> Self {
        MrCfg { tiers: mr::KNOWN_TIERS.iter().map(|t| t.to_string()).collect() }
    }
}
#[derive(Deserialize, Default)]
struct TestsCfg {
    adapter: Option<String>,
    pattern: Option<String>,
}
#[derive(Deserialize)]
struct Gate0 {
    lint: String,
    test: String,
}
#[derive(Deserialize)]
struct Branches {
    main: String,
}
#[derive(Deserialize)]
struct Docs {
    root: String,
}

fn load_config(repo: &Path) -> Result<Config, String> {
    let path = repo.join(".keel/config.toml");
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "keel: no .keel/config.toml in {}\n  run `keel init` (coming soon) to create one",
            repo.display()
        )
    })?;
    toml::from_str(&text).map_err(|e| format!("keel: bad .keel/config.toml: {e}"))
}

/// The merge gate's policy: `.keel/config.toml` as committed at HEAD, never the working tree.
/// pre-merge-commit runs with the merge already in the tree and index but before the merge commit
/// exists, so HEAD is still the target's tip - the incoming branch cannot rewrite the policy that
/// judges its own merge (e.g. rename `branches.main` to no-op the gate, or drop `key` from `mr.tiers`).
fn load_config_at_head(repo: &Path) -> Result<Config, String> {
    let text = git(repo, &["show", "HEAD:.keel/config.toml"], &[]).map_err(|_| {
        "keel: merge refused - the target (HEAD) has no committed .keel/config.toml, so there is no policy to judge this merge\n  fix -> land .keel/config.toml on the target first, outside the gated merge".to_string()
    })?;
    toml::from_str(&text).map_err(|e| format!("keel: bad .keel/config.toml at HEAD: {e}"))
}

fn git(repo: &Path, args: &[&str], envs: &[(&str, &Path)]) -> Result<String, String> {
    let mut c = Command::new("git");
    c.args(args).current_dir(repo);
    for (k, v) in envs {
        c.env(k, v);
    }
    let o = c.output().map_err(|e| format!("keel: cannot run git: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "keel: git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn repo_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    git(&cwd, &["rev-parse", "--show-toplevel"], &[])
        .map(PathBuf::from)
        .map_err(|_| "keel: not inside a git repository".into())
}

fn sh(repo: &Path, cmd: &str) -> Result<bool, String> {
    Command::new("sh")
        .args(["-c", cmd])
        .current_dir(repo)
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .map_err(|e| format!("keel: cannot run `{cmd}`: {e}"))
}

fn git_dir(repo: &Path) -> Result<PathBuf, String> {
    git(repo, &["rev-parse", "--absolute-git-dir"], &[]).map(PathBuf::from)
}

/// Git tree hash of the working tree (tracked + untracked, minus ignored),
/// built in a throwaway index so the real index is untouched.
fn tree_key(repo: &Path) -> Result<String, String> {
    let idx = git_dir(repo)?.join("keel-tmp-index");
    let _ = std::fs::remove_file(&idx);
    let e = [("GIT_INDEX_FILE", idx.as_path())];
    git(repo, &["add", "-A"], &e)?;
    let tree = git(repo, &["write-tree"], &e);
    let _ = std::fs::remove_file(&idx);
    tree
}

fn lint(repo: &Path, cfg: &Config) -> Result<i32, String> {
    Ok(if sh(repo, &cfg.gate0.lint)? {
        0
    } else {
        eprintln!("keel lint: red");
        1
    })
}

fn gate0(repo: &Path, cfg: &Config) -> Result<i32, String> {
    // The cache vouches for a tree + the exact commands, never a bare verdict.
    // Lives in the git dir: per-checkout, never synced, never tracked.
    let key = format!("{}\n{}\n{}", tree_key(repo)?, cfg.gate0.lint, cfg.gate0.test);
    let cache = git_dir(repo)?.join("keel-gate0-green");
    if std::fs::read_to_string(&cache).ok().as_deref() == Some(key.as_str()) {
        println!("gate0 skipped - tree unchanged since last green");
        return Ok(0);
    }
    if sh(repo, &cfg.gate0.lint)? && sh(repo, &cfg.gate0.test)? {
        std::fs::write(&cache, key).map_err(|e| format!("keel: write cache: {e}"))?;
        println!("gate0 green - cached");
        Ok(0)
    } else {
        eprintln!("gate0 red - lint/tests failed; not cached");
        Ok(1)
    }
}

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

/// pre-push gate0: every pushed tree proven - in place when it is the clean HEAD's
/// tree, else in a scratch worktree at that sha. `refspecs` is git's pre-push stdin.
fn gate0_push(repo: &Path, cfg: &Config, refspecs: &str) -> Result<i32, String> {
    let clean = git(repo, &["status", "--porcelain"], &[])?.is_empty();
    let head_tree = if clean { git(repo, &["rev-parse", "HEAD^{tree}"], &[]).ok() } else { None };
    let mut seen = std::collections::BTreeSet::new();
    for line in refspecs.lines() {
        let [_, sha, rref, _] = line.split_whitespace().collect::<Vec<_>>()[..] else { continue };
        if sha == ZERO_SHA || rref.starts_with("refs/notes/") {
            continue;
        }
        let tree = git(repo, &["rev-parse", &format!("{sha}^{{tree}}")], &[])?;
        if !seen.insert(tree.clone()) {
            continue;
        }
        let code = if head_tree.as_ref() == Some(&tree) {
            gate0(repo, cfg)?
        } else {
            // ponytail: the scratch worktree's gate0 cache dies with it; re-pushing a non-HEAD sha re-runs gate0.
            let dir = git_dir(repo)?.join("keel-push-proof");
            let d = dir.to_string_lossy();
            let _ = git(repo, &["worktree", "remove", "--force", &d], &[]);
            git(repo, &["worktree", "add", "-q", "--detach", &d, sha], &[])?;
            let code = load_config(&dir).and_then(|c| gate0(&dir, &c));
            let _ = git(repo, &["worktree", "remove", "--force", &d], &[]);
            code?
        };
        if code != 0 {
            eprintln!("push refused - {} fails gate0", &sha[..10]);
            return Ok(code);
        }
    }
    Ok(0)
}

/// pre-push: gate0 on every pushed tree, the range check on main/epic updates, then notes sync.
fn pre_push(repo: &Path, cfg: &Config, remote: &str) -> Result<i32, String> {
    use std::io::Read;
    let mut refspecs = String::new();
    std::io::stdin().read_to_string(&mut refspecs).map_err(|e| e.to_string())?;
    let code = gate0_push(repo, cfg, &refspecs)?;
    if code != 0 {
        return Ok(code);
    }
    for line in refspecs.lines() {
        let [_, local, rref, rsha] = line.split_whitespace().collect::<Vec<_>>()[..] else { continue };
        let branch = rref.strip_prefix("refs/heads/").unwrap_or("");
        let gated = branch == cfg.branches.main || branch.starts_with("epic/");
        if gated && local != ZERO_SHA && rsha != ZERO_SHA && gate::range(repo, cfg, rsha, local)? != 0 {
            return Ok(1);
        }
    }
    // A plain push never sends notes refs: without this the evidence stays on one machine.
    let refs = git(repo, &["for-each-ref", "--format=%(refname)", "refs/notes"], &[])?;
    if !refs.is_empty() {
        let mut a = vec!["push", "--no-verify", "--quiet", remote];
        a.extend(refs.lines());
        if git(repo, &a, &[]).is_err() {
            eprintln!("keel pre-push: warning - notes sync to {remote} failed");
        }
    }
    Ok(0)
}

const PRE_COMMIT: &str = r#"#!/bin/sh
command -v keel >/dev/null 2>&1 || { echo "keel: binary not on PATH; install it (cargo install keel) or run: git config --unset core.hooksPath" >&2; exit 1; }
exec keel hook pre-commit
"#;

const PRE_MERGE_COMMIT: &str = r#"#!/bin/sh
command -v keel >/dev/null 2>&1 || { echo "keel: binary not on PATH; install it (cargo install keel) or run: git config --unset core.hooksPath" >&2; exit 1; }
exec keel verify-dod hook
"#;

const COMMIT_MSG: &str = r#"#!/bin/sh
command -v keel >/dev/null 2>&1 || { echo "keel: binary not on PATH; install it (cargo install keel) or run: git config --unset core.hooksPath" >&2; exit 1; }
exec keel hook commit-msg "$1"
"#;

const PRE_PUSH: &str = r#"#!/bin/sh
command -v keel >/dev/null 2>&1 || { echo "keel: binary not on PATH; install it (cargo install keel) or run: git config --unset core.hooksPath" >&2; exit 1; }
exec keel hook pre-push "$1"
"#;

// Harness hooks. Claude names the edited file in tool_input.file_path; Codex's
// apply_patch carries patch text in tool_input.command, whose "*** Add/Update File:"
// headers name the targets. Red -> exit 2 + stderr (fed back / turn refused). Needs jq.
// ponytail: the Codex shape is best-effort (Trawler's), not verified against Codex docs.
const DOC_POST: &str = r#"#!/bin/sh
command -v keel >/dev/null 2>&1 || exit 0
paths=$(jq -r 'if (.tool_input.file_path? // "") != "" then .tool_input.file_path
  else (.tool_input.command? // "") | split("\n")[]
    | select(test("^\\*\\*\\* (Add|Update|Delete) File: |^\\*\\*\\* Move to: "))
    | sub("^\\*\\*\\* (Add|Update|Delete) File: |^\\*\\*\\* Move to: "; "")
  end' 2>/dev/null) || exit 0
IFS='
'
set --
for p in $paths; do
  case "$p" in *.html) [ -f "$p" ] && set -- "$@" "$p" ;; esac
done
[ $# -gt 0 ] || exit 0
out=$(keel verify-doc "$@" 2>&1) && exit 0
printf '%s\n' "$out" >&2
exit 2
"#;

const DOC_STOP: &str = r#"#!/bin/sh
command -v keel >/dev/null 2>&1 || exit 0
out=$(keel verify-doc 2>&1) && exit 0
printf 'A doc is red - fix it before ending the turn:\n%s\n' "$out" >&2
exit 2
"#;

const POST_CHECKOUT: &str = r#"#!/bin/sh
# post-checkout: seed new worktrees from .gitworktreecopy, heal merge.ff=false.
[ "$(git config --local --get merge.ff)" = "false" ] || git config --local merge.ff false
[ "$1" = "0000000000000000000000000000000000000000" ] || exit 0
main=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
here=$(git rev-parse --show-toplevel)
[ "$main" != "$here" ] && [ -f "$main/.gitworktreecopy" ] || exit 0
while IFS= read -r f || [ -n "$f" ]; do
  case "$f" in ''|'#'*) continue ;; esac
  [ -e "$main/$f" ] && [ ! -e "$here/$f" ] || continue
  mkdir -p "$(dirname "$here/$f")" && cp -p "$main/$f" "$here/$f"
done < "$main/.gitworktreecopy"
exit 0
"#;

fn hooks_install(repo: &Path) -> Result<i32, String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo.join(HOOKS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("keel: mkdir {}: {e}", dir.display()))?;
    for (name, body) in [
        ("pre-commit", PRE_COMMIT),
        ("commit-msg", COMMIT_MSG),
        ("pre-merge-commit", PRE_MERGE_COMMIT),
        ("pre-push", PRE_PUSH),
        ("post-checkout", POST_CHECKOUT),
        ("verify-doc-post", DOC_POST),
        ("verify-doc-stop", DOC_STOP),
    ] {
        let p = dir.join(name);
        std::fs::write(&p, body).map_err(|e| format!("keel: write {}: {e}", p.display()))?;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    // Relative: git resolves it per worktree, so each runs the hooks its branch carries.
    git(repo, &["config", "--local", "core.hooksPath", HOOKS_DIR], &[])?;
    println!("hooks installed - core.hooksPath = {HOOKS_DIR} (commit the directory so it is tracked)");
    Ok(0)
}

fn pre_commit(repo: &Path, cfg: &Config) -> Result<i32, String> {
    let branch = git(repo, &["symbolic-ref", "--short", "-q", "HEAD"], &[]).unwrap_or_default();
    if branch == cfg.branches.main {
        eprintln!(
            "keel: commit refused - '{b}' is merge-only.\n  fix -> git switch -c <branch>   (then commit there and merge into {b})",
            b = branch
        );
        return Ok(1);
    }
    Ok(0)
}

/// No paths: every html doc under docs.root (tracked or untracked, not ignored).
fn verify_doc(repo: &Path, paths: &[&str]) -> Result<i32, String> {
    let files: Vec<String> = if paths.is_empty() {
        let glob = format!("{}/*.html", load_config(repo)?.docs.root.trim_end_matches('/'));
        let ls = git(repo, &["ls-files", "--cached", "--others", "--exclude-standard", "--", &glob], &[])?;
        ls.lines().map(|l| repo.join(l).to_string_lossy().into_owned()).collect()
    } else {
        paths.iter().map(|s| s.to_string()).collect()
    };
    let mut red = false;
    for f in &files {
        let errs = verify_doc::verify(Path::new(f));
        for e in &errs {
            eprintln!("{f}: {e}");
        }
        red |= !errs.is_empty();
    }
    Ok(red as i32)
}

fn run() -> Result<i32, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let repo = repo_root()?;
    match args.as_slice() {
        ["hooks", "install"] => hooks_install(&repo),
        ["lint"] => lint(&repo, &load_config(&repo)?),
        ["gate0"] => gate0(&repo, &load_config(&repo)?),
        ["hook", "pre-commit"] => pre_commit(&repo, &load_config(&repo)?),
        ["hook", "commit-msg", file] => mr::commit_msg(&repo, &load_config(&repo)?, file),
        ["hook", "pre-push", remote, ..] => pre_push(&repo, &load_config(&repo)?, remote),
        ["notes", "write", kind, rest @ ..] => notes::write(&repo, &load_config(&repo)?, kind, rest),
        ["verify-doc", paths @ ..] => verify_doc(&repo, paths),
        ["verify-dod", "hook"] => gate::verify_dod(&repo, &load_config_at_head(&repo)?, &["hook"]),
        ["verify-dod", rest @ ..] => gate::verify_dod(&repo, &load_config(&repo)?, rest),
        ["preflight", rest @ ..] => gate::preflight_cmd(&repo, &load_config(&repo)?, rest),
        ["mr", rest @ ..] => mr::run(&repo, &load_config(&repo)?, rest),
        _ => Err("usage: keel <lint | gate0 | verify-doc | verify-dod | preflight | mr <setup|commit|confirm> | hooks install | notes write <test|doc|cr>>".into()),
    }
}

fn main() {
    std::process::exit(run().unwrap_or_else(|e| {
        eprintln!("{e}");
        1
    }));
}
