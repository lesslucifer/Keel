//! `keel mr setup|commit|confirm`, the `.mr-allowed` trust store, gate check 8 and the
//! commit-msg `Reviewed-By` check (UC-MR-1/2). Ported from Trawler's xtask/src/mr.rs
//! (DEC-9/10/27): stock `ssh-keygen -Y` signatures, verified offline against the roster.
use crate::notes::{self, MrNote};
use crate::{gate, git, git_dir, verify_doc, Config, ReportPolicy};
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
/// level, the range (base, tip) and the attested report, if any - with none, the report lines are
/// absent, so the signature never covers a placeholder. Never read back from a note - always rebuilt.
fn payload(level: &str, base: &str, tip: &str, change: &str, report: Option<(&str, &str)>) -> String {
    let mut p = format!("keel-mr/v2\nlevel: {level}\nbase: {base}\ntip: {tip}\nchange: {change}\n");
    if let Some((path, hash)) = report {
        p.push_str(&format!("report: {path}\nreport_hash: {hash}\n"));
    }
    p
}

fn role(epic: bool) -> &'static str {
    if epic {
        "epic-result"
    } else {
        "wu-result"
    }
}

fn has_role(html: &str, role: &str) -> bool {
    verify_doc::artifact(html).is_some_and(|a| a.split_whitespace().any(|r| r == role))
}

/// The docs whose `data-artifact` carries the level's result role: html under docs.root changed in
/// base..tip and present at the tip, plus (epic: archived out of git by then) the untracked ones,
/// ignored included - `docs/.archive/` is gitignored.
fn report_candidates(repo: &Path, cfg: &Config, base: &str, tip: &str, epic: bool) -> Result<Vec<String>, String> {
    let html = |n: &String| n.starts_with(&cfg.docs.root) && n.ends_with(".html");
    let mut found = vec![];
    for n in notes::names(repo, base, tip, &[])?.iter().filter(|n| html(n)) {
        if let Ok(body) = git(repo, &["show", &format!("{tip}:{n}")], &[]) {
            if has_role(&body, role(epic)) {
                found.push(n.clone());
            }
        }
    }
    if epic {
        let root = cfg.docs.root.trim_end_matches('/');
        for n in notes::zpaths(&git(repo, &["ls-files", "-z", "--others", "--", root], &[])?).iter().filter(|n| html(n)) {
            if std::fs::read_to_string(repo.join(n)).is_ok_and(|b| has_role(&b, role(epic))) && !found.contains(n) {
                found.push(n.clone());
            }
        }
    }
    Ok(found)
}

/// The report this sign-off attests under `[mr] report`, or `Err((what, fix))` refusing the confirm.
fn pick_report(repo: &Path, cfg: &Config, args: &[&str], base: &str, tip: &str, epic: bool) -> Result<Option<String>, (String, String)> {
    let given = flag(args, "--report");
    if cfg.mr.report == ReportPolicy::None {
        return match given {
            Some(r) => Err((
                format!("--report {r} given, but [mr] report = \"none\": this project's sign-offs attest no report"),
                "drop --report, or set [mr] report = \"optional\" or \"required\" in .keel/config.toml on the target".into(),
            )),
            None => Ok(None),
        };
    }
    if let Some(r) = given {
        return Ok(Some(r.to_string()));
    }
    let role = role(epic);
    let found = report_candidates(repo, cfg, base, tip, epic).map_err(|e| (e, "fix the repo state, then re-run".into()))?;
    match &found[..] {
        [one] => Ok(Some(one.clone())),
        [] if cfg.mr.report == ReportPolicy::Optional => Ok(None),
        [] => Err((
            format!("[mr] report = \"required\", and no {role} doc (data-artifact=\"{role}\") is under {} for this {}", cfg.docs.root, if epic { "epic" } else { "WU" }),
            format!("write the {role} report (skill keel-plan), commit it on the tip, then keel mr confirm again - or name it with --report <path>"),
        )),
        several => Err((
            format!("{} {role} docs could be the report: {}", several.len(), several.join(", ")),
            "name the one you read: keel mr confirm ... --report <path>".into(),
        )),
    }
}

/// `sha256:<hex>`: the algorithm is named in the data.
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// SHA-256 of the full `base..tip` diff text (the same bytes the cr and dod bindings hash).
fn change_hash(repo: &Path, base: &str, tip: &str) -> Result<String, String> {
    Ok(sha256(notes::diff_text(repo, base, tip, &[])?.as_bytes()))
}

/// SHA-256 of the report's exact bytes: the blob at the tip, else (epic only - archived out of
/// git by then) the raw local file. `Ok(None)` = an epic report found nowhere.
fn report_hash(repo: &Path, tip: &str, path: &str, epic: bool) -> Result<Option<String>, String> {
    let ls = git(repo, &["ls-tree", tip, "--", path], &[])?;
    if let ["100644" | "100755", "blob", id] = ls.split('\t').next().unwrap_or("").split(' ').collect::<Vec<_>>()[..] {
        let o = Command::new("git").args(["cat-file", "blob", id]).current_dir(repo).output().map_err(|e| format!("keel: cannot run git: {e}"))?;
        if !o.status.success() {
            return Err(format!("keel: git cat-file blob {id} failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        return Ok(Some(sha256(&o.stdout)));
    }
    if !epic {
        return Err(format!("{path} (the attested result report) is not tracked at the tip {}", short(tip)));
    }
    Ok(std::fs::read(repo.join(path)).ok().map(|b| sha256(&b)))
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

/// `rule_bearing` entry: a trailing `/` is a prefix; otherwise a glob where
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
    let report = match pick_report(repo, cfg, args, &base, &tip, epic) {
        Ok(r) => r,
        Err((what, fix)) => {
            eprintln!("mr confirm refused - {what}\n  fix -> {fix}");
            return Ok(1);
        }
    };
    if report.as_ref().is_some_and(|r| r.contains('\n')) {
        return Err("keel mr: a report path cannot contain a newline".into());
    }
    let report_hash = match &report {
        Some(r) => Some(report_hash(repo, &tip, r, epic)?.ok_or_else(|| format!("keel mr: cannot read {r} - name the report you read"))?),
        None => None,
    };
    let attested = report.as_deref().zip(report_hash.as_deref());
    let what = attested.map_or("no report".to_string(), |(r, _)| r.to_string());
    let level = if epic { "epic" } else { "wu" };
    let change = change_hash(repo, &base, &tip)?;
    let sig = if tier == "key" {
        let key = git(repo, &["config", "--get", "keel.signingkey"], &[])
            .map_err(|_| format!("keel mr: {id} is key-backed but no signing key is configured\n  fix -> keel mr setup --as {id} --key <path>"))?;
        let data = payload(level, &base, &tip, &change, attested);
        eprintln!("signing the {level} {tip_ref} ({}) and {what} as {id} ...", short(&tip));
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
        report,
        report_hash,
        change_hash: change,
        sig,
    };
    notes::store(repo, "mr", &tip, &note)?;
    println!("mr/v1 on {} - {tier} {id} attests {what}", short(&tip));
    Ok(0)
}

/// Check 8 for a wu/epic tip: `Err((what, fix))` on the first cause found, else the attested
/// `(report, report_hash)`, if any. Everything is rebuilt from the repo; the note contributes only
/// the signer, the report path and the (signed) report hash.
pub(crate) fn manual_review(repo: &Path, cfg: &Config, level: &str, tip: &str, base: &str) -> Result<Option<(String, String)>, (String, String)> {
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
        return again(stale_change(&n.change_hash, &format!("{}..{t}", short(base))));
    }
    let attested = match (&n.report, &n.report_hash) {
        (Some(r), Some(h)) => Some((r.as_str(), h.as_str())),
        (None, None) => None,
        _ => return again(format!("the mr/v1 note on {t} names a report or a report_hash without the other")),
    };
    if attested.is_none() && cfg.mr.report == ReportPolicy::Required {
        let role = role(level == "epic");
        return Err((
            format!("the {level} sign-off on {t} attests no result report, but [mr] report = \"required\" (the target's committed policy)"),
            format!("write the {role} report on the tip, then re-run keel mr confirm\n      {NOT_AUTHORISED}"),
        ));
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
            let data = payload(level, base, tip, &change, attested);
            if !verify(repo, &n.signer, &keys_of(&allowed, &n.signer), &data, sig) {
                return again(format!("{t}'s mr/v1 signature by {} does not verify over base..tip and the attested report against {roster}", n.signer));
            }
        }
        Some(_) => {} // identity tier: listed is all it proves
    }
    // A report in the note is re-hashed whatever the policy: an attestation is never ignored.
    let Some((path, hash)) = attested else { return Ok(None) };
    match report_hash(repo, tip, path, level == "epic") {
        Ok(Some(h)) if h != hash => again(format!("{path} differs from the bytes the mr/v1 note attests")),
        Err(e) => again(e),
        _ => Ok(Some((path.to_string(), hash.to_string()))),
    }
}

fn stale_change(signed: &str, range: &str) -> String {
    if signed.starts_with("sha256:") {
        format!("the diff {range} changed since the sign-off")
    } else {
        format!("the sign-off on {range} was signed by a keel older than the SHA-256 sign-off format; re-sign with keel mr confirm")
    }
}

/// commit-msg hook: a staged change to a rule-bearing path needs a `Reviewed-By` trailer naming a listed signer.
pub fn commit_msg(repo: &Path, cfg: &Config, msg_file: &str) -> Result<i32, String> {
    // A merge authors nothing new; its commits were authorised on their branch.
    if git(repo, &["rev-parse", "--verify", "-q", "MERGE_HEAD"], &[]).is_ok() {
        return Ok(0);
    }
    let staged = notes::zpaths(&git(repo, &["diff", "--cached", "--name-only", "--no-renames", "-z"], &[])?);
    let hits: Vec<&str> = staged.iter().map(String::as_str).filter(|p| rule_bearing(cfg, p)).collect();
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

#[cfg(test)]
mod tests {
    use super::{has_role, payload, sha256, stale_change};

    #[test]
    fn a_pre_sha256_change_hash_names_the_old_format_not_a_changed_diff() {
        assert_eq!(stale_change("sha256:00", "b..t"), "the diff b..t changed since the sign-off");
        let old = stale_change("9daeafb9864cf43055ae93beb0afd6c7d144bfa4", "b..t");
        assert!(old.contains("older than the SHA-256 sign-off format") && old.contains("keel mr confirm"), "{old}");
        assert!(!old.contains("changed since"));
    }

    #[test]
    fn sha256_matches_the_fips_180_2_abc_vector() {
        assert_eq!(sha256(b"abc"), "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn payload_v2_carries_the_report_lines_exactly_when_a_report_is_attested() {
        let with = payload("wu", "b", "t", "c", Some(("docs/r.html", "h")));
        assert_eq!(with, "keel-mr/v2\nlevel: wu\nbase: b\ntip: t\nchange: c\nreport: docs/r.html\nreport_hash: h\n");
        let without = payload("epic", "b", "t", "c", None);
        assert_eq!(without, "keel-mr/v2\nlevel: epic\nbase: b\ntip: t\nchange: c\n");
        assert!(!without.contains("report"));
    }

    #[test]
    fn a_collapsed_result_doc_has_both_roles_and_an_ungoverned_one_none() {
        let collapsed = r#"<html data-artifact="epic-result wu-result">"#;
        assert!(has_role(collapsed, "wu-result") && has_role(collapsed, "epic-result"));
        assert!(has_role(r#"<html data-artifact="wu-result">"#, "wu-result"));
        assert!(!has_role(r#"<html data-artifact="wu-result">"#, "epic-result"));
        assert!(!has_role("<p>x-result</p>", "wu-result"));
    }
}
