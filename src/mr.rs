//! `keel mr setup|commit|confirm`, the `.mr-allowed` trust store, gate check 8 and the
//! commit-msg `Reviewed-By` check (UC-MR-1/2). Ported from Trawler's xtask/src/mr.rs
//! (DEC-9/10/27): stock `ssh-keygen -Y` signatures, verified offline against the roster.
use crate::notes::{self, MrNote};
use crate::{gate, git, git_dir, Config};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `ssh-keygen -Y -n` namespace: a Keel sign-off is never valid as a git commit signature or anything else.
const NAMESPACE: &str = "keel-mr";
const TRAILER: &str = "Reviewed-By";
const ALLOWED: &str = ".mr-allowed";
pub(crate) const KNOWN_TIERS: &[&str] = &["email", "username", "key"];
const NOT_AUTHORISED: &str = "A HUMAN must review this and confirm - an AI agent MUST NOT self-authorise.
      after the human has read the change and said yes:
        keel mr commit -m \"<msg>\" --as <identity>          (rule-bearing commit)
        keel mr confirm <wu|epic> [<tip>] --as <identity>   (WU / epic tip)
      the identity must be listed in .mr-allowed (keel mr setup)";

struct Entry {
    tier: String,
    identity: String,
    /// `<keytype> <base64>` of a `key` entry.
    key: Option<String>,
}

/// Restricted so an identity can never carry allowed_signers pattern syntax (`*?,!"`) or whitespace.
fn valid_identity(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// The local trust store. `<tier>: <identity>` or `key: <identity> <keytype> <base64> [comment]`.
/// Refused while tracked (a branch could rewrite it); lines whose tier the project does not accept
/// (`mr.tiers`) authorise nothing; a malformed line is an error, never skipped.
fn read_allowed(repo: &Path, cfg: &Config) -> Result<Vec<Entry>, String> {
    if let Some(t) = cfg.mr.tiers.iter().find(|t| !KNOWN_TIERS.contains(&t.as_str())) {
        return Err(format!("keel: unknown mr tier '{t}' in .keel/config.toml (known: {})", KNOWN_TIERS.join(", ")));
    }
    if git(repo, &["ls-files", "--error-unmatch", "--", ALLOWED], &[]).is_ok() {
        return Err(format!(
            "{ALLOWED} is tracked by git - a trust store a branch can rewrite trusts whoever wrote the branch, and checking out that branch has already overwritten your local roster with its copy\n  fix -> do not trust the file on disk: git rm --cached {ALLOWED} && rm {ALLOWED}, commit the removal, then rebuild your own roster from scratch with keel mr setup --as <identity> [--key <path>]"
        ));
    }
    let text = match std::fs::read_to_string(repo.join(ALLOWED)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("keel: cannot read {ALLOWED}: {e}")),
    };
    let mut out = vec![];
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || format!("keel: {ALLOWED} line {}: expected `<tier>: <identity>` or `key: <identity> <ssh-pubkey>`", n + 1);
        let (tier, rest) = line.split_once(':').ok_or_else(bad)?;
        let tier = tier.trim();
        let mut f = rest.split_whitespace();
        let identity = f.next().filter(|i| valid_identity(i)).ok_or_else(bad)?;
        let key = match (tier, f.next(), f.next()) {
            // Only a real key type: an options field (cert-authority, ...) never reaches allowed_signers.
            ("key", Some(t), Some(b)) if ["ssh-", "ecdsa-", "sk-"].iter().any(|p| t.starts_with(p)) => Some(format!("{t} {b}")),
            ("key", ..) => return Err(bad()),
            (_, None, _) => None,
            _ => return Err(bad()),
        };
        if cfg.mr.tiers.iter().any(|t| t == tier) {
            out.push(Entry { tier: tier.into(), identity: identity.into(), key });
        }
    }
    Ok(out)
}

/// The tier that authorises `id`; `key` wins, so a key holder can never sign off unsigned.
///
/// Accepted limitation (owner decision, E-SEED W4 review H1): "signed" means "a key/identity listed
/// in this checkout's roster signed", not "a human reviewed". An agent with filesystem access can add
/// itself to `.mr-allowed` and sign with an agent-usable key; nothing here can tell. E-SEED runs
/// ungated until W7 (epic Q12), so this is revisited before Keel gates its own real work or ships.
fn tier_of<'a>(allowed: &'a [Entry], id: &str) -> Option<&'a str> {
    let mine = || allowed.iter().filter(|e| e.identity == id);
    mine().find(|e| e.key.is_some()).or_else(|| mine().next()).map(|e| e.tier.as_str())
}

fn keys_of<'a>(allowed: &'a [Entry], id: &str) -> Vec<&'a str> {
    allowed.iter().filter(|e| e.identity == id).filter_map(|e| e.key.as_deref()).collect()
}

/// The bytes a key-backed sign-off signs: versioned, labelled, one field per line, and bound to the
/// level, the range (base, tip) and the attested report. Never read back from a note - always rebuilt.
fn payload(level: &str, base: &str, tip: &str, change: &str, report: &str, report_hash: &str) -> String {
    format!("keel-mr/v1\nlevel: {level}\nbase: {base}\ntip: {tip}\nchange: {change}\nreport: {report}\nreport_hash: {report_hash}\n")
}

/// git blob hash of the full `base..tip` diff.
fn change_hash(repo: &Path, base: &str, tip: &str) -> Result<String, String> {
    notes::hash_blob(repo, &git(repo, &["diff", "--no-color", "--no-ext-diff", base, tip], &[])?)
}

/// git blob id of the report's exact bytes: the blob at the tip, else (epic only - archived out of
/// git by then) the unfiltered local file. `Ok(None)` = an epic report found nowhere.
fn report_hash(repo: &Path, tip: &str, path: &str, epic: bool) -> Result<Option<String>, String> {
    let ls = git(repo, &["ls-tree", tip, "--", path], &[])?;
    if let ["100644" | "100755", "blob", id] = ls.split('\t').next().unwrap_or("").split(' ').collect::<Vec<_>>()[..] {
        return Ok(Some(id.to_string()));
    }
    if !epic {
        return Err(format!("{path} (the attested result report) is not tracked at the tip {}", short(tip)));
    }
    if !repo.join(path).is_file() {
        return Ok(None);
    }
    git(repo, &["hash-object", "--no-filters", "--", path], &[]).map(Some)
}

fn ssh_keygen(args: &[&str], input: &[u8]) -> Result<std::process::Output, String> {
    let mut c = Command::new("ssh-keygen")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("keel: cannot run ssh-keygen: {e}"))?;
    c.stdin.take().unwrap().write_all(input).map_err(|e| e.to_string())?;
    c.wait_with_output().map_err(|e| e.to_string())
}

/// Armored SSH SIGNATURE over `data`. A `.pub` key path signs through ssh-agent.
fn sign(key: &str, data: &str) -> Result<String, String> {
    let o = ssh_keygen(&["-Y", "sign", "-n", NAMESPACE, "-f", key], data.as_bytes())?;
    if !o.status.success() {
        return Err(format!("keel: ssh-keygen -Y sign with {key} failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Does `sig` verify over `data` as `id`, against `id`'s listed keys only? Offline: no agent, no network.
fn verify(repo: &Path, id: &str, keys: &[&str], data: &str, sig: &str) -> bool {
    if keys.is_empty() || !valid_identity(id) {
        return false;
    }
    let Ok(gd) = git_dir(repo) else { return false };
    let dir = gd.join(format!("keel-mr-verify-{}", std::process::id()));
    let (signers, sig_file) = (dir.join("allowed_signers"), dir.join("sig"));
    let lines: String = keys.iter().map(|k| format!("{id} {k}\n")).collect();
    let ok = std::fs::create_dir_all(&dir).is_ok()
        && std::fs::write(&signers, lines).is_ok()
        && std::fs::write(&sig_file, sig).is_ok()
        && ssh_keygen(
            &["-Y", "verify", "-f", &signers.to_string_lossy(), "-I", id, "-n", NAMESPACE, "-s", &sig_file.to_string_lossy()],
            data.as_bytes(),
        )
        .is_ok_and(|o| o.status.success());
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

/// `rule_bearing` entry: a trailing `/` is a prefix (like code_paths); otherwise a glob where
/// `*`/`?` stay inside one path segment and `**` crosses segments.
fn rule_bearing(cfg: &Config, path: &str) -> bool {
    cfg.rule_bearing.iter().any(|p| {
        if p.ends_with('/') {
            return path.starts_with(p.as_str());
        }
        let re = regex::escape(p).replace(r"\*\*", ".*").replace(r"\*", "[^/]*").replace(r"\?", "[^/]");
        regex::Regex::new(&format!("^{re}$")).is_ok_and(|r| r.is_match(path))
    })
}

fn flag<'a>(args: &[&'a str], k: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == k).map(|w| w[1])
}

/// `--as`, else the roster's only identity.
fn signer(allowed: &[Entry], args: &[&str]) -> Result<String, String> {
    if let Some(id) = flag(args, "--as") {
        return Ok(id.into());
    }
    let mut ids: Vec<&str> = allowed.iter().map(|e| e.identity.as_str()).collect();
    ids.sort();
    ids.dedup();
    match ids[..] {
        [one] => Ok(one.into()),
        [] => Err(format!("keel mr: refused - {ALLOWED} lists nobody\n  fix -> keel mr setup --as <identity> [--key <path>]")),
        _ => Err("keel mr: several identities are listed - pass --as <identity>".into()),
    }
}

fn unlisted(repo: &Path, id: &str) -> String {
    format!("keel mr: refused - {id} is not listed in {}\n  fix -> {NOT_AUTHORISED}", repo.join(ALLOWED).display())
}

fn setup(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    let id = flag(args, "--as").ok_or("usage: keel mr setup --as <identity> [--key <path>]")?;
    if !valid_identity(id) {
        return Err(format!("keel mr: identity '{id}' may only use letters, digits and @._+-"));
    }
    let allowed = read_allowed(repo, cfg)?;
    if git(repo, &["check-ignore", "-q", ALLOWED], &[]).is_err() {
        let ex = repo.join(git(repo, &["rev-parse", "--git-path", "info/exclude"], &[])?);
        std::fs::create_dir_all(ex.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut body = std::fs::read_to_string(&ex).unwrap_or_default();
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        std::fs::write(&ex, format!("{body}/{ALLOWED}\n")).map_err(|e| format!("keel: write {}: {e}", ex.display()))?;
    }
    let key = flag(args, "--key");
    let line = match key {
        Some(k) => {
            let pubp = if k.ends_with(".pub") { k.to_string() } else { format!("{k}.pub") };
            let pk = std::fs::read_to_string(&pubp).map_err(|e| format!("keel mr: cannot read public key {pubp}: {e}"))?;
            match pk.split_whitespace().collect::<Vec<_>>()[..] {
                [t, b, ..] if ["ssh-", "ecdsa-", "sk-"].iter().any(|p| t.starts_with(p)) => format!("key: {id} {t} {b}"),
                _ => return Err(format!("keel mr: {pubp} is not an OpenSSH public key")),
            }
        }
        None => match tier_of(&allowed, id) {
            Some(t) => {
                println!("{id} is already listed ({t})");
                return Ok(0);
            }
            None => format!("{}: {id}", if id.contains('@') { "email" } else { "username" }),
        },
    };
    let tier = line.split(':').next().unwrap();
    if !cfg.mr.tiers.iter().any(|t| t == tier) {
        return Err(format!("keel mr: refused - tier '{tier}' is not accepted here ([mr] tiers = {:?})", cfg.mr.tiers));
    }
    let path = repo.join(ALLOWED);
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();
    if !body.lines().any(|l| l.trim() == line) {
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        std::fs::write(&path, format!("{body}{line}\n")).map_err(|e| format!("keel: write {ALLOWED}: {e}"))?;
    }
    let allowed = read_allowed(repo, cfg)?; // also validates the line just written
    if let Some(k) = key {
        git(repo, &["config", "--local", "keel.signingkey", k], &[])?;
        // Prove the key signs here (private half present, or loaded in ssh-agent) and matches the roster.
        let probe = format!("keel-mr setup probe for {id}\n");
        let sig = sign(k, &probe)?;
        if !verify(repo, id, &keys_of(&allowed, id), &probe, &sig) {
            return Err(format!("keel mr: the key {k} signed, but its signature does not verify against {ALLOWED}"));
        }
        println!("registered {id} (key) in {ALLOWED}; keel.signingkey = {k}; a probe signature verified");
        println!("note: a key the agent's ssh-agent can use is a key the AI agent can use - prefer an sk- (FIDO2) key or ssh-add -c");
    } else {
        println!("registered {id} ({tier}) in {ALLOWED} - an identity tier is a string match, not a signature");
    }
    Ok(0)
}

fn commit(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    let msg = flag(args, "-m").filter(|m| !m.trim().is_empty()).ok_or("usage: keel mr commit -m <msg> [--as <identity>]")?;
    let allowed = read_allowed(repo, cfg)?;
    let id = signer(&allowed, args)?;
    let Some(tier) = tier_of(&allowed, &id) else {
        eprintln!("{}", unlisted(repo, &id));
        return Ok(1);
    };
    let body = format!("{}\n\n{TRAILER}: {tier} {id}\n", msg.trim_end());
    let o = Command::new("git").args(["commit", "-m", &body]).current_dir(repo).status().map_err(|e| e.to_string())?;
    Ok(if o.success() { 0 } else { 1 })
}

fn confirm(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    const USAGE: &str = "usage: keel mr confirm <wu|epic> [<tip>] [--as <identity>] [--target R] [--report <path>]";
    let mut pos = vec![];
    let mut i = 0;
    while i < args.len() {
        if args[i].starts_with('-') {
            i += 2;
        } else {
            pos.push(args[i]);
            i += 1;
        }
    }
    let (level, tip_ref) = match pos[..] {
        [l] => (l, "HEAD"),
        [l, t] => (l, t),
        _ => return Err(USAGE.into()),
    };
    let epic = match level {
        "wu" => false,
        "epic" => true,
        _ => return Err(USAGE.into()),
    };
    // The branch name decides a WU's default target, so name HEAD by its branch.
    let tip_ref = match tip_ref {
        "HEAD" => git(repo, &["symbolic-ref", "--short", "-q", "HEAD"], &[]).unwrap_or_else(|_| "HEAD".into()),
        t => t.to_string(),
    };
    let allowed = read_allowed(repo, cfg)?;
    let id = signer(&allowed, args)?;
    let Some(tier) = tier_of(&allowed, &id).map(String::from) else {
        eprintln!("{}", unlisted(repo, &id));
        return Ok(1);
    };
    // Sign last (Trawler DEC-27): every binding pins the diff, so the tree settles before a human signs.
    let Some((tip, base)) = gate::sign_ready(repo, cfg, epic, &tip_ref, args)? else {
        eprintln!("mr confirm refused - sign last: get the notes and a bound cr PASS green on this exact tip first\n  fix -> keel notes write test|doc, run the code review (cr PASS), then keel mr confirm again");
        return Ok(1);
    };
    let report = match flag(args, "--report") {
        Some(r) => r.to_string(),
        None => {
            let found: Vec<String> = notes::names(repo, &base, &tip)?
                .into_iter()
                .filter(|n| n.starts_with(&cfg.docs.root) && n.ends_with("-result.html"))
                .collect();
            match &found[..] {
                [one] => one.clone(),
                _ => return Err(format!("keel mr: {} result reports changed in the range - pass --report <the report you read>", found.len())),
            }
        }
    };
    if report.contains('\n') {
        return Err("keel mr: a report path cannot contain a newline".into());
    }
    let report_hash = report_hash(repo, &tip, &report, epic)?
        .ok_or_else(|| format!("keel mr: cannot read {report} - name the report you read"))?;
    let level = if epic { "epic" } else { "wu" };
    let change = change_hash(repo, &base, &tip)?;
    let sig = if tier == "key" {
        let key = git(repo, &["config", "--get", "keel.signingkey"], &[])
            .map_err(|_| format!("keel mr: {id} is key-backed but no signing key is configured\n  fix -> keel mr setup --as {id} --key <path>"))?;
        let data = payload(level, &base, &tip, &change, &report, &report_hash);
        eprintln!("signing the {level} {tip_ref} ({}) and {report} as {id} ...", short(&tip));
        let sig = sign(&key, &data)?;
        if !verify(repo, &id, &keys_of(&allowed, &id), &data, &sig) {
            return Err(format!("keel mr: refused - keel.signingkey ({key}) is not a key {ALLOWED} lists for {id}"));
        }
        Some(sig)
    } else {
        None
    };
    let branch = (tip_ref != tip && tip_ref != "HEAD").then(|| tip_ref.clone());
    let note = MrNote {
        schema: "mr/v1".into(),
        id: notes::identity(repo, tip.clone(), base, branch)?,
        level: level.into(),
        signer: id.clone(),
        tier: tier.clone(),
        report: report.clone(),
        report_hash,
        change_hash: change,
        sig,
    };
    notes::store(repo, "mr", &tip, &note)?;
    println!("mr/v1 on {} - {tier} {id} attests {report}", short(&tip));
    Ok(0)
}

/// Check 8 for a wu/epic tip: `Err((what, fix))` on the first cause found. Everything is rebuilt
/// from the repo; the note contributes only the signer, the report path and the (signed) report hash.
pub(crate) fn manual_review(repo: &Path, cfg: &Config, level: &str, tip: &str, base: &str) -> Result<(), (String, String)> {
    let again = |what: String| Err((what, format!("re-run keel mr confirm on the tip as it now stands\n      {NOT_AUTHORISED}")));
    let t = short(tip);
    let Ok(raw) = git(repo, &["notes", "--ref=mr", "show", tip], &[]) else {
        return Err((format!("every {level} merge needs a human sign-off - tip {t} has no mr/v1 note"), NOT_AUTHORISED.into()));
    };
    let n = match serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| e.to_string()).and_then(|v| {
        match v["schema"].as_str() {
            Some("mr/v1") => serde_json::from_value::<MrNote>(v).map_err(|e| e.to_string()),
            other => Err(format!("declares schema {}", other.unwrap_or("none"))),
        }
    }) {
        Ok(n) => n,
        Err(e) => return again(format!("the mr note on {t} is not valid mr/v1: {e}")),
    };
    if n.id.tip != tip {
        return again(format!("the mr/v1 note on {t} signs {} (copied onto an amended or other commit?)", short(&n.id.tip)));
    }
    if n.level != level {
        return again(format!("the mr/v1 note on {t} signs level {}, this is a {level} merge", n.level));
    }
    if n.id.base != base {
        return again(format!("signed against base {}, but the merge-base is now {}", short(&n.id.base), short(base)));
    }
    let change = change_hash(repo, base, tip).map_err(|e| (e, "fix the repo state, then re-run".into()))?;
    if n.change_hash != change {
        return again(format!("the diff {}..{t} changed since the sign-off", short(base)));
    }
    let allowed = read_allowed(repo, cfg).map_err(|e| (e, NOT_AUTHORISED.into()))?;
    let roster = repo.join(ALLOWED).display().to_string();
    match tier_of(&allowed, &n.signer) {
        None => {
            return Err((
                format!("{t} is signed off by {}, whom {roster} does not list (under tiers {:?})", n.signer, cfg.mr.tiers),
                format!("the roster is per-checkout by design - add {}'s line there (keel mr setup), or merge from the checkout that signed\n      {NOT_AUTHORISED}", n.signer),
            ))
        }
        Some("key") => {
            let Some(sig) = &n.sig else {
                return again(format!("{} is key-backed in {roster}, but the note on {t} carries no signature", n.signer));
            };
            let data = payload(level, base, tip, &change, &n.report, &n.report_hash);
            if !verify(repo, &n.signer, &keys_of(&allowed, &n.signer), &data, sig) {
                return again(format!("{t}'s mr/v1 signature by {} does not verify over base..tip and the attested report against {roster}", n.signer));
            }
        }
        Some(_) => {} // identity tier: listed is all it proves
    }
    match report_hash(repo, tip, &n.report, level == "epic") {
        Ok(Some(h)) if h != n.report_hash => again(format!("{} differs from the bytes the mr/v1 note attests", n.report)),
        Err(e) => again(e),
        _ => Ok(()),
    }
}

/// commit-msg hook: a staged change to a rule-bearing path needs a `Reviewed-By` trailer naming a listed signer.
pub fn commit_msg(repo: &Path, cfg: &Config, msg_file: &str) -> Result<i32, String> {
    // A merge authors nothing new; its commits were authorised on their branch.
    if git(repo, &["rev-parse", "--verify", "-q", "MERGE_HEAD"], &[]).is_ok() {
        return Ok(0);
    }
    let staged = git(repo, &["diff", "--cached", "--name-only", "--no-renames"], &[])?;
    let hits: Vec<&str> = staged.lines().filter(|p| rule_bearing(cfg, p)).collect();
    if hits.is_empty() {
        return Ok(0);
    }
    let msg = std::fs::read_to_string(msg_file).map_err(|e| format!("keel: cannot read {msg_file}: {e}"))?;
    let allowed = match read_allowed(repo, cfg) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("keel: commit refused - {e}");
            return Ok(1);
        }
    };
    let id = msg
        .lines()
        .filter_map(|l| l.strip_prefix(TRAILER)?.trim_start().strip_prefix(':'))
        .next_back()
        .and_then(|v| v.split_whitespace().next_back());
    if id.is_some_and(|id| tier_of(&allowed, id).is_some()) {
        return Ok(0);
    }
    eprintln!(
        "keel: commit refused - it touches rule-bearing {} with no {TRAILER} trailer from a listed signer\n  fix -> {NOT_AUTHORISED}",
        hits.join(", ")
    );
    Ok(1)
}

pub fn run(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    match args {
        ["setup", rest @ ..] => setup(repo, cfg, rest),
        ["commit", rest @ ..] => commit(repo, cfg, rest),
        ["confirm", rest @ ..] => confirm(repo, cfg, rest),
        _ => Err("usage: keel mr <setup --as <id> [--key <path>] | commit -m <msg> [--as <id>] | confirm <wu|epic> [<tip>] [--as <id>] [--target R] [--report <path>]>".into()),
    }
}
