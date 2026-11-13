//! `keel init` (UC-INIT-1): wire a repo of any language to Keel. Idempotent. A project-owned file
//! that exists with other bytes is kept and reported; a Keel-owned skill file (`.claude/skills/keel-*`)
//! that differs is overwritten without asking when `.keel/skills.lock` shows keel wrote those bytes,
//! otherwise only after confirmation (or with `--force`/`-f`).
use crate::{git, hooks_install};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::Path;

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
        ("echo 'keel: set check.lint in .keel/config.toml' >&2; exit 1", "echo 'keel: set check.test in .keel/config.toml' >&2; exit 1")
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

const LOCK: &str = ".keel/skills.lock";
const LOCK_HEADER: &str =
    "# keel init: the SHA-256 of each keel skill file as init last wrote it (shasum -a 256 form). A file still matching is upgraded without asking.\n";

fn hex(bytes: &[u8]) -> String {
    crate::notes::sha256(bytes).trim_start_matches("sha256:").to_string()
}

/// `<64 hex>  <path>` lines, path relative to .claude/skills; anything else is ignored (that file then asks).
fn parse_lock(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once("  "))
        .filter(|(h, _)| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .map(|(h, p)| (p.to_string(), h.to_string()))
        .collect()
}

/// Shipped skills only, in SKILLS order.
fn format_lock(lock: &BTreeMap<String, String>) -> String {
    SKILLS.iter().filter_map(|(p, _)| lock.get(*p).map(|h| format!("{h}  {p}\n"))).fold(LOCK_HEADER.to_string(), |acc, l| acc + &l)
}

#[derive(Debug, PartialEq)]
enum SkillAction {
    Create,
    Unchanged,
    /// differs, but its bytes are what keel last wrote: never edited (B7, DEC-3 amended)
    Upgrade,
    Ask,
}

fn skill_action(current: Option<&[u8]>, build: &[u8], locked: Option<&str>) -> SkillAction {
    match current {
        None => SkillAction::Create,
        Some(c) if c == build => SkillAction::Unchanged,
        Some(c) if locked == Some(hex(c).as_str()) => SkillAction::Upgrade,
        Some(_) => SkillAction::Ask,
    }
}

/// Write the embedded skills. Missing -> create; equal -> unchanged; differs -> upgrade when the lock
/// says keel wrote these bytes, else ask (or `force`). The lock is written when a skill was written or
/// it exists, so a tree whose skills all match and that has no lock (Keel's own) stays untouched.
fn skills(repo: &Path, force: bool, input: &mut impl BufRead, out: &mut impl Write) -> Result<(), String> {
    let lock_text = std::fs::read_to_string(repo.join(LOCK)).ok();
    // like any unreadable path init meets: kept, said so, never overwritten; every differing file asks
    let unreadable = lock_text.is_none() && repo.join(LOCK).symlink_metadata().is_ok();
    let old = lock_text.as_deref().map(parse_lock).unwrap_or_default();
    let mut lock = old.clone();
    let mut wrote = false;
    for (path, body) in SKILLS {
        let rel = format!(".claude/skills/{path}");
        let p = repo.join(&rel);
        let current = std::fs::read(&p).ok();
        if current.is_none() && p.symlink_metadata().is_ok() {
            writeln!(out, "  kept      {rel} (exists but is not a readable file)").ok();
            continue;
        }
        let go = match skill_action(current.as_deref(), body.as_bytes(), old.get(*path).map(String::as_str)) {
            SkillAction::Unchanged => {
                writeln!(out, "  unchanged {rel}").ok();
                lock.insert(path.to_string(), hex(body.as_bytes()));
                continue;
            }
            SkillAction::Create => {
                std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("keel: mkdir for {rel}: {e}"))?;
                std::fs::write(&p, body).map_err(|e| format!("keel: write {rel}: {e}"))?;
                writeln!(out, "  created   {rel}").ok();
                lock.insert(path.to_string(), hex(body.as_bytes()));
                wrote = true;
                continue;
            }
            SkillAction::Upgrade => Some(" (unedited since keel last wrote it)"),
            SkillAction::Ask if force => Some(""),
            SkillAction::Ask => {
                writeln!(
                    out,
                    "{rel} differs from this keel build ({}).\nLocal edits or an older Keel version - keel cannot tell which.",
                    env!("CARGO_PKG_VERSION")
                )
                .ok();
                confirm(input, out, &format!("Overwrite with this build's version?{}  (-f overwrites without asking)", git_note(repo, &rel))).then_some("")
            }
        };
        match go {
            Some(why) => {
                std::fs::write(&p, body).map_err(|e| format!("keel: write {rel}: {e}"))?;
                writeln!(out, "  updated   {rel}{why}").ok();
                lock.insert(path.to_string(), hex(body.as_bytes()));
                wrote = true;
            }
            None => {
                writeln!(out, "  differs   {rel} (kept; re-run `keel init` and answer y, or pass -f, to take this build's version)").ok();
            }
        }
    }
    let body = format_lock(&lock);
    if unreadable {
        writeln!(out, "  kept      {LOCK} (exists but is not a readable file; every differing skill file asks)").ok();
    } else if lock_text.as_deref() == Some(body.as_str()) {
        writeln!(out, "  unchanged {LOCK}").ok();
    } else if lock_text.is_some() || wrote {
        std::fs::create_dir_all(repo.join(".keel")).map_err(|e| format!("keel: mkdir .keel: {e}"))?;
        std::fs::write(repo.join(LOCK), &body).map_err(|e| format!("keel: write {LOCK}: {e}"))?;
        writeln!(out, "  {} {LOCK}", if lock_text.is_some() { "updated  " } else { "created  " }).ok();
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
        let present = groups.iter().any(|g| g["hooks"].as_array().is_some_and(|hs| hs.iter().any(|h| h["command"].as_str() == Some(cmd))));
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
    match keel_on_path() {
        None => println!("warning: keel is not on PATH - the hooks call it; install: cargo install --git https://github.com/lesslucifer/Keel, or prebuilt binaries, npm and Homebrew: {INSTALL_PAGE}"),
        Some((path, version)) if !is_keel(&version) => println!(
            "warning: {path} is not Keel (its `keel --version` lacks \"{KEEL_MARK}\") - another program is named keel, e.g. keel.so; the git hooks refuse to run it. Put Keel first on PATH: {INSTALL_PAGE}"
        ),
        Some(_) => {}
    }
    println!("{NEXT_STEPS}");
    Ok(0)
}

/// The second line of Keel's `keel --version`; the git hook shims grep for the same text (open point 6).
pub(crate) const KEEL_MARK: &str = "Keel: merge-gate workflow CLI";
const INSTALL_PAGE: &str = "https://github.com/lesslucifer/Keel/blob/main/docs/install.html";

fn is_keel(version_out: &str) -> bool {
    version_out.lines().any(|l| l.starts_with(KEEL_MARK))
}

/// The `keel` the hooks would run, as (path, `keel --version` stdout); None when none is on PATH.
fn keel_on_path() -> Option<(String, String)> {
    let sh = |script: &str| crate::no_git_env(std::process::Command::new("sh"), false).args(["-c", script]).stdin(std::process::Stdio::null()).output().ok();
    let path = sh("command -v keel").filter(|o| o.status.success())?;
    let version = sh("keel --version 2>/dev/null").map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    Some((String::from_utf8_lossy(&path.stdout).trim().to_string(), version))
}

/// The signer comes before the first commit, so the policy lands on main with its roster (DEC-47).
const NEXT_STEPS: &str = "next:\n  1. review .keel/config.toml (check commands, code_paths) and AGENTS.md; fill in .claude/skills/project-router/SKILL.md\n  \
     2. the human owner registers as signer (never an agent): keel mr setup --as <identity> [--key ~/.ssh/<key>]\n  \
     3. land the policy and the roster (.keel/mr-allowed) on main once, before any gated merge (main is merge-only from now on):\n     \
     git add .keel AGENTS.md .claude/skills .claude/settings.json .codex .gitignore && git commit --no-verify -m \"keel init\"";

#[cfg(test)]
mod tests {
    use super::{confirm, format_lock, hex, is_keel, parse_lock, skill_action, SkillAction, KEEL_MARK, NEXT_STEPS, SKILLS};
    use std::io::Cursor;

    fn ask(input: &str) -> bool {
        confirm(&mut Cursor::new(input.to_string()), &mut Vec::new(), "q?")
    }

    #[test]
    fn the_next_steps_register_the_signer_before_the_one_time_first_commit() {
        let setup = NEXT_STEPS.find("keel mr setup").unwrap();
        let first = NEXT_STEPS.find("git commit --no-verify").unwrap();
        assert!(setup < first, "{NEXT_STEPS}");
        assert!(NEXT_STEPS.contains("git add .keel "), "the roster under .keel/ rides the first commit");
    }

    #[test]
    fn the_lock_round_trips_shipped_paths_in_order_and_ignores_the_rest() {
        let (a, b) = (SKILLS[0].0, SKILLS[1].0);
        let lock = [(b.to_string(), hex(b"b")), (a.to_string(), hex(b"a")), ("gone/SKILL.md".to_string(), hex(b"x"))].into_iter().collect();
        let text = format_lock(&lock);
        let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(lines, [format!("{}  {a}", hex(b"a")), format!("{}  {b}", hex(b"b"))], "{text}");
        let back = parse_lock(&text);
        assert_eq!(back.len(), 2);
        assert_eq!(back[a], hex(b"a"));
        let junk = parse_lock("# c\nnot a line\nABCD  x\n0123  short\nzz{}  y\n");
        assert!(junk.is_empty(), "{junk:?}");
    }

    #[test]
    fn hex_is_shasum_256_lowercase() {
        assert_eq!(hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_differing_file_upgrades_silently_only_when_its_bytes_are_what_the_lock_recorded() {
        let (build, old) = (b"new".as_slice(), b"old".as_slice());
        assert_eq!(skill_action(None, build, None), SkillAction::Create);
        assert_eq!(skill_action(None, build, Some(&hex(old))), SkillAction::Create);
        assert_eq!(skill_action(Some(build), build, None), SkillAction::Unchanged);
        assert_eq!(skill_action(Some(build), build, Some(&hex(old))), SkillAction::Unchanged);
        assert_eq!(skill_action(Some(old), build, Some(&hex(old))), SkillAction::Upgrade);
        assert_eq!(skill_action(Some(b"edited"), build, Some(&hex(old))), SkillAction::Ask);
        assert_eq!(skill_action(Some(old), build, None), SkillAction::Ask, "no entry: ask as today");
    }

    #[test]
    fn only_a_version_output_carrying_keels_line_is_keel() {
        assert!(is_keel(&format!("keel 0.4.0\n{KEEL_MARK} (not keel.so)\n")));
        assert!(!is_keel("keel.so 1.2.3\n"));
        assert!(!is_keel(""));
        assert!(!is_keel(&format!("prefix {KEEL_MARK}\n")), "the line must start with the mark, as the shims' grep '^...' does");
    }

    #[test]
    fn confirm_answers() {
        assert!(ask("y\n") && ask("YES\n"));
        assert!(!ask("n\n") && !ask("\n") && !ask(""));
        assert!(ask("what\ny\n"), "garbage is re-asked once");
        assert!(!ask("what\nwhat\ny\n"), "second garbage keeps");
    }
}
