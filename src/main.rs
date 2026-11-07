use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HOOKS_DIR: &str = ".keel/hooks";

#[derive(Deserialize)]
#[allow(dead_code)] // code_paths / rule_bearing / docs.root are consumed by later WUs
struct Config {
    #[serde(default)]
    code_paths: Vec<String>,
    #[serde(default)]
    rule_bearing: Vec<String>,
    gate0: Gate0,
    branches: Branches,
    docs: Docs,
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
#[allow(dead_code)]
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

const PRE_COMMIT: &str = r#"#!/bin/sh
command -v keel >/dev/null 2>&1 || { echo "keel: binary not on PATH; install it (cargo install keel) or run: git config --unset core.hooksPath" >&2; exit 1; }
exec keel hook pre-commit
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
    for (name, body) in [("pre-commit", PRE_COMMIT), ("post-checkout", POST_CHECKOUT)] {
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

fn run() -> Result<i32, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let repo = repo_root()?;
    match args.as_slice() {
        ["hooks", "install"] => hooks_install(&repo),
        ["lint"] => lint(&repo, &load_config(&repo)?),
        ["gate0"] => gate0(&repo, &load_config(&repo)?),
        ["hook", "pre-commit"] => pre_commit(&repo, &load_config(&repo)?),
        _ => Err("usage: keel <lint | gate0 | hooks install>".into()),
    }
}

fn main() {
    std::process::exit(run().unwrap_or_else(|e| {
        eprintln!("{e}");
        1
    }));
}
