//! E-GHFLOW.W2: scripts/router-gh.sh, the router bot's one door to GitHub, at its real boundary: a
//! scratch repository holding a copy of the script and a committed `[github] repo`, Keel's hooks
//! installed, a bare "origin" reached as GitHub's git URL would be (`ROUTER_GH_GIT=file://…`), and a
//! fake curl standing for GitHub's API (`ROUTER_GH_CURL`): it records each request the script sends
//! on its stdin and answers from a route table. Worker sandboxes refuse every socket, so the mock is
//! the client side, not a server. The App key is a throwaway RSA key the test makes in a temp home;
//! no real key or token is ever read.
//! Needs `sh`, `git`, `jq`, `openssl`, `iconv` and `perl` on PATH, as the script does, plus `curl`
//! and `python3` for one test each (the real curl; the git standing for GitHub's https transport).
compiled!();

use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

use crate::common;

const KEEL: &str = env!("CARGO_BIN_EXE_keel");
/// An installation token as GitHub mints it since 2026-04-27 (E-GHFLOW2.W1): stateless,
/// `ghs_<app id>_<JWT>`, about 520 characters of base64url parts (with `-` and `_`) and two dots.
pub(crate) const TOKEN: &str = concat!(
    "ghs_123_",
    "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9",
    ".",
    "eyJpc3MiOiIxMjMiLCJpbnN0YWxsYXRpb24iOjQ1Nn0-routerGh_test-eyJpc3MiOiIxMjMiLCJpbnN0YWxsYXRpb24iOjQ1Nn0-",
    "routerGh_test-eyJpc3MiOiIxMjMiLCJpbnN0YWxsYXRpb24iOjQ1Nn0-",
    ".",
    "c2lnbmF0dXJl-routerGh_Test-0123456789_abc-c2lnbmF0dXJl-routerGh_Test-0123456789_abc-c2lnbmF0dXJl-",
    "routerGh_Test-0123456789_abc-c2lnbmF0dXJl-routerGh_Test-0123456789_abc-c2lnbmF0dXJl-routerGh_Test-",
    "0123456789_abc-c2lnbmF0dXJl-routerGh_Test-0123456789_abc-c2lnbmF0dXJl-routerGh_Test-0123456789_abc-",
    "c2lnbmF0dXJl-rou"
);
/// The legacy installation token, `ghs_` and 36 characters, which GitHub minted before the change.
pub(crate) const LEGACY_TOKEN: &str = "ghs_routerGhTestToken0123456789abcdefXYZ";
pub(crate) const BOT: &str = "keel-router[bot]";
/// The commander the scratch config names (E-GHFLOW.W3): the owner's real id, so the fixture reads
/// as the committed config does.
pub(crate) const OWNER_ID: u64 = 5499546;

/// GitHub's API as the script reaches it: the whole request comes as a curl config on stdin.
/// Routes: `METHOD PATH STATUS FILE[,FILE…]` lines in $MOCK_GH/routes (first match; none is a 404).
/// A route's files are its answers in turn, the last repeated (E-GHFLOW.W3: a watch's polls).
pub(crate) const FAKE_CURL: &str = r#"#!/bin/sh
set -eu
[ $# -eq 3 ] && [ "$1" = -q ] && [ "$2" = --config ] && [ "$3" = - ] || { echo "fake curl: unexpected argv: $*" >&2; exit 2; }
cfg=$(cat)
val() { printf '%s\n' "$cfg" | sed -n "s/^$1 = \"\\(.*\\)\"\$/\\1/p" | head -n 1; }
url=$(val url) method=$(val request) out=$(val output) data=$(val data-binary)
auth=$(printf '%s\n' "$cfg" | sed -n 's/^header = "Authorization: \(.*\)"$/\1/p')
path=${url#https://api.github.com}
body=null; [ -z "$data" ] || body=$(jq -Rs . < "${data#@}")
jq -n -c --arg m "$method" --arg p "$path" --arg a "$auth" --argjson b "$body" --arg c "$cfg" '{method: $m, path: $p, auth: $a, body: $b, config: $c}' >> "$MOCK_GH/log"
r=$(awk -v m="$method" -v p="$path" '$1 == m && $2 == p { print NR " " $3 " " $4; exit }' "$MOCK_GH/routes")
hdr=$(val dump-header)
if [ -z "$r" ]; then printf '{"message":"Not Found"}' > "$out"; [ -z "$hdr" ] || printf 'HTTP/2 404\r\n\r\n' > "$hdr"; printf 404; exit 0; fi
set -- $r
# a route's answers are served in turn, one per call, the last repeated; status curlN is curl exit N
k=$(cat "$MOCK_GH/seq.$1" 2> /dev/null || echo 0); echo $((k + 1)) > "$MOCK_GH/seq.$1"
case "$2" in curl*) exit "${2#curl}";; esac
f=$(printf '%s\n' "$3" | tr , '\n' | sed -n "$((k + 1))p"); [ -n "$f" ] || f=${3##*,}
cp "$MOCK_GH/$f" "$out"
# the response's headers: <file>.hdr when the test wrote one (a Link header), else the status line
if [ -n "$hdr" ]; then if [ -f "$MOCK_GH/$f.hdr" ]; then cp "$MOCK_GH/$f.hdr" "$hdr"; else printf 'HTTP/2 %s\r\n\r\n' "$2" > "$hdr"; fi; fi
printf '%s' "$2"
"#;

pub(crate) fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Test-side git, with keel on PATH for the hooks the scratch repository runs.
pub(crate) fn git(d: &Path, a: &[&str]) -> String {
    let o = common::as_tester(common::cmd("git").args(a).current_dir(d).env("PATH", common::path_with(Path::new(KEEL).parent().unwrap()))).output().unwrap();
    assert!(o.status.success(), "git {a:?}: {}", text(&o));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

pub(crate) fn write(d: &Path, rel: &str, body: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, body).unwrap();
}

pub(crate) fn exe(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

pub(crate) fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(mode)).unwrap();
}

/// The test/v1 note the push checks for on a wu/ tip (schema and tip; the gate reads the rest).
pub(crate) fn test_note(d: &Path, tip: &str) {
    git(d, &["notes", "--ref=test", "add", "-f", "-m", &format!(r#"{{"schema":"test/v1","tip":"{tip}"}}"#), tip]);
}

pub(crate) fn commit(d: &Path, msg: &str) {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "--no-verify", "-m", msg]);
}

const PLAN_TITLE: &str = "Keel — E.W1 plan: `tick` @owner [x](y) | cell *b* #h";

/// A temp dir whose path the script accepts as a HOME (letters, digits and / . _ -): under TMPDIR
/// when its path is plain, else under /tmp, so a hand-set TMPDIR with another character fails no
/// test on HOME (backlog G5, W2).
pub(crate) fn plain_tempdir() -> TempDir {
    let plain = |p: &Path| p.to_str().is_some_and(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)));
    let tmp = std::env::temp_dir();
    if plain(&tmp.canonicalize().unwrap_or(tmp.clone())) {
        TempDir::new().unwrap()
    } else {
        TempDir::new_in("/tmp").unwrap()
    }
}

/// The App in `home`, as the owner's setup leaves it: a throwaway key and its ids in
/// `.config/keel/router-gh`, whatever the umask made of the directories. The key is made once per
/// run and copied (backlog G5, W2: a 2048-bit key per test cost about 0.25 s).
pub(crate) fn install_app(home: &Path) {
    let kdir = home.join(".config/keel/router-gh");
    std::fs::create_dir_all(&kdir).unwrap();
    // as the owner's setup does (`chmod 700`): never the umask's mode
    chmod(&kdir, 0o700);
    let made = common::template("router-gh app key", |t| {
        let o =
            common::cmd("openssl").args(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out"]).arg(t.join("app.pem")).output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
    });
    std::fs::copy(made.join("app.pem"), kdir.join("app.pem")).unwrap();
    chmod(&kdir.join("app.pem"), 0o600);
    write(&kdir, "app.conf", "app_id=123\ninstallation_id=456\napp_slug=keel-router\n");
}

/// main (config, the script) → epic/E (a README change) → wu/E/x (a plan, a result, code; cr and
/// test notes), main and epic/E on origin, Keel's hooks installed, the checkout on wu/E/x.
pub(crate) struct Gh {
    pub(crate) root: TempDir,
    pub(crate) repo: PathBuf,
    pub(crate) origin: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) mock: PathBuf,
    pub(crate) bin: PathBuf,
    pub(crate) marks: PathBuf,
    pub(crate) tmp: PathBuf,
}

impl Gh {
    pub(crate) fn new() -> Gh {
        let root = plain_tempdir();
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
                "code_paths = [\"src/\"]\n[check]\nlint = {}\ntest = {}\n[branches]\nmain = \"main\"\n[docs]\nroot = \"docs/\"\n\n[github]   # the router bot's repository\nrepo = \"own/Keel\"\ncommanders = [{OWNER_ID}]   # who the router hears\nfree_text = true\n",
                mark("lint-ran"),
                mark("test-ran")
            ),
        );
        for f in ["scripts/router-gh.sh", "scripts/router-gh-watch.sh", "scripts/router-gh-command.jq"] {
            let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(f)).unwrap();
            exe(&repo.join(f).tap_dir(), &script);
        }
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
        test_note(&repo, &tip);
        git(&repo, &["push", "-q", "--no-verify", origin.to_str().unwrap(), "main", "epic/E"]);
        let o = common::cmd(KEEL).args(["hooks", "install"]).current_dir(&repo).output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        install_app(&home);
        exe(&bin.join("fake-curl"), FAKE_CURL);
        let gh = Gh { root, repo, origin, home, mock, bin, marks, tmp };
        gh.routes(&[]);
        gh
    }

    pub(crate) fn sha(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    pub(crate) fn origin_sha(&self, rev: &str) -> Option<String> {
        let o = common::cmd("git").args(["rev-parse", "--verify", "-q", rev]).current_dir(&self.origin).output().unwrap();
        o.status.success().then(|| String::from_utf8(o.stdout).unwrap().trim().to_string())
    }

    /// The route table: the token exchange plus `extra` (method, path, status, JSON body).
    pub(crate) fn routes(&self, extra: &[(&str, &str, u16, serde_json::Value)]) {
        let seq: Vec<_> = extra.iter().map(|(m, p, s, b)| (*m, *p, s.to_string(), vec![b.clone()])).collect();
        self.routes_seq(&seq);
    }

    /// As `routes`, each route answering its bodies in turn, the last repeated; a status `curlN`
    /// makes curl exit N (E-GHFLOW.W3).
    pub(crate) fn routes_seq(&self, extra: &[(&str, &str, String, Vec<serde_json::Value>)]) {
        let mut all = vec![(
            "POST",
            "/app/installations/456/access_tokens",
            "201".to_string(),
            vec![serde_json::json!({"token": TOKEN, "expires_at": "2026-11-15T12:00:00Z"})],
        )];
        all.extend(extra.iter().cloned());
        for e in std::fs::read_dir(&self.mock).unwrap() {
            std::fs::remove_file(e.unwrap().path()).unwrap();
        }
        let mut table = String::new();
        for (i, (m, p, s, bodies)) in all.iter().enumerate() {
            let files: Vec<String> = bodies
                .iter()
                .enumerate()
                .map(|(j, b)| {
                    std::fs::write(self.mock.join(format!("r{i}-{j}.json")), b.to_string()).unwrap();
                    format!("r{i}-{j}.json")
                })
                .collect();
            table.push_str(&format!("{m} {p} {s} {}\n", files.join(",")));
        }
        std::fs::write(self.mock.join("routes"), table).unwrap();
        std::fs::write(self.mock.join("log"), "").unwrap();
    }

    /// GitHub's Link header for answer `j` of route `i` (counted from 1 after the token exchange):
    /// the list's last page is `last` (E-GHFLOW.W3, security review I1).
    pub(crate) fn link_last(&self, i: usize, j: usize, path: &str, last: u32) {
        let h = format!("HTTP/2 200\r\nLink: <https://api.github.com{path}?per_page=100&page=2>; rel=\"next\", <https://api.github.com{path}?per_page=100&page={last}>; rel=\"last\"\r\n\r\n");
        std::fs::write(self.mock.join(format!("r{i}-{j}.json.hdr")), h).unwrap();
    }

    /// The requests the script sent since the last `routes`.
    pub(crate) fn log(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.mock.join("log")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    pub(crate) fn calls(&self) -> Vec<String> {
        self.log().iter().map(|r| format!("{} {}", r["method"].as_str().unwrap(), r["path"].as_str().unwrap())).collect()
    }

    pub(crate) fn path(&self) -> String {
        common::test_path(&common::path_with(Path::new(KEEL).parent().unwrap()), Some(&self.bin))
    }

    /// The script in the scratch repository, as the router runs it; every run is checked for secrets.
    pub(crate) fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.run_in(&self.repo, args, env)
    }

    /// As `run_env`, with the script of the checkout `dir` (the main one, or a linked worktree).
    pub(crate) fn run_in(&self, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut c = common::cmd("sh");
        c.arg(dir.join("scripts/router-gh.sh")).args(args).current_dir(dir);
        self.github_env(&mut c);
        c.envs(env.iter().copied());
        let o = c.output().unwrap();
        self.assert_no_secret(&o);
        let left: Vec<_> = std::fs::read_dir(&self.tmp).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(left.is_empty(), "the script wrote {left:?} to the shared $TMPDIR");
        let run = self.home.join(".config/keel/router-gh/run");
        let left: Vec<_> = std::fs::read_dir(&run).map(|d| d.map(|e| e.unwrap().file_name()).collect()).unwrap_or_default();
        assert!(left.is_empty(), "the script left {left:?} in its run dir");
        o
    }

    /// The scratch GitHub every script run here sees: keel on PATH, the temp home with the App,
    /// the shared $TMPDIR the tests check, the fake curl and its routes, and origin as GitHub's git.
    pub(crate) fn github_env(&self, c: &mut std::process::Command) {
        c.env("PATH", self.path()).env("HOME", &self.home).env("TMPDIR", &self.tmp).env("MOCK_GH", &self.mock);
        c.env("ROUTER_GH_CURL", self.bin.join("fake-curl"))
            .env("ROUTER_GH_GIT", format!("file://{}", self.root.path().canonicalize().unwrap().join("gh").display()));
    }

    /// A worktree entry git did not make, forged in the shared git dir as a worker could (security
    /// review round 2, X1/N1): `$g/worktrees/<id>` naming `path`, with `branch` checked out.
    pub(crate) fn forge_worktree(&self, id: &str, path: &Path, branch: &str) {
        let admin = self.repo.join(".git/worktrees").join(id);
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(admin.join("gitdir"), format!("{}\n", path.join(".git").display())).unwrap();
        std::fs::write(admin.join("HEAD"), format!("ref: refs/heads/{branch}\n")).unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
    }

    /// GitHub merges `what` into epic/E (a merge commit made in a clone of origin); the merge's sha.
    pub(crate) fn github_merges(&self, what: &str) -> String {
        let merger = self.root.path().join("merger");
        if !merger.exists() {
            git(self.root.path(), &["clone", "-q", self.origin.to_str().unwrap(), merger.to_str().unwrap()]);
        }
        git(&self.repo, &["push", "-q", "--no-verify", self.origin.to_str().unwrap(), what]);
        git(&merger, &["fetch", "-q", "origin"]);
        git(&merger, &["checkout", "-q", "-B", "epic/E", "origin/epic/E"]);
        git(&merger, &["merge", "-q", "--no-ff", "--no-verify", "-m", what, &format!("origin/{what}")]);
        git(&merger, &["push", "-q", "--no-verify", "origin", "epic/E"]);
        git(&merger, &["rev-parse", "HEAD"])
    }

    pub(crate) fn run(&self, args: &[&str]) -> Output {
        self.run_env(args, &[])
    }

    /// No output carries the key, a JWT the script sent, or the token.
    pub(crate) fn assert_no_secret(&self, o: &Output) {
        let out = text(o);
        assert!(!out.contains(TOKEN) && !out.contains("ghs_"), "the token leaked: {out}");
        self.assert_no_key_or_jwt(&out);
    }

    /// No output carries a line of the key or a JWT the script sent (its signature, or any JSON
    /// part, which starts `eyJ`): every check of `assert_no_secret` but the token's own.
    pub(crate) fn assert_no_key_or_jwt(&self, out: &str) {
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

    pub(crate) fn pr(&self, number: u64, login: &str, head_repo: &str, head_sha: &str, draft: bool) -> serde_json::Value {
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

pub(crate) fn stdout(o: &Output) -> String {
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
pub(crate) fn assert_auth(gh: &Gh) {
    let log = gh.log();
    assert_eq!(log[0]["path"], "/app/installations/456/access_tokens");
    assert_eq!(serde_json::from_str::<serde_json::Value>(log[0]["body"].as_str().unwrap()).unwrap(), serde_json::json!({"repositories": ["Keel"]}));
    assert_jwt(gh, log[0]["auth"].as_str().unwrap());
    for r in &log[1..] {
        assert_eq!(r["auth"], format!("Bearer {TOKEN}"), "{r}");
    }
    // the response, the token's first home, lands under the key dir workers cannot read (X2)
    let run = format!("output = \"{}/.config/keel/router-gh/run/router-gh.", gh.home.display());
    for r in &log {
        let c = r["config"].as_str().unwrap();
        assert!(c.contains("proto = \"=https\"") && c.contains("max-time = 60"), "{c}");
        assert!(c.contains(&run), "the run dir is not under the key dir: {c}");
    }
}

pub(crate) fn body_of(r: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(r["body"].as_str().unwrap()).unwrap()
}

/// push lands the branch and every notes ref on origin, as the App (a token was minted), never
/// forced; a second push changes nothing. No pushed code runs (security review B1): Keel's hooks are
/// installed, yet neither the pre-push hook nor the branch's `[check]` commands ran.
#[test]
fn push_lands_the_branch_and_its_notes_and_runs_none_of_the_pushed_code() {
    let gh = Gh::new();
    assert!(!git(&gh.repo, &["config", "core.hooksPath"]).is_empty(), "Keel's hooks are installed");
    let tip = gh.sha("wu/E/x");
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("PUSHED branch=wu/E/x sha={tip} state=new"));
    assert_eq!(std::fs::read_dir(&gh.marks).unwrap().count(), 0, "a [check] command of the pushed branch ran");
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x").as_deref(), Some(tip.as_str()));
    for n in ["cr", "test"] {
        assert_eq!(gh.origin_sha(&format!("refs/notes/{n}")), Some(gh.sha(&format!("refs/notes/{n}"))), "notes {n}");
    }
    assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens"]);
    assert_auth(&gh);
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(stdout(&o), format!("PUSHED branch=wu/E/x sha={tip} state=unchanged"), "{}", text(&o));
    assert_eq!(std::fs::read_dir(&gh.marks).unwrap().count(), 0);
}

/// A wu/ tip without its test/v1 note (its worker's sandboxed keel check) is refused before any
/// request; a note for another tip does not count.
#[test]
fn a_wu_tip_without_its_test_note_is_refused() {
    let gh = Gh::new();
    let old = gh.sha("wu/E/x");
    write(&gh.repo, "src/d.txt", "d");
    commit(&gh.repo, "untested");
    let tip = gh.sha("wu/E/x");
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains(&format!("the tip {tip} of wu/E/x carries no test/v1 note")), "{}", text(&o));
    git(&gh.repo, &["notes", "--ref=test", "add", "-m", &format!(r#"{{"schema":"test/v1","tip":"{old}"}}"#), &tip]);
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(o.status.code(), Some(2), "a copied note: {}", text(&o));
    assert!(gh.log().is_empty() && gh.origin_sha("refs/heads/wu/E/x").is_none());
    test_note(&gh.repo, &tip);
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
}

/// An epic push GitHub already holds walks the range, as the pre-push hook would, with keel's
/// code-free gate: an ungated code commit refuses the push; a docs-only one passes. A new epic
/// branch has no range on GitHub to walk.
#[test]
fn an_epic_push_passes_the_range_walk_first() {
    let gh = Gh::new();
    git(&gh.repo, &["switch", "-q", "epic/E"]);
    let old = gh.sha("epic/E");
    write(&gh.repo, "README", "a docs-only epic commit");
    commit(&gh.repo, "readme");
    let o = gh.run(&["push", "epic/E"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(stdout(&o).ends_with("state=updated"), "{}", text(&o));
    let landed = gh.sha("epic/E");
    assert_ne!(landed, old);
    write(&gh.repo, "src/e.txt", "ungated code");
    commit(&gh.repo, "code on the epic");
    let o = gh.run(&["push", "epic/E"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("FAIL") && text(&o).contains(&format!("the range walk refused {landed}..epic/E")), "{}", text(&o));
    assert_eq!(gh.origin_sha("refs/heads/epic/E"), Some(landed));
    assert_eq!(std::fs::read_dir(&gh.marks).unwrap().count(), 0, "the range walk ran a [check] command");
    git(&gh.repo, &["switch", "-q", "-c", "epic/F"]);
    let o = gh.run(&["push", "epic/F"]);
    assert!(o.status.success() && stdout(&o).ends_with("state=new"), "{}", text(&o));
}

/// The user's global git config is not read (security review I1): its push.followTags would push
/// a tag, and its url.insteadOf would send the push elsewhere.
#[test]
fn the_users_global_git_config_cannot_widen_or_reroute_a_push() {
    let gh = Gh::new();
    let gh_dir = gh.origin.parent().unwrap().parent().unwrap().display().to_string();
    let elsewhere = gh.root.path().canonicalize().unwrap().join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("own/Keel.git")).unwrap();
    git(&elsewhere.join("own/Keel.git"), &["init", "-q", "--bare"]);
    write(
        &gh.home,
        ".gitconfig",
        &format!(
            "[push]\n\tfollowTags = true\n[url \"file://{}/\"]\n\tinsteadOf = file://{gh_dir}/\n\tpushInsteadOf = file://{gh_dir}/\n",
            elsewhere.display()
        ),
    );
    git(&gh.repo, &["tag", "-a", "-m", "t", "v9", "wu/E/x"]);
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), Some(gh.sha("wu/E/x")));
    assert_eq!(gh.origin_sha("refs/tags/v9"), None, "a tag was pushed");
    assert!(git(&elsewhere.join("own/Keel.git"), &["for-each-ref"]).is_empty(), "the push was rerouted");
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
    test_note(&gh.repo, &gh.sha("wu/E/x"));
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
    test_note(&gh.repo, &gh.sha("wu/E/x"));
    let tip = gh.sha("wu/E/x");
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let out = stdout(&o);
    assert!(out.lines().any(|l| l.starts_with("WARN notes ref refs/notes/cr not pushed: ")), "{out}");
    assert!(out.ends_with(&format!("PUSHED branch=wu/E/x sha={tip} state=updated")), "{out}");
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), Some(tip));
    assert_eq!(gh.origin_sha("refs/notes/test"), Some(gh.sha("refs/notes/test")), "the other notes refs still went");
    assert_eq!(gh.origin_sha("refs/notes/alt"), None, "only Keel's notes kinds are pushed (N3)");
    // E-GHFLOW2.W3 (security review I2): GitHub's cr line is never read into the checkout
    let o = common::cmd("git").args(["notes", "--ref=cr", "list", &epic]).current_dir(&gh.repo).output().unwrap();
    assert!(!o.status.success(), "GitHub's cr note reached the checkout");
}

/// A github-tier mr/v1 note as the sign-off writer makes it, on `tip`.
fn approval_note(tip: &str, id: u64) -> String {
    serde_json::json!({"schema": "mr/v1", "tip": tip, "base": "b", "level": "wu", "signer": OWNER_ID.to_string(), "tier": "github",
        "change_hash": "sha256:00", "review": {"id": id, "url": format!("https://github.com/own/Keel/pull/7#pullrequestreview-{id}"), "commit": tip}})
    .to_string()
}

/// E-GHFLOW2.W3, security review I2: GitHub's sign-offs reach the checkout add-only. A note GitHub
/// holds on a commit the checkout has and holds no mr note for is taken when it reads as the writer's;
/// any other note is not. Nothing GitHub edits or deletes reaches a note here, and the checkout's
/// refs/notes/mr replaces GitHub's, leased; the report note rides along; no temp ref is left.
#[test]
fn push_takes_githubs_sign_offs_add_only_and_never_an_edit_or_a_deletion() {
    let gh = Gh::new();
    let (tip, epic, main) = (gh.sha("wu/E/x"), gh.sha("epic/E"), gh.sha("main"));
    git(&gh.repo, &["notes", "--ref=mr", "add", "-m", r#"{"by":"local"}"#, &tip]);
    git(&gh.repo, &["notes", "--ref=report", "add", "-m", "<html>report</html>", &tip]);
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
    // on GitHub: the writer signs the epic tip; someone edits the tip's note, writes a username-tier
    // note on main, a github-tier note whose review names another commit, and a cr note
    git(&gh.origin, &["notes", "--ref=mr", "add", "-m", &approval_note(&epic, 41), &epic]);
    git(&gh.origin, &["notes", "--ref=mr", "add", "-f", "-m", &approval_note(&tip, 42), &tip]);
    let typed = serde_json::json!({"schema": "mr/v1", "tip": main, "tier": "username", "signer": "vuluu"}).to_string();
    git(&gh.origin, &["notes", "--ref=mr", "add", "-m", &typed, &main]);
    git(&gh.origin, &["notes", "--ref=cr", "add", "-m", "{}", &epic]);
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let out = stdout(&o);
    assert!(out.contains(&format!("WARN GitHub's mr note on {main} not taken: not a sign-off by approval")), "{out}");
    assert!(out.ends_with(&format!("PUSHED branch=wu/E/x sha={tip} state=unchanged")), "{out}");
    let local = |rev: &str| common::cmd("git").args(["notes", "--ref=mr", "show", rev]).current_dir(&gh.repo).output().unwrap();
    assert_eq!(String::from_utf8(local(&epic).stdout).unwrap().trim(), approval_note(&epic, 41), "the writer's sign-off was taken");
    assert_eq!(String::from_utf8(local(&tip).stdout).unwrap().trim(), r#"{"by":"local"}"#, "a one-sided edit on GitHub never reaches the checkout");
    assert!(!local(&main).status.success(), "a typed-tier note from GitHub is never taken");
    let o = common::cmd("git").args(["notes", "--ref=cr", "list", &epic]).current_dir(&gh.repo).output().unwrap();
    assert!(!o.status.success(), "no other kind is read from GitHub");
    assert_eq!(gh.origin_sha("refs/notes/mr"), Some(gh.sha("refs/notes/mr")), "the checkout's ref replaced GitHub's, leased");
    assert_eq!(git(&gh.origin, &["notes", "--ref=mr", "show", &tip]), r#"{"by":"local"}"#);
    assert_eq!(gh.origin_sha("refs/notes/report"), Some(gh.sha("refs/notes/report")), "the report note is pushed");
    assert_eq!(
        git(&gh.repo, &["for-each-ref", "--format=%(refname)", "refs/notes/keel-gh/"]),
        "refs/notes/keel-gh/pushed-mr",
        "no temp ref is left, only the snapshot"
    );
    // a one-sided deletion on GitHub: the checkout keeps its note and puts it back
    git(&gh.origin, &["notes", "--ref=mr", "remove", &tip]);
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8(local(&tip).stdout).unwrap().trim(), r#"{"by":"local"}"#);
    assert_eq!(git(&gh.origin, &["notes", "--ref=mr", "show", &tip]), r#"{"by":"local"}"#);
    // a forged writer note on a commit the checkout lacks a note for, whose review names another commit
    let gh = Gh::new();
    let (tip, epic) = (gh.sha("wu/E/x"), gh.sha("epic/E"));
    let mut forged: serde_json::Value = serde_json::from_str(&approval_note(&epic, 43)).unwrap();
    forged["review"]["commit"] = serde_json::json!(tip);
    git(&gh.origin, &["notes", "--ref=mr", "add", "-m", &forged.to_string(), &epic]);
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success() && stdout(&o).contains("not taken"), "{}", text(&o));
    let o = common::cmd("git").args(["notes", "--ref=mr", "list", &epic]).current_dir(&gh.repo).output().unwrap();
    assert!(!o.status.success());
}

/// Security review round 2 F1 and round 3 I3-1: a sign-off GitHub holds on a commit this checkout
/// lacks (the owner pressed "Update branch" and approved the new head) has that commit fetched by sha,
/// so the push carries the note on and never erases it. A writer-shaped note on a commit GitHub does
/// not serve is not taken and holds nothing back: the push goes on, every time.
#[test]
fn a_sign_off_on_a_commit_only_github_has_is_fetched_and_kept_and_an_unserved_one_holds_nothing() {
    let gh = Gh::new();
    // GitHub serves a commit reachable from one of its branches by sha, as github.com does
    git(&gh.origin, &["config", "uploadpack.allowReachableSHA1InWant", "true"]);
    git(&gh.repo, &["notes", "--ref=mr", "add", "-m", r#"{"by":"local"}"#, &gh.sha("wu/E/x")]);
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
    // GitHub makes a commit this checkout never fetched, and the writer signs it; a note also names a
    // commit GitHub never had (made in a clone, its note pushed, the commit never), as a planted one would
    let other = gh.root.path().join("other");
    git(gh.root.path(), &["clone", "-q", gh.origin.to_str().unwrap(), other.to_str().unwrap()]);
    git(&other, &["checkout", "-q", "-b", "wu/E/x", "origin/wu/E/x"]);
    write(&other, "src/b.txt", "update branch");
    commit(&other, "Merge branch 'epic/E' into wu/E/x");
    git(&other, &["push", "-q", "--no-verify", "origin", "wu/E/x:refs/heads/wu/E/x-updated"]);
    let only_github = git(&other, &["rev-parse", "HEAD"]);
    write(&other, "src/g.txt", "never pushed");
    commit(&other, "a commit GitHub never had");
    let unserved = git(&other, &["rev-parse", "HEAD"]);
    git(&other, &["fetch", "-q", "origin", "refs/notes/mr:refs/notes/mr"]);
    git(&other, &["notes", "--ref=mr", "add", "-m", &approval_note(&only_github, 50), &only_github]);
    git(&other, &["notes", "--ref=mr", "add", "-m", &approval_note(&unserved, 51), &unserved]);
    git(&other, &["push", "-q", "--no-verify", "origin", "refs/notes/mr:refs/notes/mr"]);
    assert!(!common::cmd("git").args(["cat-file", "-e", &unserved]).current_dir(&gh.origin).output().unwrap().status.success(), "GitHub never had it");
    write(&gh.repo, "src/c.txt", "c");
    commit(&gh.repo, "more");
    let tip = gh.sha("wu/E/x");
    test_note(&gh.repo, &tip);
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let out = stdout(&o);
    assert_eq!(
        out.lines().filter(|l| l.starts_with("WARN")).collect::<Vec<_>>(),
        [format!("WARN GitHub's mr note on {unserved} not taken: GitHub serves no such commit")],
        "{out}"
    );
    assert!(out.ends_with(&format!("PUSHED branch=wu/E/x sha={tip} state=updated")), "{out}");
    assert_eq!(git(&gh.repo, &["notes", "--ref=mr", "show", &only_github]), approval_note(&only_github, 50), "the sign-off and its commit came here");
    assert_eq!(gh.origin_sha("refs/notes/mr"), Some(gh.sha("refs/notes/mr")), "refs/notes/mr was pushed: nothing holds it back");
    assert_eq!(git(&gh.origin, &["notes", "--ref=mr", "show", &only_github]), approval_note(&only_github, 50), "the push kept GitHub's sign-off");
    assert_eq!(gh.sha("refs/notes/keel-gh/pushed-mr"), gh.sha("refs/notes/mr"), "the snapshot is the value pushed");
    // the next push is not held either
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success() && stdout(&o).ends_with("state=unchanged"), "{}", text(&o));
}

/// Security review round 2, F2: a sign-off removed here stays removed. Only what GitHub added since
/// this checkout last pushed refs/notes/mr is taken, so the next push takes the removal to GitHub;
/// a sign-off the writer adds after that is still taken.
#[test]
fn a_sign_off_removed_here_is_not_brought_back_by_the_next_push() {
    let gh = Gh::new();
    let (tip, epic) = (gh.sha("wu/E/x"), gh.sha("epic/E"));
    git(&gh.repo, &["notes", "--ref=mr", "add", "-m", &approval_note(&tip, 60), &tip]);
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
    assert_eq!(git(&gh.origin, &["notes", "--ref=mr", "show", &tip]), approval_note(&tip, 60));
    git(&gh.repo, &["notes", "--ref=mr", "remove", &tip]);
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let local = |rev: &str| common::cmd("git").args(["notes", "--ref=mr", "list", rev]).current_dir(&gh.repo).output().unwrap().status.success();
    assert!(!local(&tip), "the removed sign-off came back");
    let o = common::cmd("git").args(["notes", "--ref=mr", "list", &tip]).current_dir(&gh.origin).output().unwrap();
    assert!(!o.status.success(), "the removal reached GitHub");
    // the writer signs again on GitHub afterwards: a new note, taken
    git(&gh.origin, &["notes", "--ref=mr", "add", "-m", &approval_note(&tip, 61), &tip]);
    git(&gh.origin, &["notes", "--ref=mr", "add", "-m", &approval_note(&epic, 62), &epic]);
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
    assert!(local(&tip) && local(&epic), "a sign-off GitHub added since the last push is taken");
}

/// Security review round 2, F6: GitHub's refs/notes/mr moves between the read and the push (the
/// writer signs meanwhile): the leased push of refs/notes/mr fails with a WARN, the writer's note
/// survives, the branch lands, and the next push takes the note in.
#[test]
fn a_sign_off_written_on_github_during_the_push_survives_it() {
    let gh = Gh::new();
    let (tip, epic) = (gh.sha("wu/E/x"), gh.sha("epic/E"));
    git(&gh.repo, &["notes", "--ref=mr", "add", "-m", r#"{"by":"local"}"#, &tip]);
    assert!(gh.run(&["push", "wu/E/x"]).status.success());
    git(&gh.repo, &["notes", "--ref=mr", "add", "-f", "-m", r#"{"by":"local","again":true}"#, &tip]);
    let hook = gh.origin.join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    exe(
        &hook,
        &format!(
            "#!/bin/sh\n[ -f moved ] && exit 0\ntouch moved\nunset GIT_QUARANTINE_PATH GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES\nGIT_AUTHOR_NAME=w GIT_AUTHOR_EMAIL=w@w GIT_COMMITTER_NAME=w GIT_COMMITTER_EMAIL=w@w git notes --ref=mr add -m '{}' {epic} >&2\nexit 0\n",
            approval_note(&epic, 70)
        ),
    );
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let out = stdout(&o);
    assert!(gh.origin.join("moved").exists(), "the writer signed during the push");
    assert!(out.lines().any(|l| l.starts_with("WARN notes ref refs/notes/mr not pushed: ")), "{out}");
    assert_eq!(git(&gh.origin, &["notes", "--ref=mr", "show", &epic]), approval_note(&epic, 70), "the writer's note survived");
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success() && !stdout(&o).contains("WARN"), "{}", text(&o));
    assert_eq!(git(&gh.repo, &["notes", "--ref=mr", "show", &epic]), approval_note(&epic, 70), "taken in at the next push");
    assert_eq!(git(&gh.origin, &["notes", "--ref=mr", "show", &tip]), r#"{"by":"local","again":true}"#);
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
    // each changed doc's page on the Pages site, where scripts/pages-build.sh writes it (E-GHFLOW2.W8)
    assert!(body.contains("| Doc | Role | Status | Page |\n|---|---|---|---|\n"), "{body}");
    assert!(body.contains("| wu-plan | ready | [page](https://own.github.io/Keel/wu/E/x/docs/work/x-plan.html) |\n"), "{body}");
    assert!(body.contains("| wu-result | draft | [page](https://own.github.io/Keel/wu/E/x/docs/work/x-result.html) |\n"), "{body}");
    assert!(body.contains("| yes | — | PASS, round 2 | — | — |"), "{body}");
    for active in ["@owner", "`tick`", "[x]", "*b*", "#h", "| cell"] {
        assert!(!body.contains(active), "{active} survived: {body}");
    }
}

/// The page link's host is the owner in lower case, as GitHub serves a project site; its path keeps
/// the repository name, the branch and the doc's repository path (E-GHFLOW2.W8).
#[test]
fn pr_open_links_each_doc_s_page_at_the_owner_s_lower_case_host() {
    let gh = Gh::new();
    let cfg = std::fs::read_to_string(gh.repo.join(".keel/config.toml")).unwrap();
    assert!(cfg.contains("repo = \"own/Keel\""));
    write(&gh.repo, ".keel/config.toml", &cfg.replace("repo = \"own/Keel\"", "repo = \"Own-Org/Keel.docs\""));
    commit(&gh.repo, "the repository renamed");
    let tip = gh.sha("wu/E/x");
    git(&gh.repo, &["push", "-q", "--no-verify", gh.origin.to_str().unwrap(), "wu/E/x"]);
    gh.routes(&[
        ("GET", "/repos/Own-Org/Keel.docs/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": tip}})),
        ("GET", "/repos/Own-Org/Keel.docs/pulls?state=open&head=Own-Org:wu/E/x&per_page=100", 200, serde_json::json!([])),
        ("POST", "/repos/Own-Org/Keel.docs/pulls", 201, serde_json::json!({"number": 7, "html_url": "https://github.com/Own-Org/Keel.docs/pull/7"})),
    ]);
    let o = gh.run(&["pr-open", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let body = body_of(&gh.log()[3])["body"].as_str().unwrap().to_string();
    assert!(body.contains("[page](https://own-org.github.io/Keel.docs/wu/E/x/docs/work/x-plan.html)"), "{body}");
}

/// A page link only where scripts/pages-build.sh publishes the page (security review F4): a dot
/// part, docs/_report/, a part over 200 bytes or a path over 400 get "—", and router-gh.sh agrees
/// with the builder's own `--path` and `--folder` answers on every one.
#[test]
fn pr_open_links_a_page_only_where_the_builder_publishes_it() {
    let gh = Gh::new();
    let a = |n: usize| "a".repeat(n);
    let docs = [
        ("docs/work/ok.html".to_string(), true),
        ("docs/-dash.html".into(), true),
        (format!("docs/{}.html", a(195)), true),
        ("docs/.x/hidden.html".into(), false),
        ("docs/_report/r.html".into(), false),
        (format!("docs/{}.html", a(196)), false),
        (format!("docs/{}/{}.html", a(200), a(190)), false),
    ];
    for (f, _) in &docs {
        write(&gh.repo, f, "<html><head><title>t</title></head></html>\n");
    }
    commit(&gh.repo, "docs the builder may skip");
    let tip = gh.sha("wu/E/x");
    git(&gh.repo, &["push", "-q", "--no-verify", gh.origin.to_str().unwrap(), "wu/E/x"]);
    gh.routes(&[
        ("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": tip}})),
        ("GET", "/repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100", 200, serde_json::json!([])),
        ("POST", "/repos/own/Keel/pulls", 201, serde_json::json!({"number": 7, "html_url": "https://github.com/own/Keel/pull/7"})),
    ]);
    let o = gh.run(&["pr-open", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let body = body_of(&gh.log()[3])["body"].as_str().unwrap().to_string();
    let builder = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/pages-build.sh");
    let says = |args: &[&str]| common::cmd("sh").arg(&builder).args(args).output().unwrap().status.success();
    assert!(says(&["--folder", "wu/E/x"]));
    for (f, linked) in &docs {
        let row = body.lines().find(|l| l.contains(&format!("/blob/{tip}/{f})"))).unwrap_or_else(|| panic!("no row for {f}: {body}"));
        let want = if *linked { format!(" | [page](https://own.github.io/Keel/wu/E/x/{f}) |") } else { " | — |".to_string() };
        assert!(row.ends_with(&want), "{f}: {row}");
        assert_eq!(says(&["--path", f]), *linked, "router-gh.sh and the builder disagree on {f}");
    }
}

/// page_ok's branch rule, run from router-gh.sh's own text, is the builder's `--folder` rule on main,
/// an epic, a work unit and the edges between (review 1): every page link names a published folder.
#[test]
fn page_ok_links_a_page_on_exactly_the_branches_the_builder_publishes() {
    let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/router-gh.sh")).unwrap();
    let start = script.find("\npage_ok() {\n").expect("router-gh.sh defines page_ok") + 1;
    let len = script[start..].find("\n}\n").expect("page_ok ends") + 3;
    let page_ok = &script[start..start + len];
    let a = |n: usize| "a".repeat(n);
    // '|'-separated, so the empty name and names holding a space, tab or newline sit among the rest
    let names = "main|epic/E|wu/E/x|epic/E-1.2_x|epic/-E|epic/_E|wu/E/x.y|wu/E-1/x_2|epic/E.|wu/E/x-||Main|mainx|xmain|main/x|main/|/main|\
                 epic|epic/|epic/E/x|epic/.E|epic/a..b|epic//E|wu|wu/E|wu/E/|wu//x|wu/E/x/y|wu/.E/x|wu/E/.x|wu/E/..|release/1|\
                 feature/x|epic/E x|epic/E\tx|epic/E\nx|epic/E\n|epic/E*|epic/[E]|wu/E/x;rm|epic/É|refs/heads/main|origin/main";
    let mut branches: Vec<String> = names.split('|').map(String::from).collect();
    branches.extend([
        format!("epic/{}", a(200)),
        format!("epic/{}", a(201)),
        format!("wu/{}/{}", a(200), a(196)),
        format!("wu/{}/{}", a(200), a(197)),
        format!("wu/{}/x", a(201)),
    ]);
    let o = common::cmd("sh")
        .arg("-c")
        .arg(format!("{page_ok}for b do if page_ok \"$b\" docs/a.html; then echo 1; else echo 0; fi; done"))
        .arg("sh")
        .args(&branches)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let linked: Vec<bool> = String::from_utf8(o.stdout).unwrap().lines().map(|l| l == "1").collect();
    assert_eq!(linked.len(), branches.len());
    let builder = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/pages-build.sh");
    for (b, linked) in branches.iter().zip(&linked) {
        let published = common::cmd("sh").arg(&builder).args(["--folder", b]).output().unwrap().status.success();
        assert_eq!(*linked, published, "router-gh.sh and the builder disagree on branch {b:?}");
    }
    // the agreement is not both refusing everything: the three kinds are linked, their near misses not
    let at = |b: &str| linked[branches.iter().position(|x| x == b).unwrap()];
    for (b, want) in
        [("main", true), ("epic/E", true), ("wu/E/x", true), (&*format!("epic/{}", a(200)), true), ("epic/E/x", false), ("wu/E", false), ("mainx", false)]
    {
        assert_eq!(at(b), want, "{b}");
    }
}

/// The builder reads docs/ whatever [docs] root says, so a doc under another root gets no page link.
#[test]
fn pr_open_links_no_page_under_a_docs_root_other_than_docs() {
    let gh = Gh::new();
    let cfg = std::fs::read_to_string(gh.repo.join(".keel/config.toml")).unwrap();
    assert!(cfg.contains("root = \"docs/\""));
    write(&gh.repo, ".keel/config.toml", &cfg.replace("root = \"docs/\"", "root = \"pages/\""));
    write(&gh.repo, "pages/p.html", "<html><head><title>p</title></head></html>\n");
    commit(&gh.repo, "docs under pages/");
    let tip = gh.sha("wu/E/x");
    git(&gh.repo, &["push", "-q", "--no-verify", gh.origin.to_str().unwrap(), "wu/E/x"]);
    gh.routes(&[
        ("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": tip}})),
        ("GET", "/repos/own/Keel/pulls?state=open&head=own:wu/E/x&per_page=100", 200, serde_json::json!([])),
        ("POST", "/repos/own/Keel/pulls", 201, serde_json::json!({"number": 7, "html_url": "https://github.com/own/Keel/pull/7"})),
    ]);
    let o = gh.run(&["pr-open", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    let body = body_of(&gh.log()[3])["body"].as_str().unwrap().to_string();
    assert!(body.contains(&format!("| [p](https://github.com/own/Keel/blob/{tip}/pages/p.html) | — | — | — |")), "{body}");
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
    let user = serde_json::json!({"login": "lesslucifer", "id": OWNER_ID, "type": "User"});
    let with = |pr: serde_json::Value| {
        gh.routes(&[
            ("GET", "/repos/own/Keel/pulls/7", 200, pr),
            ("POST", "/graphql", 200, ready.clone()),
            ("GET", "/user/5499546", 200, user.clone()),
            ("POST", "/repos/own/Keel/pulls/7/requested_reviewers", 201, serde_json::json!({"number": 7})),
        ]);
        gh.run(&["pr-ready", "7"])
    };
    let o = with(gh.pr(7, BOT, "own/Keel", &tip, true));
    assert!(o.status.success(), "{}", text(&o));
    // E-GHFLOW.W3: GitHub asks each commander, by the login of their id, for a review
    assert_eq!(stdout(&o), format!("READY pr=7 head=wu/E/x sha={tip} state=ready reviewers=lesslucifer"));
    assert_eq!(
        gh.calls(),
        [
            "POST /app/installations/456/access_tokens",
            "GET /repos/own/Keel/pulls/7",
            "POST /graphql",
            "GET /user/5499546",
            "POST /repos/own/Keel/pulls/7/requested_reviewers"
        ]
    );
    assert_auth(&gh);
    let q = body_of(&gh.log()[2]);
    assert_eq!(q["variables"], serde_json::json!({"id": "PR_kwDOtest42"}));
    assert!(q["query"].as_str().unwrap().contains("markPullRequestReadyForReview(input: {pullRequestId: $id})"), "{q}");
    assert_eq!(body_of(&gh.log()[4]), serde_json::json!({"reviewers": ["lesslucifer"]}));
    // the pull request is ready already: a refused review request is a warning, never a failure
    gh.routes(&[
        ("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true)),
        ("POST", "/graphql", 200, ready.clone()),
        ("GET", "/user/5499546", 403, serde_json::json!({"message": "Resource not accessible by integration"})),
    ]);
    let o = gh.run(&["pr-ready", "7"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        stdout(&o),
        format!(
            "WARN no review request for commander 5499546: GitHub names no user login (HTTP 403)\nREADY pr=7 head=wu/E/x sha={tip} state=ready reviewers=none"
        )
    );
    gh.routes(&[
        ("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, BOT, "own/Keel", &tip, true)),
        ("POST", "/graphql", 200, ready.clone()),
        ("GET", "/user/5499546", 200, user.clone()),
        ("POST", "/repos/own/Keel/pulls/7/requested_reviewers", 422, serde_json::json!({"message": "Validation Failed"})),
    ]);
    let o = gh.run(&["pr-ready", "7"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("WARN review request for lesslucifer refused: HTTP 422\nREADY pr=7 head=wu/E/x sha={tip} state=ready reviewers=none"));

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
    // a worker cannot read the key, but it can name it (B2)
    std::os::unix::fs::symlink(gh.home.join(".config/keel/router-gh/app.pem"), dir.join("key.md")).unwrap();
    std::fs::hard_link(gh.root.path().join("elsewhere.md"), dir.join("hard.md")).unwrap();
    let o = common::cmd("mkfifo").arg(dir.join("fifo.md")).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    std::fs::write(dir.join("big.md"), "a".repeat(60_001)).unwrap();
    std::fs::write(dir.join("max.md"), "a".repeat(60_000)).unwrap();
    std::fs::write(dir.join("empty.md"), "").unwrap();
    std::fs::write(dir.join("bell.md"), "ding \x07\n").unwrap();
    std::fs::write(dir.join("cr.md"), "line\r\n").unwrap();
    std::fs::write(dir.join("nul.md"), b"a\0b\n").unwrap();
    std::fs::write(dir.join("latin1.md"), b"caf\xe9\n").unwrap();
    for (name, why) in [
        ("link", "must be a regular file, not a link"),
        ("key", "must be a regular file, not a link"),
        ("hard", "has more than one link: refused"),
        ("fifo", "must be a regular file, not a link"),
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
    assert!(o.status.code() == Some(2) && text(&o).contains("is a link or was moved: refused"), "{}", text(&o));
    // someone else's pull request: refused before any file is read
    gh.routes(&[("GET", "/repos/own/Keel/pulls/7", 200, gh.pr(7, "mallory", "own/Keel", &tip, true))]);
    let o = gh.run(&["post", "7", "big"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("not one the bot opened"), "{}", text(&o));
}

/// sync fetches GitHub's epic into refs/remotes/origin/epic/E and fast-forwards the local branch
/// by update-ref when no worktree has it checked out. When one does, no git runs there (round 2,
/// X1): sync says `fetched`, and that worktree's own session fast-forwards it. A local commit GitHub
/// lacks is not a fast-forward.
#[test]
fn sync_fetches_githubs_epic_and_moves_only_an_unchecked_out_branch() {
    let gh = Gh::new();
    let old = gh.sha("epic/E");
    let merged = gh.github_merges("wu/E/x");
    let o = gh.run(&["sync", "epic/E"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), format!("SYNCED branch=epic/E from={old} sha={merged} state=fast-forward"));
    assert_eq!((gh.sha("epic/E"), gh.sha("refs/remotes/origin/epic/E")), (merged.clone(), merged.clone()));
    assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens"]);
    let o = gh.run(&["sync", "epic/E"]);
    assert_eq!(stdout(&o), format!("SYNCED branch=epic/E sha={merged} state=unchanged"));

    // checked out in a worktree: fetched only; the worktree and the branch stay put
    let wt = gh.root.path().canonicalize().unwrap().join("epic-wt");
    git(&gh.repo, &["worktree", "add", "-q", wt.to_str().unwrap(), "epic/E"]);
    git(&gh.repo, &["switch", "-q", "-c", "wu/E/y", &merged]);
    write(&gh.repo, "src/y.txt", "y");
    commit(&gh.repo, "wu y");
    let merged2 = gh.github_merges("wu/E/y");
    let o = gh.run(&["sync", "epic/E"]);
    assert_eq!(
        stdout(&o),
        format!("SYNCED branch=epic/E from={merged} sha={merged2} state=fetched ref=refs/remotes/origin/epic/E worktree={}", wt.display()),
        "{}",
        text(&o)
    );
    assert_eq!((gh.sha("epic/E"), git(&wt, &["rev-parse", "HEAD"])), (merged.clone(), merged.clone()));
    assert_eq!(gh.sha("refs/remotes/origin/epic/E"), merged2);
    assert!(!wt.join("src/y.txt").exists());
    // the worktree's own session finishes it
    git(&wt, &["merge", "-q", "--ff-only", "origin/epic/E"]);
    assert!(wt.join("src/y.txt").exists());

    // a local commit GitHub lacks: not a fast-forward
    write(&wt, "local.txt", "l");
    commit(&wt, "local only");
    let local = git(&wt, &["rev-parse", "HEAD"]);
    git(&gh.repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
    let o = gh.run(&["sync", "epic/E"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("not a fast-forward"), "{}", text(&o));
    assert_eq!(gh.sha("epic/E"), local);
}

/// X1 reproduced (security review round 2): a worker can write the shared git dir, so it can forge
/// `$g/worktrees/<id>` entries. One names a repository the worker owns, holding epic/E, whose clean
/// and smudge filters and post-merge hook leave a mark: sync runs no git there (it runs git in no
/// worktree) and does not believe the entry. One at the key's parent does not refuse every call.
/// Entries that do not look as git makes them are not believed by post: a half-forged link, and a
/// later row naming the main checkout's .git (round 3, I1). Two-sided: an entry forged whole (both
/// halves) is believed, and post then posts the worker's own file, made inert; that residual is
/// policy, stated in the script's header.
#[test]
fn a_forged_worktree_entry_runs_nothing() {
    let gh = Gh::new();
    let base = gh.root.path().canonicalize().unwrap();
    let mark = base.join("evil-ran");
    let evil = base.join("evil");
    git(&base, &["init", "-q", evil.to_str().unwrap()]);
    let touch = format!("sh -c 'touch {}; cat'", common::sh_quote(&mark.to_string_lossy()));
    for (k, v) in [("filter.x.clean", touch.as_str()), ("filter.x.smudge", touch.as_str()), ("filter.x.required", "true")] {
        git(&evil, &["config", k, v]);
    }
    write(&evil, ".gitattributes", "* filter=x\n");
    write(&evil, "f.txt", "f");
    exe(&evil.join(".git/hooks/post-merge"), &format!("#!/bin/sh\ntouch {}\n", common::sh_quote(&mark.to_string_lossy())));
    git(&evil, &["add", "-A"]);
    git(&evil, &["commit", "-q", "--no-verify", "-m", "evil"]);
    std::fs::remove_file(&mark).ok();
    write(&evil, "f.txt", "changed, so status would run the clean filter");
    gh.forge_worktree("evil", &evil, "epic/E");
    gh.forge_worktree("home", &gh.home.join(".config"), "wu/E/z");
    let p = base.join("p");
    write(&p, ".router-gh/status.md", "planted\n");
    gh.forge_worktree("p", &p, "wu/E/y");
    let listed = git(&gh.repo, &["worktree", "list", "--porcelain"]);
    assert!(listed.contains(&format!("worktree {}", evil.display())) && listed.contains("branch refs/heads/epic/E"), "the forge took: {listed}");

    let merged = gh.github_merges("wu/E/x");
    let o = gh.run(&["sync", "epic/E"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(stdout(&o).ends_with(&format!("sha={merged} state=fast-forward")), "a forged entry was believed: {}", text(&o));
    assert!(!mark.exists(), "git ran in the forged worktree");
    assert_eq!(std::fs::read_to_string(evil.join("f.txt")).unwrap(), "changed, so status would run the clean filter");

    // the key-location check ignores a forged entry at the key's parent
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));

    // post believes no forged entry for the head branch
    git(&gh.repo, &["branch", "wu/E/y", "wu/E/x"]);
    let mut pr = gh.pr(7, BOT, "own/Keel", &gh.sha("wu/E/x"), true);
    pr["head"]["ref"] = "wu/E/y".into();
    gh.routes(&[("GET", "/repos/own/Keel/pulls/7", 200, pr), ("POST", "/repos/own/Keel/issues/7/comments", 201, serde_json::json!({"id": 3}))]);
    let o = gh.run(&["post", "7", "status"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("no worktree of this repository has wu/E/y checked out"), "{}", text(&o));
    assert!(p.join(".router-gh/status.md").exists());
    assert_eq!(gh.calls().len(), 2);
    assert!(!mark.exists());

    // a later row naming the main checkout's .git is not the main checkout (round 3, I1)
    gh.forge_worktree("mainlike", &gh.repo, "wu/E/w");
    git(&gh.repo, &["branch", "wu/E/w", "wu/E/x"]);
    write(&gh.repo, ".router-gh/status.md", "the main checkout's file\n");
    let mut pr = gh.pr(7, BOT, "own/Keel", &gh.sha("wu/E/x"), true);
    pr["head"]["ref"] = "wu/E/w".into();
    gh.routes(&[("GET", "/repos/own/Keel/pulls/7", 200, pr), ("POST", "/repos/own/Keel/issues/7/comments", 201, serde_json::json!({"id": 4}))]);
    let o = gh.run(&["post", "7", "status"]);
    assert!(o.status.code() == Some(2) && text(&o).contains("no worktree of this repository has wu/E/w checked out"), "{}", text(&o));
    assert!(gh.repo.join(".router-gh/status.md").exists());

    // the residual: an entry forged whole is believed; post posts that directory's file, inert
    let q = base.join("q");
    write(&q, ".router-gh/status.md", "spoofed @owner\n");
    std::fs::write(q.join(".git"), format!("gitdir: {}\n", gh.repo.join(".git/worktrees/q").display())).unwrap();
    gh.forge_worktree("q", &q, "wu/E/v");
    git(&gh.repo, &["branch", "wu/E/v", "wu/E/x"]);
    let mut pr = gh.pr(7, BOT, "own/Keel", &gh.sha("wu/E/x"), true);
    pr["head"]["ref"] = "wu/E/v".into();
    gh.routes(&[("GET", "/repos/own/Keel/pulls/7", 200, pr), ("POST", "/repos/own/Keel/issues/7/comments", 201, serde_json::json!({"id": 5}))]);
    let o = gh.run(&["post", "7", "status"]);
    assert!(o.status.success() && stdout(&o).contains(&format!("file={}/.router-gh/status.md", q.display())), "{}", text(&o));
    assert!(body_of(&gh.log()[2])["body"].as_str().unwrap().ends_with("spoofed @\u{2060}owner\n"));
    assert!(!mark.exists());
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
    chmod(&key, 0o600);
    // a second name for the key (a hard link a worker could read through) and a directory others may write (N4)
    std::fs::hard_link(&key, gh.root.path().join("second.pem")).unwrap();
    check("has 2 links: remove the others");
    std::fs::remove_file(gh.root.path().join("second.pem")).unwrap();
    for mode in [0o770, 0o702, 0o777] {
        chmod(&kdir, mode);
        check("must be a directory no one else may write");
    }
    chmod(&kdir, 0o750);
    chmod(&key, 0o400);
    assert!(gh.run(&["push", "wu/E/x"]).status.success(), "0400 in a 0750 directory is fine");
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
    // a token GitHub returns that is not one is refused, not used; the error names the status and
    // the length only (E-GHFLOW2.W1)
    std::fs::write(gh.mock.join("routes"), "POST /app/installations/456/access_tokens 201 t.json\n").unwrap();
    let long = format!("ghs_{}", "a".repeat(4093));
    for (body, len) in [
        (serde_json::json!({"token": "ghs_x\"; rm -rf /"}), 16),
        (serde_json::json!({"token": "ghs_x\"y"}), 7),
        (serde_json::json!({"token": "ghs_x y"}), 7),
        (serde_json::json!({"token": "ghs_x'y"}), 7),
        (serde_json::json!({"token": "ghs_x\\y"}), 7),
        (serde_json::json!({"token": "ghs_x\ny"}), 7),
        (serde_json::json!({"token": "ghs_x\u{e9}y"}), 8),
        (serde_json::json!({"token": long}), 4097),
        (serde_json::json!({"token": ""}), 0),
        (serde_json::json!({"token": 12345678901234567890u64}), 0),
        (serde_json::json!({"expires_at": "2026-11-15T12:00:00Z"}), 0),
    ] {
        std::fs::write(gh.mock.join("t.json"), body.to_string()).unwrap();
        let o = gh.run(&["push", "wu/E/x"]);
        assert_eq!(o.status.code(), Some(1), "{body}: {}", text(&o));
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            format!("router-gh: GitHub returned no usable installation token (HTTP 201, a token of {len} bytes)\n"),
            "{body}"
        );
        assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), None);
    }
}

/// A token is opaque (E-GHFLOW2.W1): the legacy 40-character shape and the stateless one both
/// push, each riding the API calls as it came, and the longest token taken, 4096 bytes, too.
#[test]
fn a_legacy_and_a_stateless_token_both_push() {
    let long = format!("ghs_123_{}.{}.{}", "a-".repeat(500), "b_".repeat(1000), "c".repeat(1086));
    assert_eq!(long.len(), 4096);
    for t in [LEGACY_TOKEN, TOKEN, long.as_str()] {
        let gh = Gh::new();
        let tip = gh.sha("wu/E/x");
        std::fs::write(gh.mock.join("r0-0.json"), serde_json::json!({"token": t, "expires_at": "2026-11-15T12:00:00Z"}).to_string()).unwrap();
        let o = gh.run(&["push", "wu/E/x"]);
        assert_eq!(stdout(&o), format!("PUSHED branch=wu/E/x sha={tip} state=new"), "{}", text(&o));
        assert!(!text(&o).contains(t), "the token leaked");
        // pr-open's next call carries the token exactly as GitHub minted it
        gh.routes(&[("GET", "/repos/own/Keel/git/ref/heads/wu/E/x", 200, serde_json::json!({"object": {"sha": "0"}}))]);
        std::fs::write(gh.mock.join("r0-0.json"), serde_json::json!({"token": t}).to_string()).unwrap();
        let o = gh.run(&["pr-open", "wu/E/x"]);
        assert_eq!(o.status.code(), Some(2), "{}", text(&o));
        assert_eq!(gh.log()[1]["auth"], format!("Bearer {t}"));
    }
}

/// An installation token in GitHub's stateless format (`ghs_<app id>_<JWT>`, about 520 characters,
/// base64url parts joined by dots), the shape GitHub mints since 2026-04-27 (E-GHFLOW2.W1).
pub(crate) fn stateless_token() -> String {
    let part = |seed: &str, n: usize| seed.repeat(n / seed.len() + 1)[..n].to_string();
    format!("ghs_123_{}.{}.{}", part("eyJhbGciOiJSUzI1NiJ9", 36), part("eyJpc3MiOiIxMjMi-_Ab", 140), part("c2lnbmF0dXJl-_Xy0", 342))
}

/// The owner's diagnostic (ROUTER_GH_DIAG=mint, E-GHFLOW2.W1): one token request, then only facts
/// that are no secret on stderr, exit 0, and nothing pushed; never the token, the JWT or the key.
#[test]
fn the_token_diagnostic_prints_the_tokens_shape_and_no_secret() {
    let gh = Gh::new();
    let token = stateless_token();
    let diag = || {
        let mut c = common::cmd("sh");
        c.arg(gh.repo.join("scripts/router-gh.sh")).args(["push", "wu/E/x"]).current_dir(&gh.repo);
        gh.github_env(&mut c);
        let o = c.env("ROUTER_GH_DIAG", "mint").output().unwrap();
        let out = text(&o);
        assert_eq!(o.status.code(), Some(0), "{out}");
        assert!(o.stdout.is_empty(), "{out}");
        // no secret: no line of the key, no part of the JWT, and no 8 bytes of the token past its
        // `ghs_` (its prefix and length are the facts it prints)
        gh.assert_no_key_or_jwt(&out);
        let tail = &token.as_bytes()[4..];
        for w in tail.windows(8) {
            let w = std::str::from_utf8(w).unwrap();
            assert!(!out.contains(w), "the token leaked ({w:?}): {out}");
        }
        assert_eq!(gh.calls(), ["POST /app/installations/456/access_tokens"]);
        assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), None);
        let run = gh.home.join(".config/keel/router-gh/run");
        assert_eq!(std::fs::read_dir(&run).unwrap().count(), 0, "the run dir kept a file");
        assert_eq!(std::fs::read_dir(&gh.tmp).unwrap().count(), 0, "the shared $TMPDIR got a file");
        out
    };
    // a token, with GitHub's response headers: their names are printed, never a value
    gh.routes(&[]);
    std::fs::write(gh.mock.join("r0-0.json"), serde_json::json!({"token": token, "expires_at": "2026-11-15T12:00:00Z", "permissions": {}}).to_string())
        .unwrap();
    std::fs::write(
        gh.mock.join("r0-0.json.hdr"),
        "HTTP/2 201\r\ndate: Sat, 15 Nov 2026 10:00:00 GMT\r\ncontent-type: application/json; charset=utf-8\r\nx-github-request-id: ABCD:1234:5678\r\n\r\n",
    )
    .unwrap();
    let out = diag();
    for want in [
        "DIAG status=201\n",
        "DIAG status-line=HTTP/2 201\n",
        "DIAG header-names=date content-type x-github-request-id \n",
        "DIAG keys=token expires_at permissions\n",
        "DIAG token-type=string\n",
        &format!("DIAG token-length={}\n", token.len()),
        "DIAG token-first4=ghs_\n",
        "DIAG token-dots=2 ",
        "DIAG token-shape=ghs_<app_id>_<more>\n",
    ] {
        assert!(out.contains(want), "no {want:?} in {out}");
    }
    for value in ["Sat", "GMT", "application/json", "charset", "ABCD", "1234:5678"] {
        assert!(!out.contains(value), "a header value was printed ({value}): {out}");
    }
    assert!(!out.contains("DIAG message="), "{out}");
    // a refusal: its status and GitHub's message, no token
    std::fs::write(gh.mock.join("routes"), "POST /app/installations/456/access_tokens 401 e.json\n").unwrap();
    std::fs::write(gh.mock.join("e.json"), r#"{"message":"Bad credentials\u0007","documentation_url":"https://docs.github.com"}"#).unwrap();
    std::fs::write(gh.mock.join("log"), "").unwrap();
    let out = diag();
    for want in [
        "DIAG status=401\n",
        "DIAG keys=message documentation_url\n",
        "DIAG token-type=null\n",
        "DIAG token-length=0\n",
        "DIAG token-shape=empty\n",
        "DIAG message=Bad credentials\n",
    ] {
        assert!(out.contains(want), "no {want:?} in {out}");
    }
}

/// git's credential helper: `get` prints the token once, then the file is gone and it prints
/// nothing; other ops print nothing; a file this script did not make is refused.
#[test]
fn the_credential_mode_gives_the_token_once() {
    let gh = Gh::new();
    let rundir = gh.home.join(".config/keel/router-gh/run");
    let dir = rundir.join("router-gh.AbC123");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("token");
    let ask = |f: &Path, op: &str, input: &str| {
        use std::io::Write;
        let mut c = common::cmd("sh");
        c.arg(gh.repo.join("scripts/router-gh.sh")).arg("credential").arg(f).arg(op).env("HOME", &gh.home);
        let mut p = c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        p.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        p.wait_with_output().unwrap()
    };
    let run = |f: &Path, op: &str| ask(f, op, "protocol=https\nhost=github.com\n\n");
    std::fs::write(&file, TOKEN).unwrap();
    // any host but github.com over https gets nothing, and the token stays for GitHub (I1)
    for other in [
        "protocol=https\nhost=evil.example\n\n",
        "protocol=http\nhost=github.com\n\n",
        "protocol=https\nhost=github.com.evil.example\n\n",
        "host=github.com\n\n",
        "",
    ] {
        let o = ask(&file, "get", other);
        assert!(o.status.success() && o.stdout.is_empty() && file.exists(), "{other:?}: {}", text(&o));
    }
    let o = run(&file, "store");
    assert!(o.status.success() && o.stdout.is_empty() && file.exists());
    let o = run(&file, "get");
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout), format!("username=x-access-token\npassword={TOKEN}\n"));
    assert!(!file.exists() && std::fs::read_dir(&dir).unwrap().next().is_none(), "the token file outlived its one read");
    let o = run(&file, "get");
    assert!(o.status.success() && o.stdout.is_empty());
    // any token GitHub mints is handed over as it is: legacy, stateless, the longest taken (E-GHFLOW2.W1)
    let long = format!("ghs_123_{}.{}.{}", "a-".repeat(500), "b_".repeat(1000), "c".repeat(1086));
    for t in [LEGACY_TOKEN, TOKEN, long.as_str()] {
        std::fs::write(&file, t).unwrap();
        let o = run(&file, "get");
        assert_eq!(String::from_utf8_lossy(&o.stdout), format!("username=x-access-token\npassword={t}\n"), "{}", text(&o));
        assert!(!file.exists());
    }
    // a malformed token is never printed: each byte token_ok refuses, alone, and one byte too long
    let over = format!("ghs_{}", "a".repeat(4093));
    for bad in ["not a token; id", "ghs_x\"y", "ghs_x'y", "ghs_x\\y", "ghs_x y", "ghs_x\ty", "ghs_x\ny", "ghs_x\u{7f}y", "ghs_x\u{e9}y", over.as_str()] {
        std::fs::write(&file, bad).unwrap();
        let o = run(&file, "get");
        assert!(o.status.success() && o.stdout.is_empty(), "{bad:?}: {}", text(&o));
        assert!(!file.exists(), "{bad:?}: a refused token file outlived its one read");
    }
    for f in [
        "/etc/passwd".into(),
        gh.root.path().join("token"),
        gh.root.path().join("router-gh.abc/token"),
        gh.root.path().canonicalize().unwrap().join("router-gh.AbC123/token"),
        rundir.join("router-gh.abc/token"),
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

/// Over https (security review I2): a `git` on PATH stands for GitHub's transport. Each time the
/// script sends git to `https://github.com/own/Keel.git`, it first asks git's own credential layer,
/// with the script's exact `-c` options, for another host, for github.com, and for github.com again,
/// then runs the real git against the bare origin. The token goes only to github.com over https,
/// once per git run, and is gone after; push, the epic's ls-remote and sync each get a fresh one.
#[test]
fn over_https_git_gets_the_token_once_and_only_for_github() {
    // any token GitHub mints (E-GHFLOW2.W1): the stateless one, the legacy one, the longest taken
    let long = format!("ghs_123_{}.{}.{}", "a-".repeat(500), "b_".repeat(1000), "c".repeat(1086));
    for t in [TOKEN, LEGACY_TOKEN, long.as_str()] {
        over_https_with(t);
    }
}

fn over_https_with(token: &str) {
    let gh = Gh::new();
    std::fs::write(gh.mock.join("r0-0.json"), serde_json::json!({"token": token, "expires_at": "2026-11-15T12:00:00Z"}).to_string()).unwrap();
    let real = String::from_utf8(common::cmd("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap().trim().to_string();
    let log = gh.root.path().join("cred.log");
    let target = format!("file://{}", gh.origin.display());
    exe(
        &gh.bin.join("git"),
        &format!(
            r#"#!/usr/bin/env python3
import json, os, subprocess, sys
REAL, LOG, TARGET = {real:?}, {log:?}, {target:?}
URL = "https://github.com/own/Keel.git"
a = sys.argv[1:]
if URL in a:
    i = next(i for i, x in enumerate(a) if x in ("push", "fetch", "ls-remote"))
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    def fill(host):
        p = subprocess.run([REAL] + a[:i] + ["credential", "fill"], input="protocol=https\nhost=%s\n\n" % host, capture_output=True, text=True, env=env)
        return p.stdout
    rec = {{"cmd": a[i], "evil": fill("evil.example"), "github": fill("github.com"), "again": fill("github.com"),
        "allow": os.environ.get("GIT_ALLOW_PROTOCOL"), "hooks": os.environ.get("GIT_CONFIG_VALUE_0")}}
    with open(LOG, "a") as f:
        f.write(json.dumps(rec) + "\n")
    a = [TARGET if x == URL else x for x in a]
    # this stand-in is GitHub's https transport: the file:// it reaches is its own business
    os.environ["GIT_ALLOW_PROTOCOL"] = "file"
os.execv(REAL, [REAL] + a)
"#,
            log = log.display().to_string()
        ),
    );
    let https = [("ROUTER_GH_GIT", "https://github.com")];
    let o = gh.run_env(&["push", "wu/E/x"], &https);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(gh.origin_sha("refs/heads/wu/E/x"), Some(gh.sha("wu/E/x")));
    git(&gh.repo, &["switch", "-q", "epic/E"]);
    write(&gh.repo, "README", "docs only");
    commit(&gh.repo, "readme");
    let o = gh.run_env(&["push", "epic/E"], &https);
    assert!(o.status.success(), "{}", text(&o));
    git(&gh.repo, &["reset", "-q", "--hard", "HEAD~1"]);
    git(&gh.repo, &["switch", "-q", "wu/E/x"]);
    let o = gh.run_env(&["sync", "epic/E"], &https);
    assert!(o.status.success() && stdout(&o).contains("state=fast-forward"), "{}", text(&o));
    let runs: Vec<serde_json::Value> = std::fs::read_to_string(&log).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let cmds: Vec<&str> = runs.iter().map(|r| r["cmd"].as_str().unwrap()).collect();
    // each push reads GitHub's refs/notes/mr first (E-GHFLOW2.W3); GitHub holds none here, so no fetch
    assert_eq!(cmds, ["ls-remote", "push", "ls-remote", "ls-remote", "push", "fetch"]);
    for r in &runs {
        assert_eq!(r["evil"], "", "another host got credentials: {r}");
        assert_eq!(r["github"], format!("protocol=https\nhost=github.com\nusername=x-access-token\npassword={token}\n"), "{r}");
        assert_eq!(r["again"], "", "the token was given twice in one git run: {r}");
        // security review P1c: git to GitHub speaks only https and runs no hook, whatever the config
        assert_eq!(r["allow"], "https", "{r}");
        assert_eq!(r["hooks"], "/dev/null", "{r}");
    }
}

/// HOME is normalised (round 3, N1): a trailing / still finds the key and the credential mode still
/// takes its own token file; a HOME holding a character a curl config cannot take is refused, naming
/// the limit, before anything else.
#[test]
fn home_is_normalised_and_its_characters_bounded() {
    let gh = Gh::new();
    let slash = format!("{}/", gh.home.display());
    let o = gh.run_env(&["push", "wu/E/x"], &[("HOME", &slash)]);
    assert!(o.status.success(), "{}", text(&o));
    let dir = gh.home.join(".config/keel/router-gh/run/router-gh.Zz9876");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token"), TOKEN).unwrap();
    let o = {
        use std::io::Write;
        let mut p = common::cmd("sh")
            .arg(gh.repo.join("scripts/router-gh.sh"))
            .arg("credential")
            .arg(dir.join("token"))
            .arg("get")
            .env("HOME", &slash)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        p.stdin.take().unwrap().write_all(b"protocol=https\nhost=github.com\n\n").unwrap();
        p.wait_with_output().unwrap()
    };
    assert_eq!(String::from_utf8_lossy(&o.stdout), format!("username=x-access-token\npassword={TOKEN}\n"));
    std::fs::remove_dir(&dir).unwrap();
    let spaced = gh.root.path().canonicalize().unwrap().join("a home");
    std::fs::create_dir_all(&spaced).unwrap();
    gh.routes(&[]);
    for bad in [spaced.display().to_string(), "relative".to_string(), "/".to_string()] {
        let o = gh.run_env(&["push", "wu/E/x"], &[("HOME", &bad)]);
        assert!(o.status.code() == Some(2) && text(&o).contains("HOME must"), "{bad}: {}", text(&o));
        assert!(gh.log().is_empty(), "{bad}");
    }
}

/// The fixture holds under any umask (code review round 1): under umask 002, Ubuntu's default, a
/// directory is made 0775, and the script rightly refuses a key directory others may write. Here the
/// key directory exists 0775 before the App is installed, as such a umask leaves it.
#[test]
fn the_fixture_holds_under_a_group_writable_umask() {
    let gh = Gh::new();
    let kdir = gh.home.join(".config/keel/router-gh");
    std::fs::remove_dir_all(&kdir).unwrap();
    std::fs::create_dir_all(&kdir).unwrap();
    chmod(&kdir, 0o775);
    install_app(&gh.home);
    let o = gh.run(&["push", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
}

/// The comment files workers write never reach a commit in Keel's own repository.
#[test]
fn keels_gitignore_holds_the_comment_files() {
    let ignore = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l == ".router-gh/"), "{ignore}");
}

/// E-GHFLOW.W3: one of the bot's issues, as GitHub lists it, found again by its marker line.
fn bot_issue(n: u64, branch: &str) -> serde_json::Value {
    serde_json::json!({"number": n, "id": 9000 + n, "state": "open", "html_url": format!("https://github.com/own/Keel/issues/{n}"),
        "user": {"login": BOT, "type": "Bot"}, "body": format!("<!-- keel:router-gh issue v1 {branch} -->\r\nTracking …")})
}

const ISSUE_LIST: &str = "/repos/own/Keel/issues?state=all&labels=keel-tracking&per_page=100&page=1";

/// E-GHFLOW.W3: `issue epic/E` opens the epic's tracking issue, its body built from git (the marker,
/// the tip, a row per WU branch with its notes); `issue wu/E/x` opens the WU's issue, titled by its
/// plan, and links it under the epic's as a sub-issue, by the issue's id.
#[test]
fn issue_opens_the_epics_tracking_issue_and_a_wu_sub_issue() {
    let gh = Gh::new();
    let (epic, tip) = (gh.sha("epic/E"), gh.sha("wu/E/x"));
    gh.routes(&[
        ("GET", ISSUE_LIST, 200, serde_json::json!([])),
        ("POST", "/repos/own/Keel/labels", 201, serde_json::json!({"name": "keel-tracking"})),
        ("POST", "/repos/own/Keel/issues", 201, bot_issue(30, "epic/E")),
    ]);
    let o = gh.run(&["issue", "epic/E"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), "ISSUE number=30 url=https://github.com/own/Keel/issues/30 state=opened open=yes branch=epic/E parent=-");
    // security review I1: only the bot's label is listed; the label is made before an issue carries it
    let list = format!("GET {ISSUE_LIST}");
    assert_eq!(
        gh.calls(),
        [
            "POST /app/installations/456/access_tokens",
            list.as_str(),
            "GET /repos/own/Keel/labels/keel-tracking",
            "POST /repos/own/Keel/labels",
            "POST /repos/own/Keel/issues"
        ]
    );
    assert_auth(&gh);
    assert_eq!(body_of(&gh.log()[3])["name"], "keel-tracking");
    let req = body_of(&gh.log()[4]);
    assert_eq!(req["labels"], serde_json::json!(["keel-tracking"]));
    assert_eq!(req["title"], "epic/E");
    let body = req["body"].as_str().unwrap();
    assert!(body.starts_with("<!-- keel:router-gh issue v1 epic/E -->\nTracking issue of `epic/E` into `main`"), "{body}");
    assert!(body.contains(&format!("**Tip:** `{epic}` · **merged into `main`:** no")), "{body}");
    assert!(body.contains(&format!("| `wu/E/x` | `{}` | no | yes | — | PASS, round 2 | — | — |", &tip[..12])), "{body}");

    gh.routes(&[
        ("GET", ISSUE_LIST, 200, serde_json::json!([bot_issue(30, "epic/E")])),
        ("GET", "/repos/own/Keel/labels/keel-tracking", 200, serde_json::json!({"name": "keel-tracking"})),
        ("POST", "/repos/own/Keel/issues", 201, bot_issue(31, "wu/E/x")),
        ("GET", "/repos/own/Keel/issues/30/sub_issues?per_page=100&page=1", 200, serde_json::json!([])),
        ("POST", "/repos/own/Keel/issues/30/sub_issues", 201, bot_issue(30, "epic/E")),
    ]);
    let o = gh.run(&["issue", "wu/E/x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(stdout(&o), "ISSUE number=31 url=https://github.com/own/Keel/issues/31 state=opened open=yes branch=wu/E/x parent=30");
    let log = gh.log();
    assert_eq!(log.len(), 6, "{:?}", gh.calls());
    let req = body_of(&log[3]);
    assert_eq!(req["title"], "Keel — E.W1 plan: tick owner x(y)  cell b h", "the plan's title, made plain");
    let body = req["body"].as_str().unwrap();
    assert!(body.starts_with("<!-- keel:router-gh issue v1 wu/E/x -->\nWork unit `wu/E/x` into `epic/E`"), "{body}");
    assert!(body.contains(&format!("| {} |", "yes | — | PASS, round 2 | — | —")), "{body}");
    assert!(body.contains("/blob/") && body.contains("docs/work/x-plan.html"), "{body}");
    // the issue's docs table carries the Page column too, linked where the builder publishes
    assert!(body.contains("| Doc | Role | Status | Page |\n|---|---|---|---|\n"), "{body}");
    assert!(body.contains("| wu-plan | ready | [page](https://own.github.io/Keel/wu/E/x/docs/work/x-plan.html) |\n"), "{body}");
    assert!(body.contains("| wu-result | draft | [page](https://own.github.io/Keel/wu/E/x/docs/work/x-result.html) |\n"), "{body}");
    assert_eq!(body_of(&log[5]), serde_json::json!({"sub_issue_id": 9031}));
}

/// E-GHFLOW.W3: a second run refreshes the bot's issue in place (never a stranger's issue or a pull
/// request carrying the same marker), links nothing twice, and closes a branch merged into its base.
#[test]
fn issue_refreshes_in_place_and_closes_a_merged_branch() {
    let gh = Gh::new();
    let mut stranger = bot_issue(40, "wu/E/x");
    stranger["user"] = serde_json::json!({"login": "mallory", "type": "User"});
    let mut pr = bot_issue(41, "wu/E/x");
    pr["pull_request"] = serde_json::json!({});
    let list = serde_json::json!([stranger, pr, bot_issue(31, "wu/E/x"), bot_issue(30, "epic/E")]);
    gh.routes(&[
        ("GET", ISSUE_LIST, 200, list.clone()),
        ("PATCH", "/repos/own/Keel/issues/31", 200, bot_issue(31, "wu/E/x")),
        ("GET", "/repos/own/Keel/issues/30/sub_issues?per_page=100&page=1", 200, serde_json::json!([{"id": 9031, "number": 31}])),
    ]);
    let o = gh.run(&["issue", "wu/E/x"]);
    assert_eq!(stdout(&o), "ISSUE number=31 url=https://github.com/own/Keel/issues/31 state=refreshed open=yes branch=wu/E/x parent=30", "{}", text(&o));
    assert!(!gh.calls().iter().any(|c| c.starts_with("POST /repos")), "{:?}", gh.calls());
    assert_eq!(body_of(&gh.log()[2])["state"], "open");

    git(&gh.repo, &["switch", "-q", "epic/E"]);
    git(&gh.repo, &["merge", "-q", "--no-ff", "--no-verify", "-m", "merge wu/E/x", "wu/E/x"]);
    git(&gh.repo, &["switch", "-q", "wu/E/x"]);
    gh.routes(&[
        ("GET", ISSUE_LIST, 200, list.clone()),
        ("PATCH", "/repos/own/Keel/issues/31", 200, bot_issue(31, "wu/E/x")),
        ("GET", "/repos/own/Keel/issues/30/sub_issues?per_page=100&page=1", 200, serde_json::json!([{"id": 9031, "number": 31}])),
    ]);
    let o = gh.run(&["issue", "wu/E/x"]);
    assert_eq!(stdout(&o), "ISSUE number=31 url=https://github.com/own/Keel/issues/31 state=refreshed open=no branch=wu/E/x parent=30", "{}", text(&o));
    let req = body_of(&gh.log()[2]);
    assert_eq!(req["state"], "closed");
    assert!(req["body"].as_str().unwrap().contains("**merged into `epic/E`:** yes"), "{req}");

    gh.routes(&[("GET", ISSUE_LIST, 200, list), ("PATCH", "/repos/own/Keel/issues/30", 200, bot_issue(30, "epic/E"))]);
    let o = gh.run(&["issue", "epic/E"]);
    assert_eq!(stdout(&o), "ISSUE number=30 url=https://github.com/own/Keel/issues/30 state=refreshed open=yes branch=epic/E parent=-", "{}", text(&o));
    let body = body_of(&gh.log()[2])["body"].as_str().unwrap().to_string();
    assert!(body.contains(&format!("| `wu/E/x` | `{}` | yes |", &gh.sha("wu/E/x")[..12])), "{body}");
}

/// E-GHFLOW.W3: a WU's issue needs its epic's tracking issue first; nothing is created.
#[test]
fn a_wu_issue_needs_its_epics_issue_first() {
    let gh = Gh::new();
    gh.routes(&[("GET", ISSUE_LIST, 200, serde_json::json!([bot_issue(31, "wu/E/y")]))]);
    let o = gh.run(&["issue", "wu/E/x"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("no tracking issue for epic/E yet: run router-gh.sh issue epic/E first"), "{}", text(&o));
    assert!(!gh.calls().iter().any(|c| !c.starts_with("GET") && !c.ends_with("/access_tokens")), "{:?}", gh.calls());
    gh.routes(&[]);
    let o = gh.run(&["issue", "wu/E/nope"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("no local branch wu/E/nope") && gh.calls().is_empty(), "{}", text(&o));
}

/// Security review P1 (backlog R33): the script pins its own checkout's git dir. A linked checkout
/// works as the main one does; its admin dir's commondir pointed elsewhere, or a commondir file in
/// the main git dir (which workers could write before this fix), is refused in every mode before any
/// request, since git would take another repository's config from it.
#[test]
fn a_forged_commondir_is_refused_and_a_linked_checkout_works() {
    let gh = Gh::new();
    let base = gh.root.path().canonicalize().unwrap();
    let linked = base.join("linked");
    git(&gh.repo, &["worktree", "add", "-q", "-b", "side", linked.to_str().unwrap(), "epic/E"]);
    let refresh = || {
        gh.routes(&[
            ("GET", ISSUE_LIST, 200, serde_json::json!([bot_issue(30, "epic/E")])),
            ("PATCH", "/repos/own/Keel/issues/30", 200, bot_issue(30, "epic/E")),
        ])
    };
    refresh();
    let o = gh.run_in(&linked, &["issue", "epic/E"], &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(stdout(&o).starts_with("ISSUE number=30 "), "{}", text(&o));

    let elsewhere = base.join("elsewhere.git");
    git(&base, &["init", "-q", "--bare", elsewhere.to_str().unwrap()]);
    let admin = gh.repo.join(".git/worktrees/linked");
    std::fs::write(admin.join("commondir"), format!("{}\n", elsewhere.display())).unwrap();
    gh.routes(&[]);
    let o = gh.run_in(&linked, &["issue", "epic/E"], &[]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("is not this checkout's admin dir as git makes one"), "{}", text(&o));
    assert!(gh.calls().is_empty(), "{:?}", gh.calls());
    std::fs::write(admin.join("commondir"), "../..\n").unwrap();

    std::fs::write(gh.repo.join(".git/commondir"), format!("{}\n", elsewhere.display())).unwrap();
    for args in [
        &["push", "wu/E/x"][..],
        &["pr-open", "wu/E/x"],
        &["pr-ready", "7"],
        &["post", "7", "review"],
        &["sync", "epic/E"],
        &["issue", "epic/E"],
        &["watch-read", "comment", "30"],
    ] {
        let o = gh.run(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", text(&o));
        assert!(text(&o).contains("commondir exists: a main git dir has none"), "{args:?}: {}", text(&o));
        assert!(gh.calls().is_empty(), "{args:?}: {:?}", gh.calls());
    }
    let o = gh.run_in(&linked, &["issue", "epic/E"], &[]);
    assert!(o.status.code() == Some(2) && text(&o).contains("commondir exists"), "{}", text(&o));
    std::fs::remove_file(gh.repo.join(".git/commondir")).unwrap();

    // round 2: a config.worktree, in the git dir or this checkout's admin dir, is refused too
    for (dir, at) in [(&gh.repo, gh.repo.join(".git/config.worktree")), (&linked, admin.join("config.worktree"))] {
        std::fs::write(&at, "[url \"ext::sh -c touch% /tmp/x\"]\n\tinsteadOf = https://github.com/\n").unwrap();
        gh.routes(&[]);
        let o = gh.run_in(dir, &["issue", "epic/E"], &[]);
        assert_eq!(o.status.code(), Some(2), "{}", text(&o));
        assert!(text(&o).contains("config.worktree exists: this repository uses none"), "{}", text(&o));
        assert!(gh.calls().is_empty(), "{:?}", gh.calls());
        std::fs::remove_file(&at).unwrap();
    }
    refresh();
    assert!(gh.run_in(&linked, &["issue", "epic/E"], &[]).status.success());
}

/// Security review P1c: whatever the repository's config says, the script's git runs no hook and
/// speaks no transport but the one its URL needs. A reference-transaction hook the config names runs
/// for the test's own git, never for sync's; an ext:: transport the config allows and reroutes the
/// URL to runs for the test's own git, never for push.
#[test]
fn the_repositorys_config_reaches_no_hook_and_no_other_transport() {
    let gh = Gh::new();
    let base = gh.root.path().canonicalize().unwrap();
    let (hooks, marks) = (base.join("hooks"), base.join("p1c"));
    std::fs::create_dir_all(&marks).unwrap();
    std::fs::create_dir_all(&hooks).unwrap();
    exe(&hooks.join("reference-transaction"), &format!("#!/bin/sh\ntouch {}\n", common::sh_quote(&marks.join("hook").to_string_lossy())));
    git(&gh.repo, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    gh.github_merges("wu/E/x");
    git(&gh.repo, &["update-ref", "refs/heads/probe", "HEAD"]);
    assert!(marks.join("hook").exists(), "the probe hook must run for a plain git");
    git(&gh.repo, &["update-ref", "-d", "refs/heads/probe"]);
    std::fs::remove_file(marks.join("hook")).unwrap();
    let o = gh.run(&["sync", "epic/E"]);
    assert!(o.status.success() && stdout(&o).contains("state=fast-forward"), "{}", text(&o));
    assert!(!marks.join("hook").exists(), "sync ran the repository's hook");

    let url = format!("file://{}/gh/own/Keel.git", base.display());
    let ext = format!("ext::sh -c touch% {}", marks.join("ext").display());
    git(&gh.repo, &["config", "protocol.ext.allow", "always"]);
    git(&gh.repo, &["config", &format!("url.{ext}.insteadOf"), &url]);
    let _ = common::cmd("git").args(["ls-remote", &url]).current_dir(&gh.repo).output().unwrap();
    assert!(marks.join("ext").exists(), "the probe transport must run for a plain git");
    std::fs::remove_file(marks.join("ext")).unwrap();
    let o = gh.run(&["push", "wu/E/x"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(!marks.join("ext").exists(), "push ran the repository's ext:: transport");
}
