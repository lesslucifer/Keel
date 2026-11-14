//! `keel init` (UC-INIT-1): wire a repo of any language to Keel. Idempotent. A project-owned file
//! that exists with other bytes is kept and reported; a Keel-owned skill file (`.claude/skills/keel-*`)
//! that differs is overwritten only after confirmation (or with `--force`/`-f`). The skills live
//! once, in `.claude/skills`; `.agents/skills` is a relative link every other host reads.
use crate::{git, hooks_install};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Every file under `skills/`, embedded; written to `.claude/skills/<path>`. The completeness
/// test (tests/init.rs) fails when a file under skills/ is missing here.
const SKILLS: &[(&str, &str)] = &[
    ("keel-workflow/SKILL.md", include_str!("../skills/keel-workflow/SKILL.md")),
    ("keel-plan/SKILL.md", include_str!("../skills/keel-plan/SKILL.md")),
    ("keel-code-review/SKILL.md", include_str!("../skills/keel-code-review/SKILL.md")),
    ("keel-code-review/rubric.md", include_str!("../skills/keel-code-review/rubric.md")),
    ("keel-distill/SKILL.md", include_str!("../skills/keel-distill/SKILL.md")),
    ("keel-write/SKILL.md", include_str!("../skills/keel-write/SKILL.md")),
    ("keel-write/references/example.html", include_str!("../skills/keel-write/references/example.html")),
    ("keel-router/SKILL.md", include_str!("../skills/keel-router/SKILL.md")),
    ("keel-router/references/brief-skeleton.md", include_str!("../skills/keel-router/references/brief-skeleton.md")),
    ("keel-router/references/commands.md", include_str!("../skills/keel-router/references/commands.md")),
    ("keel-router/references/lessons.md", include_str!("../skills/keel-router/references/lessons.md")),
    ("keel-router/references/messages.md", include_str!("../skills/keel-router/references/messages.md")),
    ("keel-router/references/worker-ops.md", include_str!("../skills/keel-router/references/worker-ops.md")),
    ("keel-router/references/project-router-template.md", include_str!("../skills/keel-router/references/project-router-template.md")),
    ("keel-router/references/review-checklists.md", include_str!("../skills/keel-router/references/review-checklists.md")),
];

/// The one shared skill root: a link relative to `.agents/`, so a checkout in another worktree still
/// resolves it (E-DSH.W1).
const SHARED_SKILLS: &str = "../.claude/skills";
const SHARED_SKILLS_LINK: &str = ".agents/skills";

/// What is at `.agents/skills`, read without following it: fabricated input for `shared_root`.
#[derive(Debug, PartialEq, Eq)]
enum LinkState {
    Missing,
    Symlink(PathBuf),
    Unreadable,
    RealDir,
    RealFile,
}

/// What to do about the shared root: create it, it is already the link, or keep what is there.
#[derive(Debug, PartialEq, Eq)]
enum SharedRoot {
    Create,
    Shared,
    Keep(String),
}

/// Only exactly the relative link is the shared root: anything else is a project's own root, kept.
fn shared_root(state: LinkState) -> SharedRoot {
    match state {
        LinkState::Missing => SharedRoot::Create,
        LinkState::Symlink(t) if t.as_path() == Path::new(SHARED_SKILLS) => SharedRoot::Shared,
        LinkState::Symlink(t) => SharedRoot::Keep(format!("-> {}; not the shared root", t.display())),
        LinkState::Unreadable => SharedRoot::Keep("an unreadable link; not the shared root".into()),
        LinkState::RealDir => SharedRoot::Keep("a real directory; not the shared root".into()),
        LinkState::RealFile => SharedRoot::Keep("a real file; not the shared root".into()),
    }
}

fn link_state(repo: &Path) -> LinkState {
    let p = repo.join(SHARED_SKILLS_LINK);
    if let Ok(target) = std::fs::read_link(&p) {
        return LinkState::Symlink(target);
    }
    match p.symlink_metadata() {
        Err(_) => LinkState::Missing,
        Ok(m) if m.is_dir() => LinkState::RealDir,
        Ok(m) if m.file_type().is_symlink() => LinkState::Unreadable,
        Ok(_) => LinkState::RealFile,
    }
}

/// Plant the shared root once. Idempotent, and it never replaces a real directory or another link.
fn shared_skills(repo: &Path) -> Result<(), String> {
    match shared_root(link_state(repo)) {
        SharedRoot::Shared => println!("  unchanged {SHARED_SKILLS_LINK} (-> {SHARED_SKILLS})"),
        SharedRoot::Create => {
            std::fs::create_dir_all(repo.join(".agents")).map_err(|e| format!("keel: mkdir .agents: {e}"))?;
            std::os::unix::fs::symlink(SHARED_SKILLS, repo.join(SHARED_SKILLS_LINK)).map_err(|e| format!("keel: link {SHARED_SKILLS_LINK}: {e}"))?;
            println!("  created   {SHARED_SKILLS_LINK} (-> {SHARED_SKILLS})");
        }
        SharedRoot::Keep(why) => {
            println!("  kept      {SHARED_SKILLS_LINK} ({why}; remove it and re-run `keel init` to take the shared root)")
        }
    }
    Ok(())
}

const POST_CMD: &str = r##"f="$(git rev-parse --git-common-dir 2>/dev/null)/keel-hooks/verify-doc-post"; [ -f "$f" ] || exit 0; sh "$f""##;
const STOP_CMD: &str = r##"f="$(git rev-parse --git-common-dir 2>/dev/null)/keel-hooks/verify-doc-stop"; [ -f "$f" ] || exit 0; sh "$f""##;

const CONFIG: &str = include_str!("../templates/config.toml");
const AGENTS: &str = include_str!("../templates/AGENTS.md");
const CODEX_HOOKS: &str = include_str!("../templates/codex-hooks.json");
const PROJECT_ROUTER: &str = include_str!("../skills/keel-router/references/project-router-template.md");

/// check commands by the first manifest found; else a command that fails until configured.
fn check_for(repo: &Path) -> (&'static str, &'static str) {
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
            "echo 'keel: set check.lint in .keel/config.toml' >&2; exit 1",
            "echo 'keel: set check.test in .keel/config.toml' >&2; exit 1",
        )
    }
}

fn config(repo: &Path) -> String {
    let (lint, test) = check_for(repo);
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

/// Append `line` to .gitignore unless present - additive, never rewrites the file.
fn ignore(repo: &Path, line: &str) -> Result<(), String> {
    let line = line.trim();
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

/// y/yes -> true; n, empty, EOF -> false; anything else is re-asked once, then false.
fn confirm(input: &mut impl BufRead, out: &mut impl Write, prompt: &str) -> bool {
    for _ in 0..2 {
        let _ = write!(out, "{prompt} [y/N] ");
        let _ = out.flush();
        let mut line = String::new();
        if input.read_line(&mut line).unwrap_or(0) == 0 {
            let _ = writeln!(out);
            return false;
        }
        match line.trim().to_lowercase().as_str() {
            "y" | "yes" => return true,
            "" | "n" | "no" => return false,
            _ => {}
        }
    }
    false
}

/// Recoverability note for the prompt: committed and clean, or uncommitted changes that would be lost.
fn git_note(repo: &Path, rel: &str) -> &'static str {
    let tracked = git(repo, &["ls-files", "--error-unmatch", "--", rel], &[]).is_ok();
    let clean = git(repo, &["status", "--porcelain", "--", rel], &[]).is_ok_and(|s| s.trim().is_empty());
    match (tracked, clean) {
        (true, true) => " (committed - recoverable with git checkout)",
        (_, false) | (false, _) => " (UNCOMMITTED changes will be lost)",
    }
}

/// Write the embedded skills. Missing -> create; equal -> unchanged; differs -> ask (or `force`).
fn skills(repo: &Path, force: bool, input: &mut impl BufRead, out: &mut impl Write) -> Result<(), String> {
    for (path, body) in SKILLS {
        let rel = format!(".claude/skills/{path}");
        let p = repo.join(&rel);
        match std::fs::read(&p) {
            Ok(cur) if cur == body.as_bytes() => {
                writeln!(out, "  unchanged {rel}").ok();
            }
            Ok(_) => {
                let go = force || {
                    writeln!(out, "{rel} differs from this keel build ({}).\nLocal edits or an older Keel version - keel cannot tell which.", env!("CARGO_PKG_VERSION")).ok();
                    confirm(input, out, &format!("Overwrite with this build's version?{}  (-f overwrites without asking)", git_note(repo, &rel)))
                };
                if go {
                    std::fs::write(&p, body).map_err(|e| format!("keel: write {rel}: {e}"))?;
                    writeln!(out, "  updated   {rel}").ok();
                } else {
                    writeln!(out, "  differs   {rel} (kept; re-run `keel init` and answer y, or pass -f, to take this build's version)").ok();
                }
            }
            Err(_) if p.symlink_metadata().is_ok() => {
                writeln!(out, "  kept      {rel} (exists but is not a readable file)").ok();
            }
            Err(_) => {
                std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("keel: mkdir for {rel}: {e}"))?;
                std::fs::write(&p, body).map_err(|e| format!("keel: write {rel}: {e}"))?;
                writeln!(out, "  created   {rel}").ok();
            }
        }
    }
    // never delete: report files in a keel-* dir that this build does not ship
    let mut stack = vec![repo.join(".claude/skills")];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(repo.join(".claude/skills")).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                if d != repo.join(".claude/skills") || rel.starts_with("keel-") {
                    stack.push(p);
                }
            } else if rel.starts_with("keel-") && !SKILLS.iter().any(|(s, _)| *s == rel) {
                writeln!(out, "  extra     .claude/skills/{rel} (not in this keel build; left in place)").ok();
            }
        }
    }
    Ok(())
}

/// Merge the two Keel harness hooks into .claude/settings.json. Never overwrites: absent -> create,
/// invalid or oddly shaped -> leave untouched and print the entries, present -> add only what is missing.
fn settings(repo: &Path) -> Result<(), String> {
    use serde_json::{json, Value};
    const REL: &str = ".claude/settings.json";
    let p = repo.join(REL);
    let entries = [("PostToolUse", Some("Write|Edit"), POST_CMD), ("Stop", None, STOP_CMD)];
    let by_hand = |why: &str| {
        println!("  kept      {REL} ({why} - add the Keel hooks by hand: PostToolUse matcher \"Write|Edit\" -> {POST_CMD} ; Stop -> {STOP_CMD})");
        Ok(())
    };
    let mut root: Value = match std::fs::read_to_string(&p) {
        Ok(t) => match serde_json::from_str(&t) {
            Ok(v @ Value::Object(_)) => v,
            _ => return by_hand("not valid JSON"),
        },
        Err(_) if p.symlink_metadata().is_ok() => return by_hand("not a readable file"),
        Err(_) => json!({}),
    };
    let existed = p.exists();
    let hooks = root.as_object_mut().unwrap().entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else { return by_hand("\"hooks\" is not an object") };
    let mut added = 0;
    for (event, matcher, cmd) in entries {
        let Some(groups) = hooks.entry(event).or_insert_with(|| json!([])).as_array_mut() else {
            return by_hand(&format!("\"hooks.{event}\" is not an array"));
        };
        let present = groups.iter().any(|g| {
            g["hooks"].as_array().is_some_and(|hs| hs.iter().any(|h| h["command"].as_str() == Some(cmd)))
        });
        if !present {
            let hook = json!([{"type": "command", "command": cmd}]);
            let g = match matcher {
                Some(m) => json!({"matcher": m, "hooks": hook}),
                None => json!({"hooks": hook}),
            };
            groups.push(g);
            added += 1;
        }
    }
    if added == 0 {
        println!("  unchanged {REL}");
        return Ok(());
    }
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("keel: mkdir .claude: {e}"))?;
    let body = serde_json::to_string_pretty(&root).unwrap() + "\n";
    std::fs::write(&p, body).map_err(|e| format!("keel: write {REL}: {e}"))?;
    println!("  {} {REL}", if existed { "merged   " } else { "created  " });
    Ok(())
}

/// `[paths] worktrees` from the config just written (or the user's own); default `.claude/worktrees/`.
fn worktrees_dir(repo: &Path) -> String {
    std::fs::read_to_string(repo.join(".keel/config.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Value>().ok())
        .and_then(|v| v.get("paths")?.get("worktrees")?.as_str().map(String::from))
        .unwrap_or_else(|| ".claude/worktrees/".into())
}

pub fn run(repo: &Path, force: bool) -> Result<i32, String> {
    println!("keel init in {}", repo.display());
    write(repo, ".keel/config.toml", &config(repo))?;
    write(repo, "AGENTS.md", AGENTS)?;
    skills(repo, force, &mut std::io::stdin().lock(), &mut std::io::stdout())?;
    write(repo, ".claude/skills/project-router/SKILL.md", PROJECT_ROUTER)?;
    shared_skills(repo)?;
    settings(repo)?;
    write(repo, ".codex/hooks.json", CODEX_HOOKS)?;
    for d in ["docs/use-cases", "docs/decisions", "docs/work"] {
        std::fs::create_dir_all(repo.join(d)).map_err(|e| format!("keel: mkdir {d}: {e}"))?;
    }
    ignore(repo, "docs/.archive/")?;
    ignore(repo, &worktrees_dir(repo))?;
    // -f overwrites skills only: replacing another tool's hooks is its own explicit act.
    let code = hooks_install(repo, false)?;
    if code != 0 {
        eprintln!("keel init: the files above are written; Keel's hooks are NOT installed, so nothing is gated yet");
        return Ok(code);
    }

    if repo.join("CLAUDE.md").exists() {
        println!("warning: CLAUDE.md exists - Claude Code reads it instead of AGENTS.md; merge the two or add a line `@AGENTS.md` to CLAUDE.md");
    }
    let on_path = crate::no_git_env(std::process::Command::new("sh"), false).args(["-c", "command -v keel"]).output().is_ok_and(|o| o.status.success());
    if !on_path {
        println!("warning: keel is not on PATH - the hooks call it; install: cargo install --git https://github.com/lesslucifer/Keel");
    }
    println!("{NEXT_STEPS}");
    Ok(0)
}

/// The signer comes before the first commit, so the policy lands on main with its roster (DEC-47).
/// `.agents` rides the first commit with the rest: the shared skill root is project state too.
const NEXT_STEPS: &str = "next:\n  1. review .keel/config.toml (check commands, code_paths) and AGENTS.md; fill in .claude/skills/project-router/SKILL.md\n  \
     2. the human owner registers as signer (never an agent): keel mr setup --as <identity> [--key ~/.ssh/<key>]\n  \
     3. land the policy and the roster (.keel/mr-allowed) on main once, before any gated merge (main is merge-only from now on):\n     \
     git add .keel AGENTS.md .claude/skills .claude/settings.json .codex .agents .gitignore && git commit --no-verify -m \"keel init\"\n  \
     4. DeepSeek Harness reads the same skills through .agents/skills; a plugin lives in the DSH profile, never in the project - install the three:\n       \
     dsh plugin --profile <name> add <path>/dsh-auto-review\n       \
     dsh plugin --profile <name> add <path>/dsh-worker-workspace\n       \
     dsh plugin --profile <name> add <path>/dsh-doc-gate";

#[cfg(test)]
mod tests {
    use super::{confirm, shared_root, LinkState, SharedRoot, NEXT_STEPS};
    use std::io::Cursor;
    use std::path::PathBuf;

    fn ask(input: &str) -> bool {
        confirm(&mut Cursor::new(input.to_string()), &mut Vec::new(), "q?")
    }

    fn kept(state: LinkState) -> String {
        match shared_root(state) {
            SharedRoot::Keep(why) => why,
            other => panic!("expected kept, got {other:?}"),
        }
    }

    #[test]
    fn the_shared_root_is_created_once_and_kept_when_it_is_not_the_relative_link() {
        assert_eq!(shared_root(LinkState::Missing), SharedRoot::Create);
        assert_eq!(shared_root(LinkState::Symlink(PathBuf::from("../.claude/skills"))), SharedRoot::Shared);
        for elsewhere in ["../skills", ".claude/skills", "/abs/.claude/skills"] {
            let why = kept(LinkState::Symlink(PathBuf::from(elsewhere)));
            assert!(why.contains(elsewhere), "the report names the link's target: {why}");
        }
        assert!(kept(LinkState::RealDir).contains("directory"), "a real directory is reported as one");
        assert!(kept(LinkState::RealFile).contains("file"), "a real file is reported as one");
        assert!(kept(LinkState::Unreadable).contains("link"), "an unreadable link is reported as one");
    }

    #[test]
    fn the_hand_off_installs_the_three_dsh_profile_plugins() {
        for plugin in ["dsh-auto-review", "dsh-worker-workspace", "dsh-doc-gate"] {
            let line = format!("dsh plugin --profile <name> add <path>/{plugin}");
            assert!(NEXT_STEPS.contains(&line), "the hand-off misses {line:?}");
        }
        assert!(NEXT_STEPS.contains("a plugin lives in the DSH profile, never in the project"), "{NEXT_STEPS}");
        assert!(NEXT_STEPS.contains(".agents/skills"), "{NEXT_STEPS}");
        assert!(NEXT_STEPS.contains(".agents .gitignore"), "the shared root rides the first commit: {NEXT_STEPS}");
    }

    #[test]
    fn the_next_steps_register_the_signer_before_the_one_time_first_commit() {
        let setup = NEXT_STEPS.find("keel mr setup").unwrap();
        let first = NEXT_STEPS.find("git commit --no-verify").unwrap();
        assert!(setup < first, "{NEXT_STEPS}");
        assert!(NEXT_STEPS.contains("git add .keel "), "the roster under .keel/ rides the first commit");
    }

    #[test]
    fn confirm_answers() {
        assert!(ask("y\n") && ask("YES\n"));
        assert!(!ask("n\n") && !ask("\n") && !ask(""));
        assert!(ask("what\ny\n"), "garbage is re-asked once");
        assert!(!ask("what\nwhat\ny\n"), "second garbage keeps");
    }
}
