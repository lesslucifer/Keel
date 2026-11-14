//! The router's report channel (E-RGUARD.W3): `scripts/router-inbox.sh`, as a worker and the router
//! run it. Every case spawns the shipped script through its process boundary, in its own inbox under
//! a temp dir, so the tests are hermetic and parallel. The 2026-11-14 probe's 27 checks are the
//! contract; the killed watcher and the symlinked consume are the two cases added here.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

mod common;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn script() -> PathBuf {
    root().join("scripts/router-inbox.sh")
}

struct Inbox {
    d: TempDir,
}

impl Inbox {
    fn new() -> Self {
        Inbox { d: TempDir::new().unwrap() }
    }

    fn dir(&self) -> PathBuf {
        self.d.path().join("inbox")
    }

    fn read(&self) -> PathBuf {
        self.dir().join("read")
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = common::cmd("sh");
        c.arg(script()).args(args).env("ROUTER_INBOX_DIR", self.dir()).env("ROUTER_INBOX_POLL", "1");
        c
    }

    /// One short command: its exit code and both streams.
    fn run(&self, args: &[&str]) -> (Option<i32>, String) {
        let o = self.cmd(args).output().unwrap();
        (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, out) = self.run(args);
        assert_eq!(code, Some(0), "{args:?} failed: {out}");
        out
    }

    fn refused(&self, args: &[&str], why: &str) -> String {
        let (code, out) = self.run(args);
        assert_eq!(code, Some(2), "{args:?} was not refused: {out}");
        assert!(out.contains(why), "{args:?} did not say {why:?}: {out}");
        out
    }

    fn init(&self) {
        assert!(self.ok(&["init"]).starts_with("READY dir="));
    }

    fn post(&self, from: &str, kind: &str, text: &str) -> String {
        let out = self.ok(&["post", from, kind, text]);
        out.strip_prefix("POSTED id=").unwrap_or_else(|| panic!("no POSTED id in {out:?}")).trim().to_string()
    }

    fn post_stdin(&self, from: &str, kind: &str, body: &str) -> String {
        let mut c = self.cmd(&["post", from, kind]);
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(body.as_bytes()).unwrap();
        let o = child.wait_with_output().unwrap();
        assert!(o.status.success(), "post failed: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().strip_prefix("POSTED id=").unwrap().trim().to_string()
    }

    /// The watcher's printed line and body.
    fn watch(&self, seconds: &str) -> String {
        self.ok(&["watch", seconds])
    }

    fn armed_watch(&self, seconds: &str, out: Stdio) -> Child {
        self.cmd(&["watch", seconds]).stdout(out).stderr(Stdio::piped()).spawn().unwrap()
    }
}

fn ids(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn git(dir: &Path, args: &[&str]) {
    let o = common::cmd("git").args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// The production path: a copy in a linked worktree resolves the main checkout's inbox, so a
/// worker's post lands where the router watches, whatever directory the script is called from.
#[test]
fn a_worktree_copy_resolves_the_main_checkouts_inbox() {
    let d = TempDir::new().unwrap();
    let main = d.path().join("main");
    std::fs::create_dir_all(main.join("scripts")).unwrap();
    std::fs::copy(script(), main.join("scripts/router-inbox.sh")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "."]);
    git(&main, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "base"]);
    git(&main, &["worktree", "add", "-q", "../wt", "-b", "w"]);
    let wt = d.path().join("wt");
    let run = |at: &Path, args: &[&str]| {
        let o = common::cmd("sh").arg(at.join("scripts/router-inbox.sh")).args(args).output().unwrap();
        (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
    };
    let (code, out) = run(&wt, &["init"]);
    assert_eq!(code, Some(0), "{out}");
    let main = main.canonicalize().unwrap();
    let inbox = format!("READY dir={}/.router/inbox", main.display());
    assert!(out.starts_with(&inbox), "the worktree copy did not name the main checkout's inbox: {out:?}");
    let (code, out) = run(&main, &["post", "worker", "REPORT", "from the router's checkout"]);
    assert_eq!(code, Some(0), "{out}");
    let id = out.trim().strip_prefix("POSTED id=").unwrap().to_string();
    let (code, out) = run(&wt, &["watch", "2"]);
    assert_eq!(code, Some(0), "a worktree copy did not read the main checkout's inbox: {out}");
    assert!(out.contains(&format!("MSG id={id} from=worker kind=REPORT")), "{out:?}");
    let (code, out) = run(&main, &["consume", &id]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.trim() == format!("CONSUMED id={id}"), "{out:?}");
}

#[test]
fn watch_before_init_exits_2_and_names_the_inbox() {
    let b = Inbox::new();
    let out = b.refused(&["watch", "1"], "NO-INBOX");
    assert!(out.contains(".router/inbox") || out.contains("router-inbox.sh init"), "no fix in {out:?}");
}

#[test]
fn init_creates_the_inbox_and_the_read_directory() {
    let b = Inbox::new();
    let out = b.ok(&["init"]);
    assert!(out.starts_with("READY dir="), "{out:?}");
    assert!(b.dir().is_dir() && b.read().is_dir(), "init did not create both directories");
}

#[test]
fn post_prints_an_id_and_the_next_watch_delivers_it_at_once() {
    let b = Inbox::new();
    b.init();
    let id = b.post("alice", "REPORT", "hello world");
    assert!(!id.is_empty(), "empty id");
    let t = Instant::now();
    let out = b.watch("5");
    assert!(t.elapsed() < Duration::from_secs(2), "the first check was not immediate: {:?}", t.elapsed());
    assert!(out.contains(&format!("MSG id={id} from=alice kind=REPORT age=")), "{out:?}");
    assert!(out.contains("  hello world"), "body not indented under the header: {out:?}");
}

#[test]
fn consume_acks_once_and_then_says_already() {
    let b = Inbox::new();
    b.init();
    let id = b.post("alice", "NOTE", "one");
    assert_eq!(b.ok(&["consume", &id]).trim(), format!("CONSUMED id={id}"));
    assert_eq!(b.ok(&["consume", &id]).trim(), format!("CONSUMED-ALREADY id={id}"));
    assert!(b.read().join(format!("{id}.msg")).is_file(), "the consumed message is not in read/");
    assert!(!b.dir().join(format!("{id}.msg")).exists(), "the consumed message is still in the inbox");
}

#[test]
fn an_empty_or_consumed_inbox_times_out_with_exit_1() {
    let b = Inbox::new();
    b.init();
    let (code, out) = b.run(&["watch", "1"]);
    assert_eq!(code, Some(1), "an empty inbox did not exit 1: {out}");
    assert!(out.contains("TIMEOUT") && out.contains("kind=inbox"), "{out:?}");
    let id = b.post("bob", "DONE", "bye");
    b.watch("1");
    b.ok(&["consume", &id]);
    let (code, out) = b.run(&["watch", "1"]);
    assert_eq!(code, Some(1), "a consumed message came back: {out}");
}

#[test]
fn status_counts_pending_read_and_orphan_dotfiles() {
    let b = Inbox::new();
    b.init();
    let id = b.post("carol", "BLOCKED", "waiting");
    std::fs::write(b.dir().join(".new.stale"), "unfinished post\n").unwrap();
    let out = b.ok(&["status"]);
    assert!(out.contains("pending=1") && out.contains(&format!("oldest={id}")), "{out:?}");
    assert!(out.contains("orphans=1"), "{out:?}");
    b.ok(&["consume", &id]);
    let out = b.ok(&["status"]);
    assert!(out.contains("pending=0") && out.contains("read=1") && out.contains("orphans=1"), "{out:?}");
}

#[test]
fn two_concurrent_posts_both_land_whole_with_distinct_ids() {
    let b = Inbox::new();
    b.init();
    let a = b.cmd(&["post", "bob", "NOTE", "one"]).stdout(Stdio::piped()).spawn().unwrap();
    let c = b.cmd(&["post", "carol", "NOTE", "two"]).stdout(Stdio::piped()).spawn().unwrap();
    let ida = String::from_utf8(a.wait_with_output().unwrap().stdout).unwrap().trim().strip_prefix("POSTED id=").unwrap().to_string();
    let idb = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap().trim().strip_prefix("POSTED id=").unwrap().to_string();
    assert_ne!(ida, idb, "two writers collided on one id");
    let out = b.ok(&["status"]);
    assert!(out.contains("pending=2"), "{out:?}");
    for id in ids(vec![ida, idb]) {
        let out = b.watch("1");
        assert!(out.contains(&format!("MSG id={id} ")), "not delivered: {out:?}");
        b.ok(&["consume", &id]);
    }
}

#[test]
fn watch_delivers_the_oldest_id_first() {
    let b = Inbox::new();
    b.init();
    let first = b.post("dave", "NOTE", "first");
    let second = b.post("erin", "NOTE", "second");
    for id in ids(vec![first, second]) {
        let out = b.watch("1");
        assert!(out.contains(&format!("MSG id={id} ")), "wrong order: {out:?}");
        b.ok(&["consume", &id]);
    }
}

#[test]
fn post_refuses_an_oversized_body_before_any_file_exists() {
    let b = Inbox::new();
    b.init();
    let big = "x".repeat(4001);
    b.refused(&["post", "alice", "NOTE", &big], "limit is 4000");
    let left: Vec<String> = std::fs::read_dir(b.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".msg") || n.starts_with(".new."))
        .collect();
    assert!(left.is_empty(), "an oversized post left {left:?}");
}

#[test]
fn post_refuses_a_sender_that_is_not_a_name_and_an_unknown_kind() {
    let b = Inbox::new();
    b.init();
    b.refused(&["post", "../evil", "NOTE", "hi"], "from must be a worker name");
    b.refused(&["post", "alice", "report", "hi"], "kind must be");
    b.refused(&["post", "alice", "NOTE", "-x"], "may not start with -");
    let long = "a".repeat(65);
    b.refused(&["post", &long, "NOTE", "hi"], "longer than 64");
    b.refused(&["post", "alice", "NOTE"], "empty body");
}

#[test]
fn consume_refuses_an_id_that_is_not_an_id() {
    let b = Inbox::new();
    b.init();
    b.refused(&["consume", "../x"], "bad id");
    b.refused(&["consume", "a/b"], "bad id");
    let (code, out) = b.run(&["consume", "1794659383-nosuch"]);
    assert_eq!(code, Some(2), "{out}");
    assert!(out.contains("MISSING"), "{out:?}");
}

#[test]
fn watch_shows_a_control_character_as_a_question_mark() {
    let b = Inbox::new();
    b.init();
    let id = b.post("mallory", "NOTE", "safe\u{1b}[31mtext");
    let out = b.watch("1");
    assert!(out.contains("safe?[31mtext"), "{out:?}");
    assert!(!out.contains('\u{1b}'), "a raw ESC reached the terminal: {out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn an_armed_watcher_wakes_on_a_live_post() {
    let b = Inbox::new();
    b.init();
    let child = b.armed_watch("30", Stdio::piped());
    std::thread::sleep(Duration::from_millis(200));
    let id = b.post("frank", "QUESTION", "are you there");
    let o = child.wait_with_output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success(), "{out}");
    assert!(out.contains(&format!("MSG id={id} from=frank kind=QUESTION")), "{out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn post_reads_a_multi_line_body_from_stdin() {
    let b = Inbox::new();
    b.init();
    let id = b.post_stdin("gina", "DONE", "line one\nline two\n");
    let out = b.watch("1");
    assert!(out.contains(&format!("MSG id={id}")), "{out:?}");
    assert!(out.contains("  line one") && out.contains("  line two"), "{out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn watch_skips_a_symlink_and_a_forged_header_without_following_either() {
    let b = Inbox::new();
    b.init();
    let secret = b.d.path().join("secret.txt");
    std::fs::write(&secret, "SECRET-NOT-A-MESSAGE\n").unwrap();
    std::os::unix::fs::symlink(&secret, b.dir().join("0000000001-evil.msg")).unwrap();
    std::fs::write(b.dir().join("0000000002-forged.msg"), "forged\n").unwrap();
    let id = b.post("hank", "NOTE", "real message after junk");
    let out = b.watch("1");
    assert!(out.contains(&format!("MSG id={id}")), "the real message was blocked: {out:?}");
    assert!(!out.contains("SECRET"), "the symlink target was printed: {out:?}");
    assert!(!out.contains("forged"), "the forged header was printed: {out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn watch_skips_a_message_whose_header_id_does_not_match_its_file_name() {
    let b = Inbox::new();
    b.init();
    std::fs::write(
        b.dir().join("1700000000-mismatch.msg"),
        "id: 1700000001-other\nfrom: x\nkind: NOTE\nwhen: now\n---\nnot this one\n",
    )
    .unwrap();
    let id = b.post("iris", "NOTE", "the real one");
    let out = b.watch("1");
    assert!(out.contains(&format!("MSG id={id}")), "{out:?}");
    assert!(!out.contains("not this one"), "{out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn watch_bounds_a_long_body_to_eighty_lines_and_names_the_file() {
    let b = Inbox::new();
    b.init();
    let body: String = (1..=90).map(|n| format!("{n}\n")).collect();
    let id = b.post_stdin("jane", "NOTE", &body);
    let out = b.watch("1");
    assert!(out.contains(&format!("MSG id={id}")), "{out:?}");
    assert!(out.contains("  80\n"), "the 80th line is missing: {out:?}");
    assert!(!out.contains("  81\n"), "a line past the bound was printed: {out:?}");
    assert!(out.contains("more body lines") && out.contains(&format!("{id}.msg")), "{out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn watch_delivers_from_an_unwritable_inbox_directory() {
    let b = Inbox::new();
    b.init();
    let id = b.post("karl", "NOTE", "delivered from a read-only directory");
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o500);
    std::fs::set_permissions(b.dir(), mode).unwrap();
    let out = b.ok(&["watch", "1"]);
    std::fs::set_permissions(b.dir(), std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    assert!(out.contains(&format!("MSG id={id}")), "{out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn a_killed_watcher_loses_nothing() {
    let b = Inbox::new();
    b.init();
    let mut child = b.armed_watch("30", Stdio::piped());
    std::thread::sleep(Duration::from_millis(200));
    child.kill().unwrap();
    child.wait().unwrap();
    let id = b.post("laura", "REPORT", "posted after the watcher died");
    let out = b.watch("1");
    assert!(out.contains(&format!("MSG id={id}")), "{out:?}");
    b.ok(&["consume", &id]);
}

#[test]
fn consume_refuses_a_symlink_in_the_inbox_instead_of_moving_it() {
    let b = Inbox::new();
    b.init();
    let secret = b.d.path().join("secret.txt");
    std::fs::write(&secret, "SECRET-NOT-A-MESSAGE\n").unwrap();
    let id = "1794659383-evil".to_string();
    let link = b.dir().join(format!("{id}.msg"));
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let (code, out) = b.run(&["consume", &id]);
    assert_eq!(code, Some(2), "{out}");
    assert!(out.contains("MISSING"), "{out:?}");
    assert!(!out.contains("SECRET") && !out.contains("CONSUMED"), "{out:?}");
    assert!(link.is_symlink() && !b.read().join(format!("{id}.msg")).exists(), "consume moved the symlink");
}
