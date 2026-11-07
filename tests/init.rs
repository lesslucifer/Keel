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

    for f in [".keel/config.toml", "AGENTS.md", ".codex/hooks.json", ".claude/skills/project-router/SKILL.md", "docs/work"] {
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
