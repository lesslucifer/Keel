//! E-GHFLOW.W2: scripts/router-gh.sh, the router bot's one door to GitHub, at its real boundary: a
//! scratch repository holding a copy of the script and a committed `[github] repo`, Keel's hooks
//! installed, a bare "origin" reached as GitHub's git URL would be (`ROUTER_GH_GIT=file://…`), and a
//! fake curl standing for GitHub's API (`ROUTER_GH_CURL`): it records each request the script sends
//! on its stdin and answers from a route table. Worker sandboxes refuse every socket, so the mock is
//! the client side, not a server. The App key is a throwaway RSA key the test makes in a temp home;
//! no real key or token is ever read.
//! Needs `sh`, `git`, `jq`, `openssl`, `iconv` and (one test) `curl` on PATH, as the script does.
compiled!();

use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

use crate::common;

const KEEL: &str = env!("CARGO_BIN_EXE_keel");
const TOKEN: &str = "ghs_routerGhTestToken0123456789abcdefXYZ";
const BOT: &str = "keel-router[bot]";

/// GitHub's API as the script reaches it: the whole request comes as a curl config on stdin.
/// Routes: `METHOD PATH STATUS FILE` lines in $MOCK_GH/routes (first match; none is a 404).
const FAKE_CURL: &str = r#"#!/bin/sh
set -eu
[ $# -eq 2 ] && [ "$1" = --config ] && [ "$2" = - ] || { echo "fake curl: unexpected argv: $*" >&2; exit 2; }
cfg=$(cat)
val() { printf '%s\n' "$cfg" | sed -n "s/^$1 = \"\\(.*\\)\"\$/\\1/p" | head -n 1; }
url=$(val url) method=$(val request) out=$(val output) data=$(val data-binary)
auth=$(printf '%s\n' "$cfg" | sed -n 's/^header = "Authorization: \(.*\)"$/\1/p')
path=${url#https://api.github.com}
body=null; [ -z "$data" ] || body=$(jq -Rs . < "${data#@}")
jq -n -c --arg m "$method" --arg p "$path" --arg a "$auth" --argjson b "$body" --arg c "$cfg" '{method: $m, path: $p, auth: $a, body: $b, config: $c}' >> "$MOCK_GH/log"
r=$(awk -v m="$method" -v p="$path" '$1 == m && $2 == p { print $3 " " $4; exit }' "$MOCK_GH/routes")
if [ -z "$r" ]; then printf '{"message":"Not Found"}' > "$out"; printf 404; exit 0; fi
cp "$MOCK_GH/${r#* }" "$out"; printf '%s' "${r%% *}"
"#;

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Test-side git, with keel on PATH for the hooks the scratch repository runs.
fn git(d: &Path, a: &[&str]) -> String {
    let o = common::as_tester(common::cmd("git").args(a).current_dir(d).env("PATH", common::path_with(Path::new(KEEL).parent().unwrap()))).output().unwrap();
    assert!(o.status.success(), "git {a:?}: {}", text(&o));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn write(d: &Path, rel: &str, body: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, body).unwrap();
}

fn exe(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(mode)).unwrap();
}

fn commit(d: &Path, msg: &str) {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "--no-verify", "-m", msg]);
}

const PLAN_TITLE: &str = "Keel — E.W1 plan: `tick` @owner [x](y) | cell *b* #h";

/// main (config, the script) → epic/E (a README change) → wu/E/x (a plan, a result, code; cr and
/// test notes), main and epic/E on origin, Keel's hooks installed, the checkout on wu/E/x.
struct Gh {
    root: TempDir,
    repo: PathBuf,
    origin: PathBuf,
    home: PathBuf,
    mock: PathBuf,
    bin: PathBuf,
    marks: PathBuf,
    tmp: PathBuf,
}

impl Gh {
    fn new() -> Gh {
        let root = TempDir::new().unwrap();
        let base = root.path().canonicalize().unwrap();
        let [repo, home, mock, bin, marks, tmp] = ["repo", "home", "mock", "bin", "marks", "tmp"].map(|n| base.join(n));
        for d in [&repo, &mock, &bin, &marks, &tmp] {
            std::fs::create_dir_all(d).unwrap();
        }
        let origin = base.join("gh/own/Keel.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        let mark = |n: &str| toml::Value::String(format!("touch {}", common::sh_quote(&marks.join(n).to_string_lossy())));
        write(
            &repo,
            ".keel/config.toml",
            &format!(
                "code_paths = [\"src/\"]\n[check]\nlint = {}\ntest = {}\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n\n[github]   # the router bot's repository\nrepo = \"own/Keel\"\n",
                mark("lint-ran"),
                mark("test-ran")
            ),
        );
        let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/router-gh.sh")).unwrap();
        exe(&repo.join("scripts/router-gh.sh").tap_dir(), &script);
        write(&repo, "README", "x");
        commit(&repo, "base");
        git(&repo, &["switch", "-q", "-c", "epic/E"]);
        write(&repo, "README", "epic");
        commit(&repo, "epic work");
        git(&repo, &["switch", "-q", "-c", "wu/E/x"]);
        write(&repo, "docs/work/x-plan.html", &format!("<!DOCTYPE html>\n<html lang=\"en\" data-artifact=\"wu-plan\" data-status=\"ready\">\n<head>\n  <title>{PLAN_TITLE}</title>\n</head><body></body></html>\n"));
        write(
            &repo,
            "docs/work/x-result.html",
            "<!DOCTYPE html>\n<html lang=\"en\" data-artifact=\"wu-result\" data-status=\"draft\">\n<head><title>Keel — E.W1 result</title></head></html>\n",
        );
        write(&repo, "src/a.txt", "a");
        commit(&repo, "wu x");
        let tip = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["notes", "--ref=cr", "add", "-m", r#"{"verdict":"PASS","round":2}"#, &tip]);
        git(&repo, &["notes", "--ref=test", "add", "-m", "{}", &tip]);
        git(&repo, &["push", "-q", "--no-verify", origin.to_str().unwrap(), "main", "epic/E"]);
        let o = common::cmd(KEEL).args(["hooks", "install"]).current_dir(&repo).output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        // the App: a throwaway key and its ids
        let kdir = home.join(".config/keel/router-gh");
        std::fs::create_dir_all(&kdir).unwrap();
        let o = common::cmd("openssl")
            .args(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out"])
            .arg(kdir.join("app.pem"))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", text(&o));
        chmod(&kdir.join("app.pem"), 0o600);
        write(&kdir, "app.conf", "app_id=123\ninstallation_id=456\napp_slug=keel-router\n");
        common::git_shim(&bin);
        exe(&bin.join("fake-curl"), FAKE_CURL);
        let gh = Gh { root, repo, origin, home, mock, bin, marks, tmp };
        gh.routes(&[]);
        gh
    }

    fn sha(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn origin_sha(&self, rev: &str) -> Option<String> {
        let o = common::cmd("git").args(["rev-parse", "--verify", "-q", rev]).current_dir(&self.origin).output().unwrap();
        o.status.success().then(|| String::from_utf8(o.stdout).unwrap().trim().to_string())
    }

    /// The route table: the token exchange plus `extra` (method, path, status, JSON body).
    fn routes(&self, extra: &[(&str, &str, u16, serde_json::Value)]) {
        let mut all = vec![("POST", "/app/installations/456/access_tokens", 201, serde_json::json!({"token": TOKEN, "expires_at": "2026-11-15T12:00:00Z"}))];
        all.extend(extra.iter().map(|(m, p, s, b)| (*m, *p, *s, b.clone())));
        let mut table = String::new();
        for (i, (m, p, s, b)) in all.iter().enumerate() {
            std::fs::write(self.mock.join(format!("r{i}.json")), b.to_string()).unwrap();
            table.push_str(&format!("{m} {p} {s} r{i}.json\n"));
        }
        std::fs::write(self.mock.join("routes"), table).unwrap();
        std::fs::write(self.mock.join("log"), "").unwrap();
    }

    /// The requests the script sent since the last `routes`.
    fn log(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.mock.join("log")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    fn calls(&self) -> Vec<String> {
        self.log().iter().map(|r| format!("{} {}", r["method"].as_str().unwrap(), r["path"].as_str().unwrap())).collect()
    }

    fn path(&self) -> String {
        common::test_path(&common::path_with(Path::new(KEEL).parent().unwrap()), Some(&self.bin))
    }

    /// The script in the scratch repository, as the router runs it; every run is checked for secrets.
    fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut c = common::cmd("sh");
        c.arg(self.repo.join("scripts/router-gh.sh")).args(args).current_dir(&self.repo);
        c.env("PATH", self.path()).env("HOME", &self.home).env("TMPDIR", &self.tmp).env("MOCK_GH", &self.mock);
        c.env("ROUTER_GH_CURL", self.bin.join("fake-curl"))
            .env("ROUTER_GH_GIT", format!("file://{}", self.root.path().canonicalize().unwrap().join("gh").display()));
        c.envs(env.iter().copied());
        let o = c.output().unwrap();
        self.assert_no_secret(&o);
        let left: Vec<_> = std::fs::read_dir(&self.tmp).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(left.is_empty(), "the script left {left:?} in its temp dir");
        o
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_env(args, &[])
    }

    /// No output carries the key, a JWT the script sent, or the token.
    fn assert_no_secret(&self, o: &Output) {
        let out = text(o);
        assert!(!out.contains(TOKEN) && !out.contains("ghs_"), "the token leaked: {out}");
        assert!(!out.contains("PRIVATE KEY") && !out.contains("eyJ"), "key or JWT material leaked: {out}");
        let pem = std::fs::read_to_string(self.home.join(".config/keel/router-gh/app.pem")).unwrap_or_default();
        for line in pem.lines().filter(|l| l.len() > 40) {
            assert!(!out.contains(line), "the key leaked: {out}");
        }
        for r in self.log() {
            let auth = r["auth"].as_str().unwrap_or("");
            if let Some(jwt) = auth.strip_prefix("Bearer ").filter(|t| t.contains('.')) {
                assert!(!out.contains(jwt.split('.').nth(2).unwrap()), "the JWT leaked: {out}");
            }
        }
    }

    fn pr(&self, number: u64, login: &str, head_repo: &str, head_sha: &str, draft: bool) -> serde_json::Value {
        serde_json::json!({
            "number": number, "state": "open", "draft": draft, "node_id": "PR_kwDOtest42",
            "html_url": format!("https://github.com/own/Keel/pull/{number}"),
            "user": {"login": login, "type": if login.ends_with("[bot]") { "Bot" } else { "User" }},
            "head": {"ref": "wu/E/x", "sha": head_sha, "repo": {"full_name": head_repo}},
            "base": {"ref": "epic/E", "repo": {"full_name": "own/Keel"}}
        })
    }
}

trait TapDir {
    fn tap_dir(self) -> Self;
}
impl TapDir for PathBuf {
    /// The path, its parent directory made.
    fn tap_dir(self) -> Self {
        std::fs::create_dir_all(self.parent().unwrap()).unwrap();
        self
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn b64url(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' => 62,
        b'_' => 63,
        _ => panic!("not base64url: {s}"),
    };
    let bits: Vec<u8> = s.bytes().map(val).collect();
    let mut out = Vec::new();
    for ch in bits.chunks(4) {
        let n = ch.iter().enumerate().fold(0u32, |a, (i, v)| a | (u32::from(*v) << (18 - 6 * i)));
        out.extend(n.to_be_bytes()[1..ch.len()].iter());
    }
    out
}

/// The JWT that bought the token: RS256, issued by the App id, at most 10 minutes, signed by the key.
fn assert_jwt(gh: &Gh, auth: &str) {
    let jwt = auth.strip_prefix("Bearer ").unwrap();
    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3, "{jwt}");
    let header: serde_json::Value = serde_json::from_slice(&b64url(parts[0])).unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&b64url(parts[1])).unwrap();
    assert_eq!(header, serde_json::json!({"alg": "RS256", "typ": "JWT"}));
    assert_eq!(claims["iss"], "123");
    let (iat, exp) = (claims["iat"].as_i64().unwrap(), claims["exp"].as_i64().unwrap());
    assert!(exp > iat && exp - iat <= 600, "{claims}");
    let d = TempDir::new().unwrap();
    std::fs::write(d.path().join("data"), format!("{}.{}", parts[0], parts[1])).unwrap();
    std::fs::write(d.path().join("sig"), b64url(parts[2])).unwrap();
    let key = gh.home.join(".config/keel/router-gh/app.pem");
    let o = common::cmd("openssl").args(["pkey", "-pubout", "-in"]).arg(&key).arg("-out").arg(d.path().join("pub.pem")).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let o = common::cmd("openssl").args(["dgst", "-sha256", "-verify", "pub.pem", "-signature", "sig", "data"]).current_dir(d.path()).output().unwrap();
    assert!(o.status.success() && text(&o).contains("Verified OK"), "{}", text(&o));
}

/// Every API call but the token exchange carries the token, the exchange the JWT; the token is
/// requested for this repository only.
fn assert_auth(gh: &Gh) {
    let log = gh.log();
    assert_eq!(log[0]["path"], "/app/installations/456/access_tokens");
    assert_eq!(serde_json::from_str::<serde_json::Value>(log[0]["body"].as_str().unwrap()).unwrap(), serde_json::json!({"repositories": ["Keel"]}));
    assert_jwt(gh, log[0]["auth"].as_str().unwrap());
    for r in &log[1..] {
        assert_eq!(r["auth"], format!("Bearer {TOKEN}"), "{r}");
    }
    for r in &log {
        let c = r["config"].as_str().unwrap();
        assert!(c.contains("proto = \"=https\"") && c.contains("max-time = 60"), "{c}");
    }
}

fn body_of(r: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(r["body"].as_str().unwrap()).unwrap()
}

/// push runs the pre-push hook's `[check]` and lands the branch and every notes ref on origin, as
/// the App (a token was minted), never forced; a second push changes nothing.
#[test]
fn push_lands_the_branch_and_its_notes_after_the_pre_push_hook() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("PUSHED branch=wu/E/x sha={tip} state=new"));
    assert!(gh.marks.join("test-ran").exists(), "the pre-push hook's keel check did not run");
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x").as_deref(), Some(tip.as_str()));
    for n in ["cr", "test"] {
        assert_eq!(gh.origin_sha(&format!("refs/notes/{n}")), Some(gh.sha(&format!("refs/notes/{n}"))), "notes {n}");
    }
    assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens"]);
    assert_auth(&gh);
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(stdout(&o), format!("PUSHED branch=wu/E/x sha={tip} state=unchanged"), "{}", text(&o));
}

/// A red pre-push hook stops the push: nothing reaches origin.
#[test]
fn a_red_pre_push_hook_stops_the_push() {
    let gh = Gh::new();
    let cfg = std::fs::read_to_string(gh.repo.join(".keel/config.toml")).unwrap();
    assert!(cfg.contains("test = \"touch "), "{cfg}");
    write(&gh.repo, ".keel/config.toml", &cfg.replace("test = \"touch ", "test = \"false && touch "));
    commit(&gh.repo, "red check");
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("push of wu/E/x refused"), "{}", text(&o));
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), None);
}

/// A branch GitHub holds on another line is rejected (exit 1) and origin keeps its commit.
#[test]
fn push_never_forces_a_diverged_branch() {
    let gh = Gh::new();
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
    let old = gh.sha("wu/E/x");
    git(&gh.repo, &["reset", "-q", "--hard", "epic/E"]);
    write(&gh.repo, "src/b.txt", "b");
    commit(&gh.repo, "another line");
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("push of wu/E/x rejected") && text(&o).contains("never forced"), "{}", text(&o));
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), Some(old));
}

/// A notes ref GitHub holds on another line is a WARN line; the branch still lands and the exit is 0.
#[test]
fn a_rejected_notes_ref_warns_and_the_branch_still_lands() {
    let gh = Gh::new();
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
    let epic = gh.sha("epic/E");
    git(&gh.repo, &["notes", "--ref=alt", "add", "-m", "{}", &epic]);
    git(&gh.repo, &["push", "-q", "--no-verify", "-f", gh.origin.to_str().unwrap(), "refs/notes/alt:refs/notes/cr"]);
    write(&gh.repo, "src/c.txt", "c");
    commit(&gh.repo, "more");
    let tip = gh.sha("wu/E/x");
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let out = stdout(&o);
    assert!(out.lines().any(|l| l.starts_with("WARN notes ref refs/notes/cr not pushed: ")), "{out}");
    assert!(out.ends_with(&format!("PUSHED branch=wu/E/x sha={tip} state=updated")), "{out}");
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), Some(tip));
    assert_eq!(gh.origin_sha("refs/notes/alt"), Some(gh.sha("refs/notes/alt")), "the other notes refs still went");
}

/// main, a tag, an unknown branch, a flag or an extra word: exit 2 before any request, origin as it was.
#[test]
fn push_refuses_main_a_tag_an_unknown_branch_and_flags_before_any_request() {
    let gh = Gh::new();
    git(&gh.repo, &["tag", "v1.0"]);
    let before = git(&gh.origin, &["for-each-ref"]);
    for (args, why) in [
        (vec!["push", "main"], "branch must be"),
        (vec!["push", "v1.0"], "branch must be"),
        (vec!["push", "refs/tags/v1.0"], "branch must be"),
        (vec!["push", "wu/E/nope"], "no local branch wu/E/nope"),
        (vec!["push", "-f"], "may not start with -"),
        (vec!["push", "--force", "wu/E/x"], "usage: push"),
        (vec!["push", "wu/E/x", "--force"], "usage: push"),
        (vec!["push", "+wu/E/x"], "branch must be"),
        (vec!["push", "wu/E/x:main"], "branch must be"),
    ] {
        let o = gh.run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", text(&o));
        assert!(text(&o).contains(why), "{args:?}: {}", text(&o));
        assert!(gh.log().is_empty(), "{args:?} sent a request");
    }
    assert_eq!(git(&gh.origin, &["for-each-ref"]), before);
    assert!(!gh.marks.join("test-ran").exists(), "a refused push ran the hook");
}

/// pr-open: one draft request, head and base from the name, title from the plan made plain, a body
/// with the tip, the changed docs linked at the tip and the notes; GitHub's branch must be the tip.
#[test]
fn pr_open_sends_one_draft_request_derived_from_the_branch_and_its_docs() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    git(&gh.repo, &["push", "-q", "--no-verify", gh.origin.to_str().unwrap(), "wu/E/x"]);
    gh.routes(&[
        ("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": tip}})),
        ("GET", "/repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100", 200, serde_json::json!([])),
        ("POST", "/repos/own/Keel/pulls", 201, serde_json::json!({"number": 7, "html_url": "https://github.com/own/Keel/pull/7"})),
    ]);
    let o = gh.run(&["pr-open", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("PR number=7 url=https://github.com/own/Keel/pull/7 state=opened head=wu/E/x base=epic/E sha={tip}"));
    assert_eq!(
        gh.calls(),
        [
            "POST /app/installations/456/access_tokens",
            "GET /repos/own/Keel/git/ref/heads/wu/E/x",
            "GET /repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100",
            "POST /repos/own/Keel/pulls"
        ]
    );
    assert_auth(&gh);
    let req = body_of(&gh.log()[3]);
    assert_eq!(req["head"], "wu/E/x");
    assert_eq!(req["base"], "epic/E");
    assert_eq!(req["draft"], true);
    let title = req["title"].as_str().unwrap();
    assert_eq!(title, "Keel — E.W1 plan: tick owner x(y)  cell b h", "the plan's title, made plain");
    let body = req["body"].as_str().unwrap();
    assert!(body.contains(&format!("**Tip:** `{tip}`")), "{body}");
    assert!(
        body.contains(&format!(
            "| [Keel — E.W1 plan: tick owner x(y)  cell b h](https://github.com/own/Keel/blob/{tip}/docs/work/x-plan.html) | wu-plan | ready |"
        )),
        "{body}"
    );
    assert!(body.contains(&format!("| [Keel — E.W1 result](https://github.com/own/Keel/blob/{tip}/docs/work/x-result.html) | wu-result | draft |")), "{body}");
    assert!(body.contains("| yes | — | PASS, round 2 | — | — |"), "{body}");
    for active in ["@owner", "`tick`", "[x]", "*b*", "#h", "| cell"] {
        assert!(!body.contains(active), "{active} survived: {body}");
    }
}

/// The base comes from the name (an epic into main), or from --base main|epic/E.
#[test]
fn pr_open_derives_main_for_an_epic_and_takes_an_explicit_base() {
    let gh = Gh::new();
    let (epic, tip) = (gh.sha("epic/E"), gh.sha("wu/E/x"));
    git(&gh.repo, &["push", "-q", "--no-verify", gh.origin.to_str().unwrap(), "wu/E/x"]);
    gh.routes(&[
        ("GET", "/repos/own/Keel/git/ref/heads/epic/E", 200, serde_json::json!({"object": {"sha": epic}})),
        ("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": tip}})),
        ("GET", "/repos/own/Keel/pulls?state=open&head=own:epic/E&per_page=100", 200, serde_json::json!([])),
        ("GET", "/repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100", 200, serde_json::json!([])),
        ("POST", "/repos/own/Keel/pulls", 201, serde_json::json!({"number": 8, "html_url": "https://github.com/own/Keel/pull/8"})),
    ]);
    let o = gh.run(&["pr-open", "epic/E"]);
    assert!(stdout(&o).ends_with(&format!("state=opened head=epic/E base=main sha={epic}")), "{}", text(&o));
    let req = body_of(&gh.log()[3]);
    assert_eq!((req["base"].as_str(), req["title"].as_str()), (Some("main"), Some("epic/E")), "no epic plan changed: the branch is the title");
    assert!(req["body"].as_str().unwrap().contains("No doc changed in this range."), "{req}");
    gh.routes(&[
        ("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": tip}})),
        ("GET", "/repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100", 200, serde_json::json!([])),
        ("POST", "/repos/own/Keel/pulls", 201, serde_json::json!({"number": 9, "html_url": "https://github.com/own/Keel/pull/9"})),
    ]);
    let o = gh.run(&["pr-open", "wu/E/x", "--base", "main"]);
    assert!(stdout(&o).ends_with(&format!("state=opened head=wu/E/x base=main sha={tip}")), "{}", text(&o));
    assert_eq!(body_of(&gh.log()[3])["base"], "main");
}

/// The bot's open pull request for the head is refreshed (PATCH), never doubled; someone else's, or
/// one into another base, is refused with no write.
#[test]
fn pr_open_refreshes_the_bots_open_pull_request_and_refuses_anyone_elses() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    let refresh = |login: &str, base: &str| {
        let mut pr = gh.pr(7, login, "own/Keel", &tip, true);
        pr["base"]["ref"] = base.into();
        gh.routes(&[
            ("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": tip}})),
            ("GET", "/repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100", 200, serde_json::json!([pr])),
            ("PATCH", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true)),
        ]);
        gh.run(&["pr-open", "wu/E/x"])
    };
    let o = refresh(BOT, "epic/E");
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("PR number=7 url=https://github.com/own/Keel/pull/7 state=refreshed head=wu/E/x base=epic/E sha={tip}"));
    let req = body_of(&gh.log()[3]);
    assert_eq!(gh.calls()[3], "PATCH /repos/own/Keel/pulls/7");
    assert!(req.get("draft").is_none() && req.get("base").is_none() && req["body"].as_str().unwrap().contains(&tip), "{req}");
    for (login, base, why) in [("mallory", "epic/E", "the bot did not open it"), (BOT, "main", "merges into main, not epic/E")] {
        let o = refresh(login, base);
        assert_eq!(o.status.code(), Some(2), "{}", text(&o));
        assert!(text(&o).contains(why), "{}", text(&o));
        assert!(!gh.calls().iter().any(|c| c.starts_with("PATCH") || c.starts_with("POST /repos")), "{:?}", gh.calls());
    }
}

/// GitHub must hold the local tip: a missing or older branch is "push first", with no pull request sent.
#[test]
fn pr_open_refuses_until_github_has_the_local_tip() {
    let gh = Gh::new();
    for (status, sha, why) in [(404, "0".repeat(40), "push first: wu/E/x is not on GitHub"), (200, gh.sha("epic/E"), "push first: GitHub has wu/E/x at")] {
        gh.routes(&[("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", status, serde_json::json!({"object": {"sha": sha}}))]);
        let o = gh.run(&["pr-open", "wu/E/x"]);
        assert_eq!(o.status.code(), Some(2), "{}", text(&o));
        assert!(text(&o).contains(why), "{}", text(&o));
        assert_eq!(gh.calls().len(), 2, "{:?}", gh.calls());
    }
}

/// pr-ready marks the bot's draft ready by its node id, only when its head is the local tip; anyone
/// else's pull request, a fork's head or an older head sends no mutation; a ready one is left alone.
#[test]
fn pr_ready_marks_only_the_bots_draft_at_the_local_tip() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    let ready = serde_json::json!({"data": {"markPullRequestReadyForReview": {"pullRequest": {"isDraft": false}}}});
    let with = |pr: serde_json::Value| {
        gh.routes(&[("GET", "/repos/own/Keel/pulls/7", 200, pr), ("POST", "/graphql", 200, ready.clone())]);
        gh.run(&["pr-ready", "7"])
    };
    let o = with(gh.pr(7, BOT, "own/Keel", &tip, true));
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("READY pr=7 head=wu/E/x sha={tip} state=ready"));
    assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens", "GET /repos/own/Keel/pulls/7", "POST /graphql"]);
    assert_auth(&gh);
    let q = body_of(&gh.log()[2]);
    assert_eq!(q["variables"], serde_json::json!({"id": "PR_kwDOtest42"}));
    assert!(q["query"].as_str().unwrap().contains("markPullRequestReadyForReview(input: {pullRequestId: $id})"), "{q}");

    let o = with(gh.pr(7, BOT, "own/Keel", &tip, false));
    assert_eq!(stdout(&o), format!("READY pr=7 head=wu/E/x sha={tip} state=unchanged"), "{}", text(&o));
    assert_eq!(gh.calls().len(), 2);

    let older = gh.sha("epic/E");
    for (pr, why) in [
        (gh.pr(7, "mallory", "own/Keel", &tip, true), "not one the bot opened"),
        (gh.pr(7, "keel-router", "own/Keel", &tip, true), "not one the bot opened"),
        (gh.pr(7, BOT, "mallory/Keel", &tip, true), "not one the bot opened"),
        (gh.pr(7, BOT, "own/Keel", &older, true), "not the local tip: push first"),
        (
            {
                let mut p = gh.pr(7, BOT, "own/Keel", &tip, true);
                p["state"] = "closed".into();
                p
            },
            "is closed, not open",
        ),
        (
            {
                let mut p = gh.pr(7, BOT, "own/Keel", &tip, true);
                p["head"]["ref"] = "main".into();
                p
            },
            "branch must be",
        ),
    ] {
        let o = with(pr);
        assert_eq!(o.status.code(), Some(2), "{}", text(&o));
        assert!(text(&o).contains(why), "{why}: {}", text(&o));
        assert!(!gh.calls().contains(&"POST /graphql".to_string()), "{:?}", gh.calls());
    }
    // a GraphQL error is exit 1 with its message
    gh.routes(&[
        ("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true)),
        ("POST", "/graphql", 200, serde_json::json!({"errors": [{"message": "Resource not accessible by integration"}]})),
    ]);
    let o = gh.run(&["pr-ready", "7"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("did not mark pull request 7 ready: Resource not accessible by integration"), "{}", text(&o));
}

/// post sends the head's worker's file under a fixed header, mentions inert, then deletes it.
#[test]
fn post_sends_the_workers_file_once_with_mentions_inert() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    write(&gh.repo, ".router-gh/status.md", "Round 2 PASS. @owner please look.\n\tdone\n");
    gh.routes(&[
        ("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true)),
        ("POST", "/repos/own/Keel/issues/7/comments", 201, serde_json::json!({"id": 991})),
    ]);
    let o = gh.run(&["post", "7", "status"]);
    assert!(o.status.success(), "{}", text(&o));
    let file = gh.repo.join(".router-gh/status.md");
    assert_eq!(stdout(&o), format!("POSTED pr=7 comment=991 file={}", file.display()));
    assert!(!file.exists(), "a posted file is removed, so it is never resent");
    assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens", "GET /repos/own/Keel/pulls/7", "POST /repos/own/Keel/issues/7/comments"]);
    assert_auth(&gh);
    let body = body_of(&gh.log()[2])["body"].as_str().unwrap().to_string();
    assert_eq!(body, format!("**From the worker on `wu/E/x`** (`status.md`, tip `{}`):\n\nRound 2 PASS. @\u{2060}owner please look.\n\tdone\n", &tip[..12]));
}

/// A link, a linked directory, an oversized, empty, non-UTF-8 or control-character file, or none at
/// all: exit 2, no comment sent, the file kept.
#[test]
fn post_refuses_unsafe_files_and_keeps_them() {
    let gh = Gh::new();
    let tip = gh.sha("wu/E/x");
    let dir = gh.repo.join(".router-gh");
    std::fs::create_dir_all(&dir).unwrap();
    write(&gh.root.path().canonicalize().unwrap(), "elsewhere.md", "outside");
    std::os::unix::fs::symlink(gh.root.path().join("elsewhere.md"), dir.join("link.md")).unwrap();
    std::fs::write(dir.join("big.md"), "a".repeat(60_001)).unwrap();
    std::fs::write(dir.join("max.md"), "a".repeat(60_000)).unwrap();
    std::fs::write(dir.join("empty.md"), "").unwrap();
    std::fs::write(dir.join("bell.md"), "ding \x07\n").unwrap();
    std::fs::write(dir.join("cr.md"), "line\r\n").unwrap();
    std::fs::write(dir.join("nul.md"), b"a\0b\n").unwrap();
    std::fs::write(dir.join("latin1.md"), b"caf\xe9\n").unwrap();
    for (name, why) in [
        ("link", "must be a regular file, not a link"),
        ("big", "must hold 1 to 60000 bytes"),
        ("empty", "must hold 1 to 60000 bytes"),
        ("bell", "control character"),
        ("cr", "control character"),
        ("nul", "control character"),
        ("latin1", "is not UTF-8"),
        ("missing", "no comment file"),
    ] {
        gh.routes(&[
            ("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true)),
            ("POST", "/repos/own/Keel/issues/7/comments", 201, serde_json::json!({"id": 1})),
        ]);
        let o = gh.run(&["post", "7", name]);
        assert_eq!(o.status.code(), Some(2), "{name}: {}", text(&o));
        assert!(text(&o).contains(why), "{name}: {}", text(&o));
        assert_eq!(gh.calls().len(), 2, "{name}: {:?}", gh.calls());
        assert!(name == "missing" || std::fs::symlink_metadata(dir.join(format!("{name}.md"))).is_ok(), "{name} was removed");
    }
    // the limit itself is fine; a linked .router-gh directory is not
    gh.routes(&[
        ("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true)),
        ("POST", "/repos/own/Keel/issues/7/comments", 201, serde_json::json!({"id": 2})),
    ]);
    assert!(gh.run(&["post", "7", "max"]).status.success());
    std::fs::rename(&dir, gh.root.path().join("moved")).unwrap();
    std::os::unix::fs::symlink(gh.root.path().join("moved"), &dir).unwrap();
    gh.routes(&[("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true))]);
    let o = gh.run(&["post", "7", "big"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("is a link: refused"), "{}", text(&o));
    // someone else's pull request: refused before any file is read
    gh.routes(&[("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, "mallory", "own/Keel", &tip, true))]);
    let o = gh.run(&["post", "7", "big"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("not one the bot opened"), "{}", text(&o));
}

/// sync fast-forwards the local epic to a merge made on GitHub, in its worktree when it is checked
/// out; a dirty worktree or a diverged local branch moves nothing.
#[test]
fn sync_fast_forwards_the_local_epic_to_githubs_merge() {
    let gh = Gh::new();
    // GitHub merges wu/E/x into epic/E
    let merger = gh.root.path().join("merger");
    git(gh.root.path(), &["clone", "-q", gh.origin.to_str().unwrap(), merger.to_str().unwrap()]);
    git(&gh.repo, &["push", "-q", "--no-verify", gh.origin.to_str().unwrap(), "wu/E/x"]);
    let github_merges = |what: &str| {
        git(&merger, &["fetch", "-q", "origin"]);
        git(&merger, &["checkout", "-q", "-B", "epic/E", "origin/epic/E"]);
        git(&merger, &["merge", "-q", "--no-ff", "--no-verify", "-m", what, &format!("origin/{what}")]);
        git(&merger, &["push", "-q", "--no-verify", "origin", "epic/E"]);
        git(&merger, &["rev-parse", "HEAD"])
    };
    let old = gh.sha("epic/E");
    let merged = github_merges("wu/E/x");
    let o = gh.run(&["sync", "epic/E"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("SYNCED branch=epic/E from={old} sha={merged} state=fast-forward"));
    assert_eq!(gh.sha("epic/E"), merged);
    assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens"]);
    let o = gh.run(&["sync", "epic/E"]);
    assert_eq!(stdout(&o), format!("SYNCED branch=epic/E sha={merged} state=unchanged"));

    // checked out in a worktree: the worktree moves with it
    let wt = gh.root.path().canonicalize().unwrap().join("epic-wt");
    git(&gh.repo, &["worktree", "add", "-q", wt.to_str().unwrap(), "epic/E"]);
    git(&gh.repo, &["switch", "-q", "-c", "wu/E/y", &merged]);
    write(&gh.repo, "src/y.txt", "y");
    commit(&gh.repo, "wu y");
    git(&gh.repo, &["push", "-q", "--no-verify", gh.origin.to_str().unwrap(), "wu/E/y"]);
    let merged2 = github_merges("wu/E/y");
    write(&wt, "dirt.txt", "d");
    let o = gh.run(&["sync", "epic/E"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("has uncommitted changes: nothing moved"), "{}", text(&o));
    assert_eq!(gh.sha("epic/E"), merged);
    std::fs::remove_file(wt.join("dirt.txt")).unwrap();
    let o = gh.run(&["sync", "epic/E"]);
    assert_eq!(stdout(&o), format!("SYNCED branch=epic/E from={merged} sha={merged2} state=fast-forward worktree={}", wt.display()), "{}", text(&o));
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), merged2);
    assert!(wt.join("src/y.txt").exists(), "the worktree's files follow the branch");

    // a local commit GitHub lacks: not a fast-forward
    write(&wt, "local.txt", "l");
    commit(&wt, "local only");
    let local = git(&wt, &["rev-parse", "HEAD"]);
    let o = gh.run(&["sync", "epic/E"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("not a fast-forward"), "{}", text(&o));
    assert_eq!(gh.sha("epic/E"), local);
}

/// The key must be there, a regular file, the user's, 0600 or 0400, and outside every worktree;
/// app.conf must hold exactly the three ids. Each problem is one line naming the file; no request.
#[test]
fn key_and_app_settings_problems_are_refused_before_any_request() {
    let gh = Gh::new();
    let kdir = gh.home.join(".config/keel/router-gh");
    let key = kdir.join("app.pem");
    let check = |why: &str| {
        let o = gh.run(&["push", "wu/E/x"]);
        assert_eq!(o.status.code(), Some(2), "{why}: {}", text(&o));
        assert!(text(&o).contains(why), "{why}: {}", text(&o));
        assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1, "{}", text(&o));
        assert!(gh.log().is_empty() && gh.origin_sha("refs/heads/wu/E/x").is_none(), "{why}");
    };
    chmod(&key, 0o644);
    check("must have mode 0600 or 0400");
    chmod(&key, 0o640);
    check("must have mode 0600 or 0400");
    chmod(&key, 0o400);
    assert!(gh.run(&["push", "wu/E/x"]).status.success(), "0400 is fine");
    git(&gh.origin, &["update-ref", "-d", "refs/heads/wu/E/x"]);
    gh.routes(&[]);
    std::fs::rename(&key, kdir.join("real.pem")).unwrap();
    check(&format!("no App key at {}", key.display()));
    std::os::unix::fs::symlink(kdir.join("real.pem"), &key).unwrap();
    check("must be a regular file, not a link");
    std::fs::remove_file(&key).unwrap();
    std::fs::rename(kdir.join("real.pem"), &key).unwrap();
    for (conf, why) in [
        ("app_id=123\ninstallation_id=456\n", "must set app_id, installation_id and app_slug"),
        ("app_id=123\ninstallation_id=456\napp_slug=keel-router\ntoken=x\n", "unknown line"),
        ("app_id=12a\ninstallation_id=456\napp_slug=keel-router\n", "bad app_id line"),
        ("app_id=1\napp_id=2\ninstallation_id=456\napp_slug=keel-router\n", "bad app_id line"),
        ("app_id=123\ninstallation_id=456\napp_slug=Keel Router\n", "bad app_slug line"),
    ] {
        write(&kdir, "app.conf", conf);
        check(why);
    }
    std::fs::remove_file(kdir.join("app.conf")).unwrap();
    check("no App settings at");
    write(&kdir, "app.conf", "app_id=123\ninstallation_id=456\napp_slug=keel-router\n");
    chmod(&key, 0o600);
    write(&kdir, "app.pem", "not a key\n");
    check("cannot sign with the App key");
    // a key directory inside a worktree of the repository
    let inside = gh.repo.join("home");
    std::fs::create_dir_all(inside.join(".config/keel")).unwrap();
    std::fs::rename(&kdir, inside.join(".config/keel/router-gh")).unwrap();
    let o = gh.run_env(&["push", "wu/E/x"], &[("HOME", inside.to_str().unwrap())]);
    assert!(o.status.code() == Some(2) && text(&o).contains("which workers can reach: move it out"), "{}", text(&o));
    assert!(gh.log().is_empty());
}

/// An API error is exit 1 with its status and GitHub's message, one line, never the request.
#[test]
fn an_http_error_prints_its_status_and_githubs_message_only() {
    let gh = Gh::new();
    std::fs::write(gh.mock.join("routes"), "POST /app/installations/456/access_tokens 401 e.json\n").unwrap();
    std::fs::write(gh.mock.join("e.json"), r#"{"message":"A JSON web token could not be decoded\u0007","documentation_url":"https://docs.github.com"}"#)
        .unwrap();
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "router-gh: GitHub refused POST /app/installations/456/access_tokens: HTTP 401: A JSON web token could not be decoded\n"
    );
    assert!(o.stdout.is_empty());
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), None);
    // a token GitHub returns that is not one is refused, not used
    std::fs::write(gh.mock.join("routes"), "POST /app/installations/456/access_tokens 201 t.json\n").unwrap();
    std::fs::write(gh.mock.join("t.json"), r#"{"token":"ghs_x\"; rm -rf /"}"#).unwrap();
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.code() == Some(1) && text(&o).contains("no usable installation token"), "{}", text(&o));
}

/// git's credential helper: `get` prints the token once, then the file is gone and it prints
/// nothing; other ops print nothing; a file this script did not make is refused.
#[test]
fn the_credential_mode_gives_the_token_once() {
    let gh = Gh::new();
    let dir = gh.root.path().canonicalize().unwrap().join("router-gh.AbC123");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("token");
    let run = |f: &Path, op: &str| common::cmd("sh").arg(gh.repo.join("scripts/router-gh.sh")).arg("credential").arg(f).arg(op).output().unwrap();
    std::fs::write(&file, TOKEN).unwrap();
    let o = run(&file, "store");
    assert!(o.status.success() && o.stdout.is_empty() && file.exists());
    let o = run(&file, "get");
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout), format!("username=x-access-token\npassword={TOKEN}\n"));
    assert!(!file.exists() && std::fs::read_dir(&dir).unwrap().next().is_none(), "the token file outlived its one read");
    let o = run(&file, "get");
    assert!(o.status.success() && o.stdout.is_empty());
    std::fs::write(&file, "not a token; id").unwrap();
    assert!(run(&file, "get").stdout.is_empty(), "a malformed token is never printed");
    for f in [
        "/etc/passwd".into(),
        gh.root.path().join("token"),
        gh.root.path().join("router-gh.abc/token"),
        dir.join("../router-gh.AbC123/token"),
        dir.join("other"),
    ] {
        let o = run(&f, "get");
        assert!(o.status.code() == Some(2) && text(&o).contains("not a router-gh token file"), "{}: {}", f.display(), text(&o));
    }
}

/// The real curl reads the request config the script writes: an unreachable host fails as a network
/// error (curl exit 6, 7, 28, 35, 56…), never as a config or option error, and prints no secret.
#[test]
fn the_real_curl_reads_the_request_config() {
    let gh = Gh::new();
    let o = gh.run_env(&["push", "wu/E/x"], &[("ROUTER_GH_CURL", "curl"), ("ROUTER_GH_API", "https://127.0.0.1:9")]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    let code: i32 = err.split("(curl exit ").nth(1).and_then(|r| r.split(')').next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("{err}"));
    assert!(![1, 2, 3, 26, 27, 43, 48, 49].contains(&code), "curl refused the request config (exit {code}): {err}");
    assert!(err.contains("GitHub unreachable: POST /app/installations/456/access_tokens"), "{err}");
}

/// The comment files workers write never reach a commit in Keel's own repository.
#[test]
fn keels_gitignore_holds_the_comment_files() {
    let ignore = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l == ".router-gh/"), "{ignore}");
}
