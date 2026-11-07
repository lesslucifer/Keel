//! `keel init` (UC-INIT-1): wire a repo of any language to Keel. Idempotent; a file that
//! exists with other bytes is kept and reported, never overwritten.
use crate::{git, hooks_install};
use std::path::Path;

const CONFIG: &str = include_str!("../templates/config.toml");
const AGENTS: &str = include_str!("../templates/AGENTS.md");
const CODEX_HOOKS: &str = include_str!("../templates/codex-hooks.json");
const PROJECT_ROUTER: &str = include_str!("../skills/router/references/project-router-template.md");

/// gate0 commands by the first manifest found; else a command that fails until configured.
fn gate0_for(repo: &Path) -> (&'static str, &'static str) {
    let has = |f: &str| repo.join(f).exists();
    if has("Cargo.toml") {
        ("cargo clippy --all-targets -- -D warnings", "cargo test")
    } else if has("pyproject.toml") || has("setup.py") || has("requirements.txt") {
        ("ruff check .", "pytest -q")
    } else if has("package.json") {
        ("npm run lint", "npm test")
    } else if has("go.mod") {
        ("go vet ./...", "go test ./...")
    } else {
        (
            "echo 'keel: set gate0.lint in .keel/config.toml' >&2; exit 1",
            "echo 'keel: set gate0.test in .keel/config.toml' >&2; exit 1",
        )
    }
}

fn config(repo: &Path) -> String {
    let (lint, test) = gate0_for(repo);
    let has_branch = |b: &str| git(repo, &["rev-parse", "--verify", "-q", &format!("refs/heads/{b}")], &[]).is_ok();
    let main = if has_branch("master") && !has_branch("main") { "master" } else { "main" };
    // Debug-quoting gives a valid TOML basic string for these ASCII values.
    CONFIG.replace("{{LINT}}", &format!("{lint:?}")).replace("{{TEST}}", &format!("{test:?}")).replace("{{MAIN}}", &format!("{main:?}"))
}

fn write(repo: &Path, rel: &str, body: &str) -> Result<(), String> {
    let p = repo.join(rel);
    match std::fs::read_to_string(&p) {
        Ok(cur) if cur == body => println!("  unchanged {rel}"),
        Ok(_) => println!("  kept      {rel} (differs from the default; not overwritten - delete it to take the default)"),
        Err(_) if p.symlink_metadata().is_ok() => println!("  kept      {rel} (exists but is not a readable file)"),
        Err(_) => {
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("keel: mkdir for {rel}: {e}"))?;
            std::fs::write(&p, body).map_err(|e| format!("keel: write {rel}: {e}"))?;
            println!("  created   {rel}");
        }
    }
    Ok(())
}

fn symlink(repo: &Path, rel: &str, target: &str) -> Result<(), String> {
    let p = repo.join(rel);
    match std::fs::read_link(&p) {
        Ok(t) if t == Path::new(target) => println!("  unchanged {rel} -> {target}"),
        _ if p.symlink_metadata().is_ok() => println!("  kept      {rel} (exists and is not a symlink to {target})"),
        _ => {
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("keel: mkdir for {rel}: {e}"))?;
            std::os::unix::fs::symlink(target, &p).map_err(|e| format!("keel: symlink {rel}: {e}"))?;
            println!("  created   {rel} -> {target}");
        }
    }
    Ok(())
}

/// Append `line` to .gitignore unless present - additive, never rewrites the file.
fn ignore(repo: &Path, line: &str) -> Result<(), String> {
    let p = repo.join(".gitignore");
    let mut body = std::fs::read_to_string(&p).unwrap_or_default();
    if body.lines().any(|l| l.trim() == line) {
        println!("  unchanged .gitignore ({line})");
        return Ok(());
    }
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    std::fs::write(&p, format!("{body}{line}\n")).map_err(|e| format!("keel: write .gitignore: {e}"))?;
    println!("  appended  .gitignore ({line})");
    Ok(())
}

pub fn run(repo: &Path) -> Result<i32, String> {
    println!("keel init in {}", repo.display());
    write(repo, ".keel/config.toml", &config(repo))?;
    write(repo, "AGENTS.md", AGENTS)?;
    write(repo, ".claude/skills/project-router/SKILL.md", PROJECT_ROUTER)?;
    symlink(repo, ".agents/skills", "../.claude/skills")?;
    write(repo, ".codex/hooks.json", CODEX_HOOKS)?;
    for d in ["docs/use-cases", "docs/decisions", "docs/work"] {
        std::fs::create_dir_all(repo.join(d)).map_err(|e| format!("keel: mkdir {d}: {e}"))?;
    }
    ignore(repo, "docs/.archive/")?;
    hooks_install(repo)?;

    if repo.join("CLAUDE.md").exists() {
        println!("warning: CLAUDE.md exists - Claude Code reads it instead of AGENTS.md; merge the two or add a line `@AGENTS.md` to CLAUDE.md");
    }
    let on_path = std::process::Command::new("sh").args(["-c", "command -v keel"]).output().is_ok_and(|o| o.status.success());
    if !on_path {
        println!("warning: keel is not on PATH - the hooks call it; install: cargo install keel (or a release binary)");
    }
    println!(
        "next:\n  1. review .keel/config.toml (gate0 commands, code_paths) and AGENTS.md; fill in .claude/skills/project-router/SKILL.md\n  \
         2. land the policy on main once, before any gated merge (main is merge-only from now on):\n     \
         git add .keel AGENTS.md .claude .agents .codex .gitignore && git commit --no-verify -m \"keel init\"\n  \
         3. the human owner registers as signer (never an agent): keel mr setup --as <identity> [--key ~/.ssh/<key>.pub]"
    );
    Ok(0)
}
