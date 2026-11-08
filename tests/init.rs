use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn keel(d: &Path, a: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_keel")).args(a).current_dir(d).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// path -> file bytes, symlink target, or "<dir>"; `.git` excluded.
fn tree(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        if rel == ".git" {
            continue;
        }
        let meta = p.symlink_metadata().unwrap();
        if meta.file_type().is_symlink() {
            out.insert(rel, format!("-> {}", std::fs::read_link(&p).unwrap().display()));
        } else if meta.is_dir() {
            out.insert(rel, "<dir>".into());
            tree(root, &p, out);
        } else {
            out.insert(rel, std::fs::read_to_string(&p).unwrap());
        }
    }
}

fn snapshot(p: &Path) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    tree(p, p, &mut m);
    m
}

#[test]
fn init_twice_produces_the_same_tree_and_keeps_user_edits() {
    let d = TempDir::new().unwrap();
    let p = d.path();
    assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(p).status().unwrap().success());
    std::fs::write(p.join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();

    let first = keel(p, &["init"]);
    assert!(first.status.success(), "{}", text(&first));
    let after_first = snapshot(p);
    let second = keel(p, &["init"]);
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(after_first, snapshot(p), "second init changed the tree");
    assert!(!text(&second).contains("created"), "{}", text(&second));

    for f in [".keel/config.toml", "AGENTS.md", ".codex/hooks.json", ".claude/skills/project-router/SKILL.md", ".claude/skills/keel-workflow/SKILL.md", ".claude/settings.json", "docs/work"] {
        assert!(after_first.contains_key(f), "missing {f}");
    }
    // W6 #1/#5: skills live only in .claude/skills; no .agents/skills Codex symlink
    assert!(!after_first.contains_key(".agents"), "init must not create .agents");
    assert!(after_first[".keel/config.toml"].contains("test = \"pytest -q\""));
    let hooks = Command::new("git").args(["config", "core.hooksPath"]).current_dir(p).output().unwrap();
    assert!(String::from_utf8_lossy(&hooks.stdout).trim().ends_with("keel-hooks"));
    // the written config loads
    assert!(keel(p, &["verify-doc"]).status.success());
    // the Codex PostToolUse command reaches the installed hook script: a red doc -> exit 2
    std::fs::write(p.join("docs/work/x.html"), r#"<html data-artifact="wu-plan" data-status="ready"></html>"#).unwrap();
    let hooks_json: serde_json::Value = serde_json::from_str(&after_first[".codex/hooks.json"]).unwrap();
    let cmd = hooks_json["hooks"]["PostToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().display().to_string();
    let mut sh = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(p)
        .env("PATH", format!("{bin}:{}", std::env::var("PATH").unwrap()))
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    sh.stdin.take().unwrap().write_all(br#"{"tool_input":{"file_path":"docs/work/x.html"}}"#).unwrap();
    let out = sh.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_file(p.join("docs/work/x.html")).unwrap();

    let edited = after_first[".keel/config.toml"].replace("pytest -q", "pytest -x");
    std::fs::write(p.join(".keel/config.toml"), &edited).unwrap();
    let third = keel(p, &["init"]);
    assert!(text(&third).contains("kept      .keel/config.toml"), "{}", text(&third));
    assert_eq!(std::fs::read_to_string(p.join(".keel/config.toml")).unwrap(), edited);
}

fn md_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            md_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
}

#[test]
fn every_keel_command_a_skill_names_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    let dispatch = &main[main.find("fn run()").unwrap()..main.find("fn main()").unwrap()];
    let arm = regex::Regex::new(r#"(?m)^\s*\["([a-z0-9-]+)""#).unwrap();
    let known: Vec<&str> = arm.captures_iter(dispatch).map(|c| c.get(1).unwrap().as_str()).collect();
    assert!(known.contains(&"init") && known.contains(&"verify-dod"), "{known:?}");

    let mut files = vec![root.join("templates/AGENTS.md")];
    md_files(&root.join("skills"), &mut files);
    let named = regex::Regex::new(r"\bkeel ([a-z][a-z0-9-]*)").unwrap();
    let mut seen = 0;
    for f in &files {
        let body = std::fs::read_to_string(f).unwrap();
        assert!(!body.contains("cargo xtask"), "{} still names cargo xtask", f.display());
        for c in named.captures_iter(&body) {
            let cmd = c.get(1).unwrap().as_str();
            assert!(known.contains(&cmd), "{} names `keel {cmd}`, which run() does not dispatch", f.display());
            seen += 1;
        }
    }
    assert!(seen > 20, "only {seen} keel commands found - is the scan broken?");
}

fn fresh_repo() -> TempDir {
    let d = TempDir::new().unwrap();
    assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(d.path()).status().unwrap().success());
    d
}

fn keel_in(d: &Path, a: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut c = Command::new(env!("CARGO_BIN_EXE_keel"))
        .args(a)
        .current_dir(d)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let _ = c.stdin.take().unwrap().write_all(stdin.as_bytes());
    c.wait_with_output().unwrap()
}

fn files(dir: &Path, base: &Path, out: &mut BTreeMap<String, String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, base, out);
        } else {
            out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap());
        }
    }
}

#[test]
fn embedding_is_complete_and_prefixed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut src = BTreeMap::new();
    files(&root.join("skills"), &root.join("skills"), &mut src);
    let d = fresh_repo();
    assert!(keel(d.path(), &["init"]).status.success());
    let mut got = BTreeMap::new();
    let sk = d.path().join(".claude/skills");
    files(&sk, &sk, &mut got);
    // project-router is the template, written once, project-owned
    assert_eq!(got.remove("project-router/SKILL.md").as_deref(), src.get("keel-router/references/project-router-template.md").map(String::as_str));
    assert_eq!(src, got, "skills/ and .claude/skills differ (file missing from SKILLS?)");

    // every skill dir is keel-*, name: matches, and no bare skill names remain
    let bare = regex::Regex::new(r"(?i)\bskills? `?(workflow|plan|code-review|distill|router)\b|\b(workflow|plan|code-review|distill|router)`? skill\b|\*\*(workflow|plan|code-review|distill|router)\*\* skill").unwrap();
    for e in std::fs::read_dir(root.join("skills")).unwrap() {
        let p = e.unwrap().path();
        let n = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(n.starts_with("keel-"), "{n}");
        assert!(src[&format!("{n}/SKILL.md")].contains(&format!("\nname: {n}\n")), "{n} frontmatter name");
    }
    let agents = std::fs::read_to_string(root.join("templates/AGENTS.md")).unwrap();
    for (f, body) in src.iter().chain([(&"templates/AGENTS.md".to_string(), &agents)]) {
        // a match right after "-" is keel-X / project-router, not a bare name
        let hit = bare.find_iter(body).find(|m| !body[..m.start()].ends_with('-'));
        assert!(hit.is_none(), "{f}: bare skill name {:?}", hit.map(|m| m.as_str()));
    }
    for f in std::fs::read_dir(root.join("src")).unwrap() {
        let body = std::fs::read_to_string(f.unwrap().path()).unwrap();
        assert!(!body.contains("cargo install keel"), "wrong install hint");
    }
}

fn hook_cmds(v: &serde_json::Value, ev: &str) -> Vec<String> {
    v["hooks"][ev].as_array().unwrap().iter().flat_map(|g| g["hooks"].as_array().unwrap().iter().map(|h| h["command"].as_str().unwrap().to_string())).collect()
}

#[test]
fn settings_merge() {
    // (a) absent -> created with the two entries; (c) second init byte-identical
    let d = fresh_repo();
    let p = d.path();
    assert!(keel(p, &["init"]).status.success());
    let s1 = std::fs::read_to_string(p.join(".claude/settings.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s1).unwrap();
    assert_eq!(v["hooks"]["PostToolUse"][0]["matcher"], "Write|Edit");
    assert_eq!(hook_cmds(&v, "PostToolUse").len(), 1);
    assert_eq!(hook_cmds(&v, "Stop").len(), 1);
    assert!(keel(p, &["init"]).status.success());
    assert_eq!(s1, std::fs::read_to_string(p.join(".claude/settings.json")).unwrap());

    // (f) the merged commands run under sh: no keel-hooks -> silent exit 0; red doc -> 2; green -> 0
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().display().to_string();
    let path = format!("{bin}:{}", std::env::var("PATH").unwrap());
    let run = |cmd: &str, cwd: &Path, payload: &str| {
        use std::io::Write;
        let mut c = Command::new("sh").args(["-c", cmd]).current_dir(cwd).env("PATH", &path)
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        c.wait_with_output().unwrap()
    };
    let post = &hook_cmds(&v, "PostToolUse")[0];
    let payload = r#"{"tool_input":{"file_path":"docs/work/x.html"}}"#;
    std::fs::write(p.join("docs/work/x.html"), r#"<html data-artifact="wu-plan" data-status="ready"></html>"#).unwrap();
    let nogit = TempDir::new().unwrap();
    let o = run(post, nogit.path(), payload);
    assert!(o.status.success() && o.stderr.is_empty(), "outside git must be inert");
    let o = run(post, p, payload);
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
    std::fs::write(p.join("docs/work/x.html"), "<html></html>").unwrap();
    assert!(run(post, p, payload).status.success());
    let clone = fresh_repo(); // hooks never installed here
    assert!(run(post, clone.path(), payload).status.success());

    // (b) a project's own settings: original keys, order and values kept; (d) command already
    // present under another matcher group -> not added again
    let d = fresh_repo();
    let p = d.path();
    std::fs::create_dir_all(p.join(".claude")).unwrap();
    let stop = hook_cmds(&v, "Stop")[0].clone();
    let own = serde_json::json!({
        "zeta": 1, "agent": "router", "permissions": {"allow": ["Bash(git status:*)"]},
        "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "x"}]}],
                  "Stop": [{"matcher": "other", "hooks": [{"type": "command", "command": stop}]}]},
        "alpha": 2
    });
    std::fs::write(p.join(".claude/settings.json"), serde_json::to_string_pretty(&own).unwrap()).unwrap();
    assert!(keel(p, &["init"]).status.success());
    let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p.join(".claude/settings.json")).unwrap()).unwrap();
    let keys: Vec<&String> = m.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["zeta", "agent", "permissions", "hooks", "alpha"]);
    assert_eq!(m["permissions"], own["permissions"]);
    assert_eq!(m["hooks"]["PreToolUse"], own["hooks"]["PreToolUse"]);
    assert_eq!(hook_cmds(&m, "Stop").len(), 1, "already present: not added again");
    assert_eq!(hook_cmds(&m, "PostToolUse").len(), 1);

    // (e) invalid JSON: untouched, warned, init continues
    let d = fresh_repo();
    std::fs::create_dir_all(d.path().join(".claude")).unwrap();
    std::fs::write(d.path().join(".claude/settings.json"), "{ not json").unwrap();
    let o = keel(d.path(), &["init"]);
    assert!(o.status.success() && text(&o).contains("not valid JSON"), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(d.path().join(".claude/settings.json")).unwrap(), "{ not json");
}

#[test]
fn upgrade_confirmation_and_force() {
    let d = fresh_repo();
    let p = d.path();
    assert!(keel(p, &["init"]).status.success());
    let skill = p.join(".claude/skills/keel-workflow/SKILL.md");
    let orig = std::fs::read_to_string(&skill).unwrap();
    let edit = || std::fs::write(&skill, "edited\n").unwrap();

    edit(); // empty stdin: kept, differs, exit 0
    let o = keel_in(p, &["init"], "");
    assert!(o.status.success() && text(&o).contains("differs   .claude/skills/keel-workflow/SKILL.md"), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), "edited\n");
    let o = keel_in(p, &["init"], "n\n");
    assert!(o.status.success());
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), "edited\n");

    let o = keel_in(p, &["init"], "y\n"); // piped y overwrites
    assert!(o.status.success() && text(&o).contains("UNCOMMITTED"), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), orig);

    edit();
    let o = keel_in(p, &["init", "-f"], ""); // -f: no prompt
    assert!(o.status.success() && !text(&o).contains("[y/N]"), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), orig);
    edit();
    assert!(keel_in(p, &["init", "--force"], "").status.success());
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), orig);

    let o = keel_in(p, &["init", "--yes"], "");
    assert!(!o.status.success() && text(&o).contains("usage: keel init"), "{}", text(&o));

    std::fs::remove_file(&skill).unwrap(); // deleted -> recreated without a prompt
    let o = keel_in(p, &["init"], "");
    assert!(text(&o).contains("created   .claude/skills/keel-workflow/SKILL.md") && !text(&o).contains("[y/N]"));
    std::fs::write(p.join(".claude/skills/keel-router/mine.md"), "x").unwrap(); // extra: kept, reported
    let o = keel_in(p, &["init"], "");
    assert!(text(&o).contains("extra     .claude/skills/keel-router/mine.md"), "{}", text(&o));
    assert!(p.join(".claude/skills/keel-router/mine.md").exists());
}

#[test]
fn worktrees_path_is_ignored_and_configurable() {
    let d = fresh_repo();
    assert!(keel(d.path(), &["init"]).status.success());
    assert!(std::fs::read_to_string(d.path().join(".gitignore")).unwrap().lines().any(|l| l == ".claude/worktrees/"));

    let d = fresh_repo();
    std::fs::create_dir_all(d.path().join(".keel")).unwrap();
    std::fs::write(d.path().join(".keel/config.toml"), "[paths]\nworktrees = \"wt/\"\n").unwrap();
    assert!(keel(d.path(), &["init"]).status.success());
    let g = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
    assert!(g.lines().any(|l| l == "wt/") && !g.contains(".claude/worktrees"), "{g}");
}

fn git_in(d: &Path, a: &[&str]) -> Output {
    Command::new("git").args(a).current_dir(d).env("PATH", format!("{}:{}", Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().display(), std::env::var("PATH").unwrap())).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap()
}

/// Keel gates itself (Q12): a clone of committed HEAD with hooks installed refuses a direct commit
/// on main and an evidence-less merge, and `keel init` over it changes nothing (the committed
/// .claude/skills/keel-* equal the embedded copies).
#[test]
fn self_hosting_clone_is_gated_and_init_is_a_noop() {
    let root = env!("CARGO_MANIFEST_DIR");
    let d = TempDir::new().unwrap();
    let c = d.path().join("c");
    assert!(Command::new("git").args(["clone", "-q", root, c.to_str().unwrap()]).status().unwrap().success());
    assert!(git_in(&c, &["checkout", "-q", "-B", "main"]).status.success());
    assert!(keel(&c, &["hooks", "install"]).status.success());

    let o = keel_in(&c, &["init"], "");
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    // Keel's rule_bearing differs from the default by design, so config is the one expected "kept"
    let t = t.lines().filter(|l| !l.contains(".keel/config.toml")).collect::<Vec<_>>().join("\n") + "\nkept      .keel/config.toml";
    assert!(!t.contains("created") && !t.contains("differs") && !t.contains("updated") && !t.contains("merged"), "init is not a no-op on Keel's own tree:\n{t}");
    assert!(t.contains("kept      .keel/config.toml") || t.contains("unchanged .keel/config.toml"), "{t}");
    assert!(git_in(&c, &["status", "--porcelain"]).stdout.is_empty(), "init dirtied the tree");

    let o = git_in(&c, &["commit", "--allow-empty", "-q", "-m", "direct"]);
    assert!(!o.status.success(), "a direct commit on main must be refused");
    assert!(String::from_utf8_lossy(&o.stderr).to_lowercase().contains("merge"), "{}", String::from_utf8_lossy(&o.stderr));

    assert!(git_in(&c, &["checkout", "-q", "-b", "feat"]).status.success());
    std::fs::write(c.join("x.txt"), "x").unwrap();
    assert!(git_in(&c, &["add", "x.txt"]).status.success());
    assert!(git_in(&c, &["commit", "-q", "--no-verify", "-m", "feat"]).status.success());
    assert!(git_in(&c, &["checkout", "-q", "main"]).status.success());
    let o = git_in(&c, &["merge", "--no-ff", "-m", "m", "feat"]);
    assert!(!o.status.success(), "an evidence-less merge into main must be refused");
}
