use std::collections::BTreeMap;
use std::path::Path;
use std::process::Output;
use tempfile::TempDir;

mod common;

fn keel(d: &Path, a: &[&str]) -> Output {
    common::cmd(env!("CARGO_BIN_EXE_keel")).args(a).current_dir(d).output().unwrap()
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
    assert!(common::cmd("git").args(["init", "-q", "-b", "main"]).current_dir(p).status().unwrap().success());
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
    // E-DSH.W1: one shared skill root - a relative link, so Claude Code and DSH read one copy
    assert_eq!(after_first.get(".agents/skills").map(String::as_str), Some("-> ../.claude/skills"), "init must plant .agents/skills as a relative link");
    assert_eq!(std::fs::read_link(p.join(".agents/skills")).unwrap(), Path::new("../.claude/skills"));
    assert!(after_first[".keel/config.toml"].contains("test = \"pytest -q\""));
    let hooks = common::cmd("git").args(["config", "core.hooksPath"]).current_dir(p).output().unwrap();
    assert!(String::from_utf8_lossy(&hooks.stdout).trim().ends_with("keel-hooks"));
    // the written config loads
    assert!(keel(p, &["verify-doc"]).status.success());
    // the Codex PostToolUse command reaches the installed hook script: a red doc -> exit 2
    std::fs::write(p.join("docs/work/x.html"), r#"<html data-artifact="wu-plan" data-status="ready"></html>"#).unwrap();
    let hooks_json: serde_json::Value = serde_json::from_str(&after_first[".codex/hooks.json"]).unwrap();
    let cmd = hooks_json["hooks"]["PostToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    let bin = Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().display().to_string();
    let mut sh = common::cmd("sh")
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

/// E-DSH.W1 (UC-INIT-1): init plants one shared skill root, `.agents/skills` a relative link to
/// `.claude/skills`, so a host that reads `.agents` and one that reads `.claude` read one copy. A
/// second init changes nothing and a worktree checkout carries the link.
#[test]
fn init_plants_one_relative_skill_root_a_worktree_carries() {
    let d = fresh_repo();
    let p = d.path();
    let first = keel(p, &["init"]);
    let t = text(&first);
    assert!(first.status.success(), "{t}");
    for plugin in ["dsh-auto-review", "dsh-worker-workspace", "dsh-doc-gate"] {
        assert!(t.contains(&format!("dsh plugin --profile <name> add <path>/{plugin}")), "the hand-off misses {plugin}:\n{t}");
    }
    assert!(t.contains("never in the project"), "the hand-off says where a plugin lives:\n{t}");

    let link = p.join(".agents/skills");
    assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("../.claude/skills"), "the shared root is the one relative link");
    let through = std::fs::read_to_string(link.join("keel-workflow/SKILL.md")).unwrap();
    assert_eq!(through, std::fs::read_to_string(p.join(".claude/skills/keel-workflow/SKILL.md")).unwrap(), "the link reads the one copy, not a second");

    let second = keel(p, &["init"]);
    assert!(second.status.success(), "{}", text(&second));
    assert!(text(&second).contains("unchanged .agents/skills"), "{}", text(&second));
    assert!(!text(&second).contains("created"), "{}", text(&second));

    assert!(git_in(p, &["add", "-A"]).status.success());
    assert!(git_in(p, &["commit", "-q", "--no-verify", "-m", "keel init"]).status.success());
    let w = d.path().join("w");
    let o = git_in(p, &["worktree", "add", "-q", "-b", "wu/e/x", w.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_link(w.join(".agents/skills")).unwrap(), Path::new("../.claude/skills"));
    assert!(w.join(".agents/skills/keel-workflow/SKILL.md").is_file(), "a worktree checkout carries the shared root");
}

/// E-DSH.W1: init never clobbers a project's own `.agents/skills` - a real directory or a link to
/// somewhere else is kept and reported, with how to take the shared root.
#[test]
fn a_project_owned_agents_skills_is_kept_and_reported() {
    let d = fresh_repo();
    let p = d.path();
    std::fs::create_dir_all(p.join(".agents/skills")).unwrap();
    std::fs::write(p.join(".agents/skills/mine.md"), "mine").unwrap();
    let o = keel(p, &["init"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("kept      .agents/skills") && t.contains("real directory"), "{t}");
    assert_eq!(std::fs::read_to_string(p.join(".agents/skills/mine.md")).unwrap(), "mine");
    let again = keel(p, &["init"]);
    assert!(text(&again).contains("kept      .agents/skills"), "a second run still reports the kept root: {}", text(&again));

    std::fs::remove_dir_all(p.join(".agents/skills")).unwrap();
    std::os::unix::fs::symlink("../skills", p.join(".agents/skills")).unwrap();
    let o = keel(p, &["init"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("kept      .agents/skills") && t.contains("../skills"), "{t}");
    assert_eq!(std::fs::read_link(p.join(".agents/skills")).unwrap(), Path::new("../skills"), "the project's link is untouched");
}

/// E-DSH.W1: the link is relative, so init plants it only under a real `.agents` at the repo root.
/// A symlinked `.agents` (here to a sibling directory) is kept as found: init writes nothing inside
/// the target, says so, and exits clean.
#[test]
fn a_symlinked_agents_directory_is_kept_and_its_target_untouched() {
    let d = TempDir::new().unwrap();
    let shared = d.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let p = d.path().join("repo");
    std::fs::create_dir_all(&p).unwrap();
    assert!(common::cmd("git").args(["init", "-q", "-b", "main"]).current_dir(&p).status().unwrap().success());
    std::os::unix::fs::symlink("../shared", p.join(".agents")).unwrap();
    let before = snapshot(&shared);

    let o = keel(&p, &["init"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("kept      .agents/skills") && t.contains("symlink -> ../shared"), "{t}");
    assert!(!t.contains("created   .agents"), "init must not claim it planted the link: {t}");
    assert_eq!(std::fs::read_link(p.join(".agents")).unwrap(), Path::new("../shared"), "the symlinked .agents is untouched");
    assert_eq!(before, snapshot(&shared), "init wrote inside the symlink's target");

    let again = keel(&p, &["init"]);
    assert!(text(&again).contains("kept      .agents/skills"), "a second run still reports the kept root: {}", text(&again));
}

/// E-DSH.W1: `.agents` as a regular file is the other non-directory case - kept, reported, no link.
#[test]
fn a_regular_file_at_agents_is_kept_and_reported() {
    let d = fresh_repo();
    let p = d.path();
    std::fs::write(p.join(".agents"), "mine\n").unwrap();
    let o = keel(p, &["init"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("kept      .agents/skills") && t.contains("regular file"), "{t}");
    assert!(!t.contains("created   .agents"), "{t}");
    assert_eq!(std::fs::read_to_string(p.join(".agents")).unwrap(), "mine\n", "the file is untouched");
    assert!(std::fs::symlink_metadata(p.join(".agents/skills")).is_err(), "no link is planted through a file");
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
    assert!(common::cmd("git").args(["init", "-q", "-b", "main"]).current_dir(d.path()).status().unwrap().success());
    d
}

fn keel_in(d: &Path, a: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut c = common::cmd(env!("CARGO_BIN_EXE_keel"))
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
    // what Claude Code needs to list and trigger a skill: a closed frontmatter block with name and
    // a one-line description of at most 1024 chars (project-router's template included)
    for f in src.keys().filter(|f| f.ends_with("/SKILL.md") || f.ends_with("project-router-template.md")) {
        let fm = src[f].strip_prefix("---\n").and_then(|b| b.split_once("\n---\n")).map(|(fm, _)| fm);
        let fm = fm.unwrap_or_else(|| panic!("{f}: no closed --- frontmatter at the top"));
        let field = |k: &str| fm.lines().find_map(|l| l.strip_prefix(k)).map(str::trim).unwrap_or("");
        assert!(!field("name:").is_empty(), "{f}: frontmatter has no name");
        let d = field("description:");
        // a block scalar (`>` / `|`) would hide the real description from the length check below
        assert!(!d.starts_with(['>', '|']), "{f}: description must be a one-line scalar, not a block scalar");
        assert!(!d.is_empty() && d.len() <= 1024, "{f}: frontmatter description empty or over 1024 chars ({})", d.len());
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

/// E-DOCSTYLE.W1: `keel init` ships the writing skill and its example; the example is a ready epic
/// plan the doc check passes; every place that tells an agent how to write a doc points to it.
#[test]
fn writing_skill_ships_with_a_passing_example_and_is_pointed_to() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let d = fresh_repo();
    let p = d.path();
    assert!(keel(p, &["init"]).status.success());
    let skill = std::fs::read_to_string(p.join(".claude/skills/keel-write/SKILL.md")).expect("keel init did not install keel-write");
    assert!(skill.contains("references/example.html"), "SKILL.md does not name its example");

    let example = std::fs::read_to_string(p.join(".claude/skills/keel-write/references/example.html")).expect("keel init did not install the example");
    let head = &example[example.find("<html").expect("no <html> tag")..];
    let head = &head[..head.find('>').unwrap()];
    assert!(head.contains(r#"data-artifact="epic-plan""#) && head.contains(r#"data-status="ready""#), "the example is not a ready epic plan: {head}");
    std::fs::write(p.join("docs/work/example-plan.html"), &example).unwrap();
    let o = keel(p, &["verify-doc", "docs/work/example-plan.html"]);
    assert!(o.status.success(), "the example fails the doc check: {}", text(&o));

    for f in ["skills/keel-plan/SKILL.md", "skills/keel-distill/SKILL.md", "AGENTS.md", "templates/AGENTS.md"] {
        let body = std::fs::read_to_string(root.join(f)).unwrap();
        assert!(body.contains("skill `keel-write`"), "{f} does not point to skill `keel-write`");
        assert!(!body.contains("AI readers") && !body.contains("target an AI reader"), "{f} still writes docs for AI readers only");
    }
    assert!(std::fs::read_to_string(p.join("AGENTS.md")).unwrap().contains("skill `keel-write`"), "init's AGENTS.md does not name the skill");
}

/// E-DSH.W2: the doc check runs at the write on every host, not at a review or a merge; a red
/// doc returns the problem lines, and no shipped skill makes Claude Code (or another host) the
/// gate's trigger.
#[test]
fn shipped_skills_put_the_doc_check_at_the_write_and_name_no_required_host() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = std::fs::read_to_string(root.join("skills/keel-workflow/SKILL.md")).unwrap();
    let plan = std::fs::read_to_string(root.join("skills/keel-plan/SKILL.md")).unwrap();

    // the write-time contract, the problem feedback and the no-substitute rule
    for (f, body) in [("keel-workflow", workflow.to_ascii_lowercase()), ("keel-plan", plan.to_ascii_lowercase())] {
        for said in ["problem lines", "no substitute"] {
            assert!(body.contains(said), "{f} does not say {said:?}");
        }
    }
    assert!(workflow.to_ascii_lowercase().contains("at the write"), "keel-workflow does not put the doc check at the write");
    assert!(plan.to_ascii_lowercase().contains("trigger is the write"), "keel-plan does not make the write the trigger");
    // a host without shell hooks runs the same checks through a plugin; it does not drop them
    let wf = workflow.to_ascii_lowercase();
    assert!(wf.contains("no shell hooks") && wf.contains("host plugin"), "keel-workflow has no plugin row for a host without shell hooks");
    // the plan skill stops naming one host's hook scripts as the trigger
    for hook in ["verify-doc-post", "verify-doc-stop", "PostToolUse", "Stop hook"] {
        assert!(!plan.contains(hook), "keel-plan still names the {hook} trigger");
    }
    // no shipped skill claims the doc gate needs Claude Code
    let mut files = Vec::new();
    md_files(&root.join("skills"), &mut files);
    for f in &files {
        let body = std::fs::read_to_string(f).unwrap();
        for line in body.lines() {
            let l = line.to_ascii_lowercase();
            let about_check = ["verify-doc", "doc check", "doc gate"].iter().any(|k| l.contains(k));
            assert!(!(about_check && l.contains("claude")), "{}: the doc check names Claude Code: {line}", f.display());
        }
    }
    for (f, body) in [("keel-workflow", &workflow), ("keel-plan", &plan)] {
        assert!(!body.to_ascii_lowercase().contains("claude"), "{f} still names Claude Code");
    }
}

/// UC-ROUTE-4 (E-ROUTER.W4): the router skill ships exactly the references SKILL.md names,
/// including the three recovered from Trawler's trawler-router (W2) and a Keel lessons section
/// (W4); no Trawler-only token appears outside lineage. Capitalised "Trawler" is allowed: it is
/// the source project's name in citations. "(Trawler: …)" evidence, "Trawler's `trawler-router/…`"
/// source pointers and lessons.md's Trawler sections are lineage and are not scanned.
#[test]
fn router_skill_is_complete() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("skills/keel-router");
    let skill = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
    let named: std::collections::BTreeSet<String> =
        regex::Regex::new(r"references/([a-z0-9-]+\.md)").unwrap().captures_iter(&skill).map(|c| c[1].to_string()).collect();
    let shipped: std::collections::BTreeSet<String> =
        std::fs::read_dir(dir.join("references")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(named, shipped, "SKILL.md names a missing reference (left only) or a reference is orphaned (right only)");
    for r in ["brief-skeleton.md", "commands.md", "review-checklists.md", "lessons.md", "messages.md", "worker-ops.md"] {
        assert!(shipped.contains(r), "references/{r} missing");
    }
    let lessons = std::fs::read_to_string(dir.join("references/lessons.md")).unwrap();
    let keel = lessons.split_once("\n## K. Keel").map(|(_, k)| k.split("\n## ").next().unwrap()).expect("lessons.md has no '## K. Keel' section");
    assert!(regex::Regex::new(r"\n- K\d").unwrap().find_iter(keel).count() >= 5, "Keel section has fewer than 5 lessons");

    let token = regex::Regex::new(r"(?i:xtask|tauri)|\btrawler|\b(?:7420|7421|4445)\b").unwrap();
    let lineage = regex::Regex::new(r"\(Trawler[^)\n]*\)|Trawler's `trawler-router/[^`\n]*`").unwrap();
    let mut src = BTreeMap::new();
    files(&root.join("skills"), &root.join("skills"), &mut src);
    src.insert("keel-router/references/lessons.md".into(), keel.to_string());
    let keel_state = regex::Regex::new(r"(?:epic|wu)/E-ROUTER|\b2d7e18d\b|scripts/router-worker\.sh").unwrap();
    for (f, body) in &src {
        let scrubbed = lineage.replace_all(body, "");
        let hit = token.find(&scrubbed);
        assert!(hit.is_none(), "{f}: Trawler-only token {:?} outside lineage", hit.map(|m| m.as_str()));
        // Keel's own state (its epic branches, its commits) goes stale in a shipped skill: lessons.md only
        if !f.ends_with("lessons.md") {
            let hit = keel_state.find(body);
            assert!(hit.is_none(), "{f}: Keel-only fact {:?} in the shipped skill (belongs in project-router or lessons)", hit.map(|m| m.as_str()));
        }
    }
}

/// E-DSH.W3: the router skill carries the DSH transport, report and persona rules in one dated
/// host table - every mechanism, every DSH worker tool - and the project-router template gains the
/// harness, permission-preset and brief slots; the description no longer claims Claude Code only.
/// DSH's host facts stay in this skill, and both shipped copies stay byte-identical.
#[test]
fn router_skill_carries_the_dsh_transport_and_the_template_its_slots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("skills/keel-router");
    let skill = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();

    let fm = skill.strip_prefix("---\n").and_then(|b| b.split_once("\n---\n")).map(|(fm, _)| fm).expect("SKILL.md has no closed frontmatter");
    assert!(!fm.to_ascii_lowercase().contains("claude code only"), "the description still says Claude Code only");

    let start = skill.find("### Transport by host").expect("SKILL.md has no transport table");
    let table = &skill[start..start + skill[start..].find("\n\n- ").unwrap_or(skill.len() - start)];
    for mechanism in ["Dispatch", "Follow-up", "Report", "Fleet list", "Status", "Stop", "Permission preset"] {
        assert!(table.contains(&format!("| {mechanism} |")), "the transport table has no {mechanism} row");
    }
    for tool in ["worker_spawn", "worker_send", "worker_output", "worker_list", "worker_status", "worker_stop"] {
        assert!(table.contains(&format!("`{tool}`")), "the transport table never names {tool}");
    }
    assert!(table.contains("Claude Code") && table.contains("DSH") && table.contains("re-check"), "the table is not per-host, or says nothing about re-checking the host's tool list");

    assert!(skill.contains("`worker_output` at every wake and before every ask"), "SKILL.md does not read a DSH report from the transcript");
    assert!(skill.contains("A DSH subagent is not a worker"), "SKILL.md does not keep the worker-vs-subagent rule");
    assert!(skill.contains("no project-scoped agent definition") && skill.contains("per-worker preset"), "SKILL.md does not state the DSH persona");
    assert!(skill.to_ascii_lowercase().contains("posture is unenforced"), "SKILL.md implies a guard where DSH has none");

    let template = std::fs::read_to_string(dir.join("references/project-router-template.md")).unwrap();
    for slot in ["| Harness |", "| Permission preset |", "| Brief location |"] {
        assert!(template.contains(slot), "the template has no {slot} slot");
    }

    let tools = ["worker_spawn", "worker_send", "worker_output", "worker_list", "worker_status", "worker_stop"];
    let mut files = Vec::new();
    md_files(&root.join("skills"), &mut files);
    for f in &files {
        if f.starts_with(&dir) {
            continue;
        }
        let body = std::fs::read_to_string(f).unwrap();
        for tool in tools {
            assert!(!body.contains(tool), "{} names the DSH tool {tool} outside the router skill", f.display());
        }
        assert!(!body.contains("DSH") && !body.contains("DeepSeek"), "{} names the DSH host outside the router skill", f.display());
    }

    for f in ["SKILL.md", "references/project-router-template.md"] {
        let self_hosting = std::fs::read_to_string(root.join(".claude/skills/keel-router").join(f)).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(f)).unwrap(), self_hosting, "skills/keel-router/{f} and .claude/skills/keel-router/{f} differ");
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
        let mut c = common::cmd("sh").args(["-c", cmd]).current_dir(cwd).env("PATH", &path)
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

/// I10: init over another hook manager writes its files, then stops on the hooks refusal.
#[test]
fn init_over_a_foreign_hooks_path_writes_its_files_then_refuses_the_hooks() {
    let d = fresh_repo();
    let p = d.path();
    assert!(common::cmd("git").args(["config", "core.hooksPath", ".husky"]).current_dir(p).status().unwrap().success());
    let o = keel(p, &["init"]);
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(p.join(".keel/config.toml").exists() && p.join("AGENTS.md").exists(), "{t}");
    assert!(t.contains(".husky") && t.contains("keel hooks install --force"), "{t}");
    let hooks = common::cmd("git").args(["config", "core.hooksPath"]).current_dir(p).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&hooks.stdout).trim(), ".husky");
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
    common::cmd("git").args(a).current_dir(d).env("PATH", format!("{}:{}", Path::new(env!("CARGO_BIN_EXE_keel")).parent().unwrap().display(), std::env::var("PATH").unwrap())).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap()
}

use common::self_clone;

/// `keel init` over a self-clone changes nothing: its .claude/skills/keel-* equal the embedded copies.
fn assert_init_is_a_noop(c: &Path) {
    assert!(keel(c, &["hooks", "install"]).status.success());
    let o = keel_in(c, &["init"], "");
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    // Keel's rule_bearing differs from the default by design, and its project-router is filled in
    // (E-ROUTER.W3), so those two are the expected "kept"; every other status line (after the
    // header, up to "hooks installed") is unchanged
    let kept = ["  kept      .keel/config.toml", "  kept      .claude/skills/project-router/SKILL.md"];
    for l in t.lines().skip(1).take_while(|l| !l.starts_with("hooks installed")) {
        assert!(l.starts_with("  unchanged") || kept.iter().any(|k| l.starts_with(k)), "init is not a no-op on Keel's own tree: {l:?}\n{t}");
    }
    assert!(t.contains("hooks installed"), "{t}");
    assert!(kept.iter().all(|k| t.contains(k)), "{t}");
    assert!(git_in(c, &["status", "--porcelain"]).stdout.is_empty(), "init dirtied the tree");
}

/// Keel gates itself (Q12): a clone of the source's current tree with hooks installed refuses a
/// direct commit on main and an evidence-less merge, and `keel init` over it is a no-op.
#[test]
fn self_hosting_clone_is_gated_and_init_is_a_noop() {
    let d = TempDir::new().unwrap();
    let c = d.path().join("c");
    self_clone(Path::new(env!("CARGO_MANIFEST_DIR")), &c);
    assert_init_is_a_noop(&c);

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

/// E-SEED2.W14: the pre-merge-commit check runs before the merge commit exists, so HEAD still
/// holds the old skills while the build embeds the merged ones; the self-clone must follow the
/// merged index, not HEAD.
#[test]
fn self_hosting_check_passes_mid_merge_of_a_skill_change() {
    let d = TempDir::new().unwrap();
    let s = d.path().join("s");
    self_clone(Path::new(env!("CARGO_MANIFEST_DIR")), &s);
    let skill = s.join(".claude/skills/keel-workflow/SKILL.md");
    let current = std::fs::read_to_string(&skill).unwrap();
    std::fs::write(&skill, format!("{current}stale\n")).unwrap();
    assert!(git_in(&s, &["commit", "-qam", "stale skill"]).status.success());
    assert!(git_in(&s, &["checkout", "-q", "-b", "skill-update"]).status.success());
    std::fs::write(&skill, current).unwrap();
    assert!(git_in(&s, &["commit", "-qam", "skill update"]).status.success());
    assert!(git_in(&s, &["checkout", "-q", "main"]).status.success());
    assert!(git_in(&s, &["merge", "--no-ff", "--no-commit", "skill-update"]).status.success());

    let c = d.path().join("c");
    self_clone(&s, &c);
    assert_init_is_a_noop(&c);
}

/// E-ROUTER3.W4 (DEC-47): a new worktree gets Keel's signer roster by checkout, as committed; the
/// main checkout's local `.mr-allowed` is no longer copied into it.
#[test]
fn self_hosting_new_worktree_reads_the_committed_roster_and_copies_no_local_one() {
    let d = TempDir::new().unwrap();
    let c = d.path().join("c");
    self_clone(Path::new(env!("CARGO_MANIFEST_DIR")), &c);
    assert!(keel(&c, &["hooks", "install"]).status.success());
    std::fs::write(c.join(".mr-allowed"), "username: owner\n").unwrap();
    let committed = String::from_utf8(git_in(&c, &["show", "HEAD:.keel/mr-allowed"]).stdout).unwrap();
    assert!(!committed.trim().is_empty(), "Keel commits a roster");

    let w = d.path().join("w");
    assert!(git_in(&c, &["worktree", "add", "-q", "-b", "wu/x/y", w.to_str().unwrap()]).status.success());
    assert_eq!(std::fs::read_to_string(w.join(".keel/mr-allowed")).unwrap(), committed);
    assert!(!w.join(".mr-allowed").exists(), "no local roster is copied");
}
