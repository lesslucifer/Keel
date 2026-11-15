//! `keel mr setup|commit|confirm`, the committed signer roster `.keel/mr-allowed`, gate check 8 and
//! the commit-msg `Reviewed-By` check (UC-MR-1/2, DEC-47). Ported from Trawler's xtask/src/mr.rs
//! (DEC-9/10/27): stock `ssh-keygen -Y` signatures, verified offline against the roster.
use crate::notes::{self, sha256, CrNote, MrNote, Review};
use crate::{gate, git, git_dir, no_git_env, review, short, verify_doc, Config, ReportPolicy};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `ssh-keygen -Y -n` namespace: a Keel sign-off is never valid as a git commit signature or anything else.
const NAMESPACE: &str = "keel-mr";
const TRAILER: &str = "Reviewed-By";
const ROSTER: &str = ".keel/mr-allowed";
pub(crate) const KNOWN_TIERS: &[&str] = &["email", "username", "key", "github"];
/// The tiers a project accepts when its config names none: the typed ones. The github tier is opt-in.
pub(crate) const DEFAULT_TIERS: &[&str] = &["email", "username", "key"];
/// The tier a pull-request approval signs with (E-GHFLOW2.W3): never typed.
const GITHUB: &str = "github";
/// The `report` a sign-off names when it attests the epic report carried as a note on the tip.
pub(crate) const REPORT_NOTE: &str = "refs/notes/report";
const NOT_AUTHORISED: &str = "A HUMAN must review this and confirm - an AI agent MUST NOT self-authorise.
      after the human has read the change and said yes:
        keel mr commit -m \"<msg>\" --as <identity>          (rule-bearing commit)
        keel mr confirm <wu|epic> [<tip>] --as <identity>   (WU / epic tip)
      the identity must be listed in the target's committed .keel/mr-allowed (keel mr setup, then keel mr commit)";

struct Entry {
    tier: String,
    identity: String,
    /// `<keytype> <base64>` of a `key` entry.
    key: Option<String>,
}

/// A GitHub login as GitHub allows one: letters, digits and single inner hyphens, 1 to 39 characters.
fn valid_login(s: &str) -> bool {
    (1..=39).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--")
}

/// A GitHub user id: decimal digits, no leading zero, at most 19 (so it fits a u64).
fn valid_user_id(s: &str) -> bool {
    (1..=19).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
}

/// Restricted so an identity can never carry allowed_signers pattern syntax (`*?,!"`) or whitespace.
fn valid_identity(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
}

/// The roster's meaningful lines, numbered from 0: trimmed, without blank lines and `#` comments.
fn roster_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines().map(str::trim).enumerate().filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
}

/// `<tier>: <identity>`, `key: <identity> <keytype> <base64> [comment]` or `github: <user id> <login>`
/// lines, read from `src`. A github line's identity is the numeric id, which a rename never changes;
/// the login is for the reader.
/// Lines whose tier is not in `tiers` authorise nothing; a malformed line is an error, never skipped.
fn parse_roster(text: &str, tiers: &[String], src: &str) -> Result<Vec<Entry>, String> {
    let mut out = vec![];
    for (n, line) in roster_lines(text) {
        let bad = || format!("keel: {src} line {}: expected `<tier>: <identity>`, `key: <identity> <ssh-pubkey>` or `github: <user id> <login>`", n + 1);
        let (tier, rest) = line.split_once(':').ok_or_else(bad)?;
        let tier = tier.trim();
        let mut f = rest.split_whitespace();
        let identity = f.next().filter(|i| valid_identity(i)).ok_or_else(bad)?;
        let key = match (tier, f.next(), f.next()) {
            // Only a real key type: an options field (cert-authority, ...) never reaches allowed_signers.
            ("key", Some(t), Some(b)) if ["ssh-", "ecdsa-", "sk-"].iter().any(|p| t.starts_with(p)) => Some(format!("{t} {b}")),
            ("key", ..) => return Err(bad()),
            (GITHUB, Some(login), None) if valid_user_id(identity) && valid_login(login) => None,
            (GITHUB, ..) => return Err(bad()),
            (_, None, _) => None,
            _ => return Err(bad()),
        };
        if tiers.iter().any(|t| t == tier) {
            out.push(Entry { tier: tier.into(), identity: identity.into(), key });
        }
    }
    Ok(out)
}

/// The committed roster at `rev` (`""` = the index), if that tree holds one.
fn roster_text(repo: &Path, rev: &str) -> Option<String> {
    git(repo, &["show", &format!("{rev}:{ROSTER}")], &[]).ok()
}

/// The roster that judges a change: the one committed on `judge` (the merge target, or HEAD for a
/// commit), else - bootstrap, said on stderr - the one the change carries (`carried`: its tip, or
/// `""` for the index). Never a file on disk: whoever writes a checkout must not pick its signers.
/// Returns the entries and where they were read, for the refusal texts.
fn roster(repo: &Path, cfg: &Config, judge: &str, carried: &str) -> Result<(Vec<Entry>, String), String> {
    if let Some(t) = cfg.mr.tiers.iter().find(|t| !KNOWN_TIERS.contains(&t.as_str())) {
        return Err(format!("keel: unknown mr tier '{t}' in .keel/config.toml (known: {})", KNOWN_TIERS.join(", ")));
    }
    if let Some(text) = roster_text(repo, judge) {
        let src = format!("{judge}:{ROSTER}");
        return Ok((parse_roster(&text, &cfg.mr.tiers, &src)?, src));
    }
    let Some(text) = roster_text(repo, carried) else {
        return Err(format!(
            "nobody is listed - neither {judge} nor the change commits {ROSTER}\n  fix -> keel mr setup --as <identity> [--key <path>], then land {ROSTER} with keel mr commit; a local .mr-allowed is no longer read"
        ));
    };
    let src = if carried.is_empty() { format!("the staged {ROSTER}") } else { format!("{carried}:{ROSTER}") };
    eprintln!("keel: {judge} commits no {ROSTER} - bootstrap: {src} judges this change");
    Ok((parse_roster(&text, &cfg.mr.tiers, &src)?, src))
}

/// The roster lines `new` adds (`+ `) and removes (`- `), each once; comments, blank lines,
/// spacing and order are not a change.
fn roster_change(old: &str, new: &str) -> Vec<String> {
    let lines = |t: &str| -> std::collections::BTreeSet<String> { roster_lines(t).map(|(_, l)| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect() };
    let (old, new) = (lines(old), lines(new));
    new.difference(&old).map(|l| format!("+ {l}")).chain(old.difference(&new).map(|l| format!("- {l}"))).collect()
}

/// The tier that authorises `id` to sign by typing (`keel mr commit`, `confirm`, a `Reviewed-By`
/// trailer): every tier but github, which signs only by an approval on GitHub.
fn typed_tier<'a>(allowed: &'a [Entry], id: &str) -> Option<&'a str> {
    let typed: Vec<&Entry> = allowed.iter().filter(|e| e.tier != GITHUB).collect();
    let mine = || typed.iter().filter(|e| e.identity == id);
    mine().find(|e| e.key.is_some()).or_else(|| mine().next()).map(|e| e.tier.as_str())
}

/// Whether `id` is listed under the github tier.
fn approves(allowed: &[Entry], id: &str) -> bool {
    allowed.iter().any(|e| e.tier == GITHUB && e.identity == id)
}

/// The refusal for a typed sign-off by an id that signs only by approval.
fn by_approval_only(src: &str, id: &str) -> String {
    format!("keel mr: refused - {id} is listed in {src} under the github tier only: it signs by approving the pull request on GitHub, never by a typed command\n  fix -> approve the pull request's head commit on GitHub, or sign as an identity listed under another tier")
}

/// The tier that authorises `id`; `key` wins, so a key holder can never sign off unsigned.
///
/// "Signed" means "a key listed in the target's committed roster signed", not "a human reviewed":
/// that a key needs a human's touch or passphrase is the owner's setup, which Keel cannot see (DEC-48).
fn tier_of<'a>(allowed: &'a [Entry], id: &str) -> Option<&'a str> {
    let mine = || allowed.iter().filter(|e| e.identity == id);
    mine().find(|e| e.key.is_some()).or_else(|| mine().next()).map(|e| e.tier.as_str())
}

fn keys_of<'a>(allowed: &'a [Entry], id: &str) -> Vec<&'a str> {
    allowed.iter().filter(|e| e.identity == id).filter_map(|e| e.key.as_deref()).collect()
}

/// The bytes a key-backed sign-off signs: versioned, labelled, one field per line, and bound to the
/// level, the range (base, tip), the attested report, if any, and the accepted review waiver, if any -
/// with none, those lines are absent, so the signature never covers a placeholder and a sign-off with
/// no waiver signs exactly the bytes it always has. Never read back from a note - always rebuilt.
fn payload(level: &str, base: &str, tip: &str, change: &str, report: Option<(&str, &str)>, waiver: Option<&str>) -> String {
    let mut p = format!("keel-mr/v2\nlevel: {level}\nbase: {base}\ntip: {tip}\nchange: {change}\n");
    if let Some((path, hash)) = report {
        p.push_str(&format!("report: {path}\nreport_hash: {hash}\n"));
    }
    if let Some(hash) = waiver {
        p.push_str(&format!("review: WAIVED {hash}\n"));
    }
    p
}

/// Check 8's waiver rule: the waiver the sign-off accepted (`signed`) is the one the cr note carries
/// now (`current`). `Some(cause)` when they differ. `reason`: the note's reason-only hash, which a keel
/// before E-SEED3.W6 signed: that sign-off is named as such, not as a changed waiver.
fn waiver_drift(signed: Option<&str>, current: Option<&str>, reason: Option<&str>, t: &str) -> Option<String> {
    match (signed, current) {
        (Some(s), Some(c)) if s != c && Some(s) == reason => Some(format!(
            "the sign-off on {t} signed only the review waiver's reason (an older keel); the signature now covers the whole waiver - re-sign with keel mr confirm"
        )),
        (Some(s), Some(c)) if s != c => Some(format!("the review waiver on {t} changed after the sign-off accepted it (its reason, counts, open findings, walk waivers or review state)")),
        (None, Some(_)) => Some(format!("the review on {t} is WAIVED, but its sign-off accepted no waiver (the waiver was written after signing)")),
        (Some(_), None) => Some(format!("the sign-off on {t} accepted a review waiver that the cr note no longer carries")),
        _ => None,
    }
}

/// One row of the review history: the commit, its cr note (`Err`: why it cannot be read), and its tag
/// (round and scope, or the fix-PASS it is).
type Row = (String, Result<CrNote, String>, String);

/// The cr notes on the first-parent commits of `base..tip`, newest first: every review the branch's
/// own line of history recorded (an epic's merged WUs keep theirs on their own tips), read in a fixed
/// number of git processes through plumbing only (review::line), so no display config can change or
/// drop a row. A note git cannot print or parse is a row of its own (`Err`), never dropped: the
/// signer must see that a round exists. A fix-PASS row names the files its commit changed. `own`: the
/// tip's note when the gate already read it, never read twice (B16).
fn review_history(repo: &Path, base: &str, tip: &str, own: Option<CrNote>) -> Result<Vec<Row>, String> {
    let entries = review::line(repo, base, tip, own)?;
    let tags = review::tags(&entries, |from, to| notes::own_names(repo, from, to, &[]));
    Ok(entries.into_iter().zip(tags).map(|(e, tag)| (e.commit, e.note, tag)).collect())
}

/// The history lines, one per row, recorded text made inert.
fn history_lines(history: &[Row]) -> String {
    let mut out = String::from("  review history (newest first):\n");
    for (c, n, tag) in history {
        let Ok(n) = n else {
            out.push_str(&format!("    {} unreadable cr note - git cannot print it or it does not parse as cr/v1; a round was recorded here\n", short(c)));
            continue;
        };
        let why = n.waiver().map(|w| format!(" - {}", w.trim())).unwrap_or_default();
        let open = if n.unreviewed() { "not reviewed".to_string() } else { format!("blockers {} importants {}", n.blockers, n.importants) };
        out.push_str(&printable(&format!("    {} {} {open}{why}{tag}", short(c), n.verdict)));
        out.push('\n');
    }
    out
}

/// What the signer reads before a waiver is accepted or written: the reason, what the review left
/// open (its unwaived findings, its walk waivers) - or that no review ran - and every round.
fn waiver_listing(tip: &str, cr: &CrNote, history: &[Row]) -> String {
    let mut out = format!("the review of {} is WAIVED: {}\n", short(tip), printable(cr.waiver().unwrap_or("").trim()));
    if cr.unreviewed() {
        out.push_str("  open: not reviewed - no review of this range ran on this tip, so what it would find is unknown\n");
    } else {
        out.push_str(&format!("  open: {} blockers, {} importants\n", cr.blockers, cr.importants));
    }
    for f in cr.open_findings() {
        let line = match (f["severity"].as_str(), f["location"].as_str(), f["claim"].as_str(), f.as_str()) {
            (Some(s), Some(l), Some(c), _) => format!("{s} {l}: {c}"),
            (Some(s), None, Some(c), _) => format!("{s}: {c}"),
            (None, _, Some(c), _) | (_, _, _, Some(c)) => c.to_string(),
            _ => f.to_string(),
        };
        out.push_str(&format!("    - {}\n", printable(&line)));
    }
    if let Some(old) = cr.extra.get("replaced") {
        let base = old["base"].as_str().unwrap_or("?");
        out.push_str(&printable(&format!(
            "  replaces a {} note on this tip that judged another range (base {}); its counts and findings are not this tip's",
            old["verdict"].as_str().unwrap_or("?"),
            short(base)
        )));
        out.push('\n');
    }
    for w in &cr.walk_waivers {
        out.push_str(&format!("  walk waiver {}: {}\n", printable(&w.seam), printable(w.why.trim())));
    }
    out.push_str(&history_lines(history));
    out
}

/// Agent-written text made inert for a terminal, by an allowlist: printable ASCII but `\`, and
/// Unicode letters and digits that render as themselves (std's alphanumeric, which also takes the
/// Alphabetic combining marks: they draw visible ink and cannot hide text), pass; every other
/// character - controls, format and invisible characters (bidi, zero-width, tags, variation
/// selectors), separators, private use, unassigned code points, the invisible filler letters, and
/// also non-ASCII punctuation, symbols and the non-Alphabetic marks, for which std has no category
/// test - is shown as `\u{..}`, and `\` as `\\`, so no recorded text can hide, reorder or redraw a
/// line of the listing the signer confirms, or forge an escape.
pub(crate) fn printable(s: &str) -> String {
    let shown = |c: char| {
        (c.is_ascii_graphic() && c != '\\')
            || c == ' '
            || (!c.is_ascii() && c.is_alphanumeric() && !matches!(c, '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}'))
    };
    s.chars()
        .map(|c| match c {
            '\\' => "\\\\".to_string(),
            c if shown(c) => c.to_string(),
            c => format!("\\u{{{:x}}}", c as u32),
        })
        .collect()
}

/// The code a `--waive-review` preview prints and its confirming re-run must give back. It names the
/// tip, the base, the listing shown and the note to be written (all but its identity, which carries the
/// time of writing), so the waiver written is exactly the one previewed - which proves what was shown,
/// not who read it.
fn waiver_code(tip: &str, base: &str, listing: &str, note: &CrNote) -> String {
    let mut body = serde_json::to_value(note).unwrap_or_default();
    if let Some(m) = body.as_object_mut() {
        for k in notes::Identity::keys() {
            m.remove(&k);
        }
    }
    sha256(format!("{tip}\n{base}\n{listing}\n{body}").as_bytes()).trim_start_matches("sha256:")[..12].to_string()
}

/// `--waive-review "<reason>"`: `Ok(None)` when absent, the trimmed reason when given, refused when
/// blank or missing (a following flag is not a reason).
fn waive_reason(args: &[&str]) -> Result<Option<String>, String> {
    if !args.contains(&"--waive-review") {
        return Ok(None);
    }
    if args.contains(&"--accept-waiver") {
        return Err("--waive-review and --accept-waiver together: the first waives this review yourself, the second accepts one the review tool wrote\n  fix -> give one of them".into());
    }
    match notes::waiver_reason(flag(args, "--waive-review")) {
        Ok(r) => Ok(Some(r)),
        Err(_) => Err("--waive-review needs the reason, in the signer's own words\n  fix -> keel mr confirm <wu|epic> <tip> --as <identity> --waive-review \"<why this review is waived>\"".into()),
    }
}

/// `keel mr confirm`'s positional arguments: every flag takes a value except `--roster-change`.
fn positionals<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let (mut pos, mut i) = (vec![], 0);
    while i < args.len() {
        match args[i] {
            "--roster-change" => i += 1,
            a if a.starts_with('-') => i += 2,
            a => {
                pos.push(a);
                i += 1;
            }
        }
    }
    pos
}

/// The docs whose `data-artifact` carries the level's result role: html under docs.root changed in
/// base..tip and present at the tip. Never an untracked or archived file: an epic whose report is
/// archived out of git by sign-off time names it with `--report` (owner ruling, DEC-40).
fn report_candidates(repo: &Path, cfg: &Config, base: &str, tip: &str, epic: bool) -> Result<Vec<String>, String> {
    let role = verify_doc::result_role(epic);
    let root = cfg.docs.root.trim_end_matches('/');
    let mut found = vec![];
    for n in notes::names(repo, base, tip, &[root.to_string()])?.iter().filter(|n| n.ends_with(".html")) {
        if git(repo, &["show", &format!("{tip}:{n}")], &[]).is_ok_and(|body| verify_doc::has_role(&body, role)) {
            found.push(n.clone());
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
    let role = verify_doc::result_role(epic);
    let found = report_candidates(repo, cfg, base, tip, epic).map_err(|e| (e, "fix the repo state, then re-run".into()))?;
    match &found[..] {
        [one] => Ok(Some(one.clone())),
        [] if cfg.mr.report == ReportPolicy::Optional => Ok(None),
        [] => Err((
            format!(
                "[mr] report = \"required\", and no {role} doc (data-artifact=\"{role}\") under {} is committed in this {}'s diff {}..{}",
                cfg.docs.root,
                if epic { "epic" } else { "WU" },
                short(base),
                short(tip)
            ),
            format!("commit the {role} report (skill keel-plan) on the tip, then keel mr confirm again - or name the one you read: keel mr confirm ... --report <path>"),
        )),
        several => Err((
            format!("{} {role} docs could be the report: {}", several.len(), several.join(", ")),
            "name the one you read: keel mr confirm ... --report <path>".into(),
        )),
    }
}

/// SHA-256 of the full `base..tip` diff text (the same bytes the cr and dod bindings hash).
fn change_hash(repo: &Path, base: &str, tip: &str) -> Result<String, String> {
    Ok(sha256(notes::diff_text(repo, base, tip, &[])?.as_bytes()))
}

/// SHA-256 of the report's exact bytes: the report note on the tip when `path` is `refs/notes/report`
/// (a committed file of that name is never the report); else the blob at the tip, else (epic only -
/// archived out of git by then) the raw local file. `Ok(None)` = an epic report found nowhere.
fn report_hash(repo: &Path, tip: &str, path: &str, epic: bool) -> Result<Option<String>, String> {
    if path == REPORT_NOTE {
        return match notes::raw(repo, "report", tip)? {
            Some(body) => Ok(Some(sha256(body.as_bytes()))),
            None => Err(format!("the tip {} carries no {REPORT_NOTE} note (keel notes write report <path>)", short(tip))),
        };
    }
    let ls = git(repo, &["ls-tree", tip, "--", path], &[])?;
    if let ["100644" | "100755", "blob", id] = ls.split('\t').next().unwrap_or("").split(' ').collect::<Vec<_>>()[..] {
        let o = no_git_env(Command::new("git"), true)
            .args(["cat-file", "blob", id])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("keel: cannot run git: {e}"))?;
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
        && ssh_keygen(&["-Y", "verify", "-f", &signers.to_string_lossy(), "-I", id, "-n", NAMESPACE, "-s", &sig_file.to_string_lossy()], data.as_bytes())
            .is_ok_and(|o| o.status.success());
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

/// `rule_bearing` entry: a trailing `/` is a prefix; otherwise a glob where
/// `*`/`?` stay inside one path segment and `**` crosses segments.
/// The roster itself always is: the policy's own list cannot exempt who may sign.
fn rule_bearing(patterns: &[String], path: &str) -> bool {
    path == ROSTER
        || patterns.iter().any(|p| {
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
fn signer(allowed: &[Entry], src: &str, args: &[&str]) -> Result<String, String> {
    if let Some(id) = flag(args, "--as") {
        return Ok(id.into());
    }
    let mut ids: Vec<&str> = allowed.iter().map(|e| e.identity.as_str()).collect();
    ids.sort();
    ids.dedup();
    match ids[..] {
        [one] => Ok(one.into()),
        [] => {
            Err(format!("keel mr: refused - {src} lists nobody\n  fix -> keel mr setup --as <identity> [--key <path>], then land {ROSTER} with keel mr commit"))
        }
        _ => Err("keel mr: several identities are listed - pass --as <identity>".into()),
    }
}

fn unlisted(src: &str, id: &str) -> String {
    format!("keel mr: refused - {id} is not listed in {src}\n  fix -> {NOT_AUTHORISED}")
}

fn setup(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    let id = flag(args, "--as").ok_or("usage: keel mr setup --as <identity> [--key <path>]")?;
    if !valid_identity(id) {
        return Err(format!("keel mr: identity '{id}' may only use letters, digits and @._+-"));
    }
    // The working file only: setup authorises nothing, the line counts once it is committed.
    let path = repo.join(ROSTER);
    let read = || match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        r => r.map_err(|e| format!("keel mr: cannot read {ROSTER}: {e}")),
    };
    let working = || parse_roster(&read()?, &cfg.mr.tiers, ROSTER);
    let allowed = working()?;
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
    let mut body = read()?;
    if !body.lines().any(|l| l.trim() == line) {
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, format!("{body}{line}\n")).map_err(|e| format!("keel: write {ROSTER}: {e}"))?;
    }
    let allowed = working()?; // also validates the line just written
    if let Some(k) = key {
        git(repo, &["config", "--local", "keel.signingkey", k], &[])?;
        // Prove the key signs here (private half present, or loaded in ssh-agent) and matches the roster.
        let probe = format!("keel-mr setup probe for {id}\n");
        let sig = sign(k, &probe)?;
        if !verify(repo, id, &keys_of(&allowed, id), &probe, &sig) {
            return Err(format!("keel mr: the key {k} signed, but its signature does not verify against {ROSTER}"));
        }
        println!("registered {id} (key) in {ROSTER}; keel.signingkey = {k}; a probe signature verified");
        println!("note: a key the agent's ssh-agent can use is a key the AI agent can use - prefer an sk- (FIDO2) key or ssh-add -c");
    } else {
        println!("registered {id} ({tier}) in {ROSTER} - an identity tier is a string match, not a signature");
    }
    println!("the line authorises nothing until it is committed: git add {ROSTER} && keel mr commit -m \"<msg>\" --as {id}");
    Ok(0)
}

fn commit(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    let msg = flag(args, "-m").filter(|m| !m.trim().is_empty()).ok_or("usage: keel mr commit -m <msg> [--as <identity>]")?;
    let (allowed, src) = roster(repo, cfg, "HEAD", "")?;
    let id = signer(&allowed, &src, args)?;
    let Some(tier) = typed_tier(&allowed, &id) else {
        eprintln!("{}", if approves(&allowed, &id) { by_approval_only(&src, &id) } else { unlisted(&src, &id) });
        return Ok(1);
    };
    let body = format!("{}\n\n{TRAILER}: {tier} {id}\n", msg.trim_end());
    let o = no_git_env(Command::new("git"), true).args(["commit", "-m", &body]).current_dir(repo).status().map_err(|e| e.to_string())?;
    Ok(if o.success() { 0 } else { 1 })
}

/// merge-base(target, tip) = tip exactly when the target already contains the tip: the range is
/// empty, and a sign-off would bind the empty diff over the note that attests the real change.
fn landed(tip: &str, base: &str, target: &str) -> Option<String> {
    (tip == base).then(|| {
        format!(
            "mr confirm refused - {} is already contained in {target} (merge-base = tip): the range is empty, so a sign-off here would bind the empty diff and replace the note that attests the real change\n  fix -> nothing to sign; the existing sign-off stands. To sign new work, commit it on the branch and confirm the new tip",
            short(tip)
        )
    })
}

fn confirm(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    const USAGE: &str = "usage: keel mr confirm <wu|epic> [<tip>] [--as <identity>] [--target R] [--report <path>] [--roster-change] [--accept-waiver <code> | --waive-review \"<reason>\" [--confirm-waiver <code>]]";
    let pos = positionals(args);
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
    // Sign last (Trawler DEC-27): every binding pins the diff, so the tree settles before a human signs.
    let waive = match waive_reason(args) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("mr confirm refused - {e}");
            return Ok(1);
        }
    };
    let Some(gate::Ready { tip, base, target, cr: judged }) = gate::sign_ready(repo, cfg, epic, &tip_ref, args, waive.is_some())? else {
        if waive.is_some() {
            eprintln!("mr confirm refused - sign last: everything but the review must be green on this exact tip before you waive it (the review is the one thing this waiver replaces)\n  fix -> keel notes write test|doc and whatever else the lines above name, then keel mr confirm ... --waive-review again");
        } else {
            eprintln!("mr confirm refused - sign last: get the notes and a bound cr PASS green on this exact tip first\n  fix -> keel notes write test|doc, run the code review (cr PASS), then keel mr confirm again");
        }
        return Ok(1);
    };
    if let Some(refusal) = landed(&tip, &base, &target) {
        eprintln!("{refusal}");
        return Ok(1);
    }
    // The roster check 8 will judge by: a sign-off the gate would refuse is refused before signing.
    let (allowed, src) = roster(repo, cfg, &target, &tip)?;
    let id = signer(&allowed, &src, args)?;
    let Some(tier) = typed_tier(&allowed, &id).map(String::from) else {
        eprintln!("{}", if approves(&allowed, &id) { by_approval_only(&src, &id) } else { unlisted(&src, &id) });
        return Ok(1);
    };
    // The one point a roster edit made inside a branch meets the signer, who reads a report, not the diff.
    let change = roster_change(&roster_text(repo, &base).unwrap_or_default(), &roster_text(repo, &tip).unwrap_or_default());
    if !change.is_empty() && !args.contains(&"--roster-change") {
        eprintln!(
            "mr confirm refused - {}..{} changes who may sign ({ROSTER}):\n{}\n  fix -> if you mean to sign this roster change, re-run with --roster-change",
            short(&base),
            short(&tip),
            change.iter().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")
        );
        return Ok(1);
    }
    // The report is settled before any waiver is shown, so the only refusal left after "<id> waives ..."
    // is a key-backed signature that fails; it writes nothing either.
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
    let branch = (tip_ref != tip && tip_ref != "HEAD").then(|| tip_ref.clone());
    // A waived review meets the signer here, shown with every round (UC-REV-2): the signer waives it in
    // this sign-off (`--waive-review`, written with the signature), or accepts a waiver the tool wrote.
    let mut owner_cr = None;
    // The review history, read once with the tip's cr note in it - the gate's read when it judged the
    // review (B16): the note the owner's waiver is written over (the writer refuses one it cannot
    // parse), and the waiver the tool wrote, if any.
    let history = review_history(repo, &base, &tip, judged)?;
    let stored = history.first().filter(|(c, ..)| *c == tip).map(|(_, n, _)| n.clone());
    let tool_waiver = stored.as_ref().and_then(|n| n.as_ref().ok()).filter(|n| n.waiver().is_some()).cloned();
    let waiver = match tool_waiver {
        _ if waive.is_some() => {
            let reason = waive.as_deref().unwrap_or_default();
            let ident = notes::identity(repo, tip.clone(), base.clone(), branch.clone())?;
            let cr = match notes::waive(stored, ident, Some(reason), notes::code_diff_hash(repo, cfg, &base, &tip)?) {
                Ok(cr) => cr,
                Err((what, fix)) => {
                    eprintln!("mr confirm refused - {what}\n  fix -> {fix}");
                    return Ok(1);
                }
            };
            let listing = waiver_listing(&tip, &cr, &history);
            eprint!("{listing}");
            let code = waiver_code(&tip, &base, &listing, &cr);
            match flag(args, "--confirm-waiver") {
                Some(c) if c == code => eprintln!("{id} waives the review of {}: {}", short(&tip), printable(reason)),
                Some(_) => {
                    eprintln!("mr confirm refused - the waiver above has changed since the preview whose code you gave (or the code is not its own); nothing is written\n  fix -> read the waiver above; to write it as shown, re-run with --confirm-waiver {code}");
                    return Ok(1);
                }
                None => {
                    eprintln!("mr confirm refused - a preview: nothing is written until you confirm the waiver above\n  fix -> to waive this review as shown, re-run the same command with --confirm-waiver {code}");
                    return Ok(1);
                }
            }
            let hash = cr.waiver_hash();
            owner_cr = Some(cr);
            hash
        }
        Some(cr) => {
            let listing = waiver_listing(&tip, &cr, &history);
            eprint!("{listing}");
            let code = waiver_code(&tip, &base, &listing, &cr);
            match flag(args, "--accept-waiver") {
                Some(c) if c == code => {}
                Some(_) => {
                    eprintln!("mr confirm refused - the waiver above has changed since the run whose code you gave (or the code is not its own); nothing is written\n  fix -> read the waiver above; to accept it as shown, re-run with --accept-waiver {code}");
                    return Ok(1);
                }
                None => {
                    eprintln!("mr confirm refused - the review of {} did not pass; it was waived, and nothing is signed until you accept the waiver above\n  fix -> if you accept it as shown, re-run with --accept-waiver {code}", short(&tip));
                    return Ok(1);
                }
            }
            // The waiver signed is the one listed, never a second read of the note.
            cr.waiver_hash()
        }
        None => {
            // A fix nobody reviewed meets the signer here, with the files it changed (UC-REV-3).
            if history.iter().any(|(_, n, _)| n.as_ref().is_ok_and(|n| review::kind(n) == review::Kind::FixPass)) {
                eprint!("{}", history_lines(&history));
            }
            None
        }
    };
    let attested = report.as_deref().zip(report_hash.as_deref());
    let what = attested.map_or("no report".to_string(), |(r, _)| r.to_string());
    let level = if epic { "epic" } else { "wu" };
    let change = change_hash(repo, &base, &tip)?;
    let sig = if tier == "key" {
        let key = git(repo, &["config", "--get", "keel.signingkey"], &[])
            .map_err(|_| format!("keel mr: {id} is key-backed but no signing key is configured\n  fix -> keel mr setup --as {id} --key <path>"))?;
        let data = payload(level, &base, &tip, &change, attested, waiver.as_deref());
        eprintln!("signing the {level} {tip_ref} ({}) and {what} as {id} ...", short(&tip));
        let sig = sign(&key, &data)?;
        if !verify(repo, &id, &keys_of(&allowed, &id), &data, &sig) {
            return Err(format!("keel mr: refused - keel.signingkey ({key}) is not a key {src} lists for {id}"));
        }
        Some(sig)
    } else {
        None
    };
    let note = MrNote {
        schema: "mr/v1".into(),
        id: notes::identity(repo, tip.clone(), base, branch)?,
        level: level.into(),
        signer: id.clone(),
        tier: tier.clone(),
        report,
        report_hash,
        waiver_hash: waiver.clone(),
        change_hash: change,
        sig,
        review: None,
    };
    // The owner's waiver and the sign-off land together or not at all (B17).
    let mut writes = vec![];
    if let Some(cr) = &owner_cr {
        writes.push(("cr", tip.as_str(), serde_json::to_string(cr).map_err(|e| e.to_string())?));
    }
    writes.push(("mr", tip.as_str(), serde_json::to_string(&note).map_err(|e| e.to_string())?));
    notes::store_all(repo, &writes)?;
    let accepted = if waiver.is_some() { " and accepts the review waiver" } else { "" };
    println!("mr/v1 on {} - {tier} {id} attests {what}{accepted}", short(&tip));
    Ok(0)
}

/// Check 8 for a wu/epic tip: `Err((what, fix))` on the first cause found, else the attested
/// `(report, report_hash)`, if any. Everything is rebuilt from the repo; the note contributes only
/// the signer, the report path and the (signed) report hash.
/// The roster is `target`'s committed one, else the tip's (bootstrap), never the checkout's.
/// `cr`: the tip's cr note when the caller already read it (checks 3-5); otherwise it is read here.
pub(crate) fn manual_review(
    repo: &Path,
    cfg: &Config,
    level: &str,
    tip: &str,
    base: &str,
    target: &str,
    cr: Option<&CrNote>,
) -> Result<Option<(String, String)>, (String, String)> {
    let again = |what: String| Err((what, format!("re-run keel mr confirm on the tip as it now stands\n      {NOT_AUTHORISED}")));
    let t = short(tip);
    let raw = match notes::raw(repo, "mr", tip) {
        Ok(Some(raw)) => raw,
        Ok(None) => return Err((format!("every {level} merge needs a human sign-off - tip {t} has no mr/v1 note"), NOT_AUTHORISED.into())),
        Err(e) => return again(format!("the mr note on {t} cannot be read: {e}")),
    };
    let n = match notes::parse::<MrNote>(&raw, "mr") {
        Ok(n) => n,
        Err(e) => return again(format!("the mr note on {t} is {}", notes::invalid("mr", &e))),
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
    let read;
    let cr = match cr {
        Some(cr) => Some(cr),
        None => {
            read = notes::cr_note(repo, tip);
            read.as_ref()
        }
    };
    let waiver = cr.and_then(CrNote::waiver_hash);
    if let Some(cause) = waiver_drift(n.waiver_hash.as_deref(), waiver.as_deref(), cr.and_then(CrNote::reason_hash).as_deref(), t) {
        return again(cause);
    }
    let attested = match (&n.report, &n.report_hash) {
        (Some(r), Some(h)) => Some((r.as_str(), h.as_str())),
        (None, None) => None,
        _ => return again(format!("the mr/v1 note on {t} names a report or a report_hash without the other")),
    };
    if attested.is_none() && cfg.mr.report == ReportPolicy::Required {
        let role = verify_doc::result_role(level == "epic");
        return Err((
            format!("the {level} sign-off on {t} attests no result report, but [mr] report = \"required\" (the target's committed policy)"),
            format!("write the {role} report on the tip, then re-run keel mr confirm\n      {NOT_AUTHORISED}"),
        ));
    }
    let (allowed, roster) = roster(repo, cfg, target, tip).map_err(|e| (e, NOT_AUTHORISED.into()))?;
    let not_listed = || {
        Err((
            format!("{t} is signed off by {}, whom {roster} does not list (under tiers {:?})", n.signer, cfg.mr.tiers),
            format!("a signer is added by landing their line in {ROSTER} on {target}, signed off by a signer it already lists\n      {NOT_AUTHORISED}"),
        ))
    };
    // An approval binds through its review; a typed sign-off through the typed tiers (a key wins).
    let tier = match (&n.review, typed_tier(&allowed, &n.signer)) {
        (_, Some("key")) => Some("key"),
        (Some(r), _) if approves(&allowed, &n.signer) => {
            if let Some(cause) = review_problem(r, tip, cfg.github.repo.as_deref()) {
                return again(format!("the approval on {t}: {cause}"));
            }
            Some(GITHUB)
        }
        (Some(_), _) => return not_listed(),
        (None, None) if approves(&allowed, &n.signer) => {
            return again(format!("{} is listed in {roster} under the github tier only, but the note on {t} carries no approval (no review)", n.signer))
        }
        (None, tier) => tier,
    };
    match tier {
        None => return not_listed(),
        Some("key") => {
            let Some(sig) = &n.sig else {
                return again(format!("{} is key-backed in {roster}, but the note on {t} carries no signature", n.signer));
            };
            let data = payload(level, base, tip, &change, attested, waiver.as_deref());
            if !verify(repo, &n.signer, &keys_of(&allowed, &n.signer), &data, sig) {
                return again(format!("{t}'s mr/v1 signature by {} does not verify over base..tip and the attested report against {roster}", n.signer));
            }
        }
        Some(_) => {} // identity tier: listed is all it proves; github: the review is bound above
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

/// Why an approval's review does not bind this tip, if it does not: it approved another commit, or its
/// page is not that review's on a pull request of a GitHub repository - of `repo` (`[github] repo`,
/// GitHub's owner/name, compared ignoring ASCII case as GitHub does) when the policy names one.
fn review_problem(r: &Review, tip: &str, repo: Option<&str>) -> Option<String> {
    if r.commit != tip {
        return Some(format!("its review approved {}, not this tip", printable(short(&r.commit))));
    }
    if let Some(repo) = repo {
        let on = r.url.strip_prefix("https://github.com/").and_then(|u| u.split("/pull/").next()).unwrap_or("");
        if !on.eq_ignore_ascii_case(repo) {
            return Some(format!("its review link {} is not on {} ([github] repo)", printable(&r.url), printable(repo)));
        }
    }
    let page =
        regex::Regex::new(r"^https://github\.com/[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}/pull/[1-9][0-9]{0,9}#pullrequestreview-([1-9][0-9]{0,19})$").ok()?;
    match page.captures(&r.url).and_then(|c| c[1].parse::<u64>().ok()) {
        Some(id) if id == r.id => None,
        _ => Some(format!("its review link {} is not review {}'s page on a pull request", printable(&r.url), r.id)),
    }
}

/// GitHub's review list (`GET /repos/{o}/{r}/pulls/{n}/reviews`), read as data, judged by GitHub's own
/// review decision, which is per pull request: for each id `allowed` lists under the github tier, its
/// latest (highest id) review whose state is a decision - APPROVED, CHANGES_REQUESTED, or DISMISSED
/// (withdrawn by a maintainer) - on any commit. A comment decides nothing. Blocked: any of those
/// latest decisions is CHANGES_REQUESTED, or a change request names no user or review id GitHub
/// gave (blocking is decided from the user and the state alone, before the commit or any other field,
/// so a missing field never lifts a block). Else the newest latest decision that is an APPROVED on
/// exactly `tip`, with a page of its own (and on `repo` when set), is the approval; a signer whose
/// latest approval is of another commit approves nothing here.
fn pick_approval(reviews: &serde_json::Value, tip: &str, allowed: &[Entry], repo: Option<&str>) -> Option<(String, Review)> {
    let mut latest: std::collections::BTreeMap<String, (u64, &serde_json::Value)> = Default::default();
    for v in reviews.as_array()? {
        let state = v["state"].as_str().unwrap_or("");
        if !["APPROVED", "CHANGES_REQUESTED", "DISMISSED"].contains(&state) {
            continue;
        }
        let user = v["user"]["id"].as_u64().map(|u| u.to_string());
        let id = v["id"].as_u64();
        let (Some(user), Some(id)) = (user, id) else {
            if state == "CHANGES_REQUESTED" && v["user"]["id"].as_u64().is_none_or(|u| approves(allowed, &u.to_string())) {
                return None;
            }
            continue;
        };
        if approves(allowed, &user) && latest.get(&user).is_none_or(|(w, _)| *w < id) {
            latest.insert(user, (id, v));
        }
    }
    if latest.values().any(|(_, v)| v["state"] == "CHANGES_REQUESTED") {
        return None;
    }
    latest
        .into_iter()
        .filter(|(_, (_, v))| v["state"] == "APPROVED" && v["commit_id"].as_str() == Some(tip))
        .filter_map(|(user, (id, v))| {
            let review = Review { id, url: v["html_url"].as_str()?.to_string(), commit: tip.to_string() };
            review_problem(&review, tip, repo).is_none().then_some((user, review))
        })
        .max_by_key(|(_, r)| r.id)
}

pub(crate) const SIGN_OFF_USAGE: &str = "usage: keel ci sign-off <wu|epic> <tip> --target <base> --reviews <file> [--report <path>]";

/// `keel ci sign-off` (E-GHFLOW2.W3): the approval writer's core, run by scripts/ci-sign-off.sh with
/// keel built from the pull request's base. `--reviews`: GitHub's review list for the pull request,
/// as the script fetched it. Exit 0: mr/v1 written (tier github, the review copied in); exit 3: no
/// approval to sign, nothing written; exit 1: refused, nothing written. The roster and policy are
/// the target's committed ones; a target with no roster signs nothing (no bootstrap by approval).
pub(crate) fn sign_off(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    let ([lvl, tip_ref], Some(target), Some(file)) = (positionals(args)[..].try_into().unwrap_or(["", ""]), notes::target_flag(args), flag(args, "--reviews"))
    else {
        return Err(SIGN_OFF_USAGE.into());
    };
    let epic = match lvl {
        "wu" => false,
        "epic" => true,
        _ => return Err(SIGN_OFF_USAGE.into()),
    };
    let refuse = |what: String| {
        eprintln!("sign-off refused - {what}");
        Ok(1)
    };
    let text = std::fs::read_to_string(file).map_err(|e| format!("keel ci sign-off: cannot read {file}: {e}"))?;
    let reviews: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("keel ci sign-off: {file} is not JSON: {e}"))?;
    if !reviews.is_array() {
        return Err(format!("keel ci sign-off: {file} is not GitHub's list of reviews"));
    }
    if let Some(t) = cfg.mr.tiers.iter().find(|t| !KNOWN_TIERS.contains(&t.as_str())) {
        return Err(format!("keel: unknown mr tier '{t}' in .keel/config.toml (known: {})", KNOWN_TIERS.join(", ")));
    }
    let tip =
        git(repo, &["rev-parse", "--verify", "-q", &format!("{tip_ref}^{{commit}}")], &[]).map_err(|_| format!("keel ci sign-off: no commit {tip_ref}"))?;
    let Some(listed) = roster_text(repo, target) else {
        return refuse(format!("{target} commits no {ROSTER}, so no approval can sign: a roster is landed by a typed sign-off"));
    };
    let src = format!("{target}:{ROSTER}");
    let allowed = parse_roster(&listed, &cfg.mr.tiers, &src)?;
    let Some((id, review)) = pick_approval(&reviews, &tip, &allowed, cfg.github.repo.as_deref()) else {
        println!("no approval to sign on {}: by GitHub's review decision on this commit, no id {src} lists under the github tier approves it, or one requests changes", short(&tip));
        return Ok(3);
    };
    let Some(gate::Ready { tip, base, target, cr }) = gate::sign_ready(repo, cfg, epic, tip_ref, args, false)? else {
        return refuse(format!("sign last: a check before the sign-off is red on {} (above)", short(&tip)));
    };
    if let Some(r) = landed(&tip, &base, &target) {
        return refuse(r);
    }
    if !roster_change(&listed, &roster_text(repo, &tip).unwrap_or_default()).is_empty() {
        return refuse(format!(
            "{}..{} changes who may sign ({ROSTER}): a roster change is signed by the typed keel mr confirm --roster-change",
            short(&base),
            short(&tip)
        ));
    }
    if cr.as_ref().is_some_and(|c| c.waiver().is_some()) {
        return refuse(format!("the review of {} is WAIVED: a waiver is accepted by the typed keel mr confirm --accept-waiver", short(&tip)));
    }
    let report = match pick_report(repo, cfg, args, &base, &tip, epic) {
        Ok(r) => r,
        Err((what, _)) => return refuse(what),
    };
    // GitHub shows a pull request's files at the approved commit, never a note: an approval attests a
    // report committed in the range, and the report note only through the typed sign-off (security
    // review I6: a note can be rewritten after the signer last saw anything of it).
    if report.as_deref() == Some(REPORT_NOTE) {
        return refuse(format!("an approval cannot attest {REPORT_NOTE}: GitHub never showed its bytes to the signer; commit the report in the range, or sign with keel mr confirm --report {REPORT_NOTE}"));
    }
    let report_hash = match &report {
        Some(r) => match report_hash(repo, &tip, r, epic) {
            Ok(Some(h)) => Some(h),
            Ok(None) => return refuse(format!("cannot read {r}")),
            Err(e) => return refuse(e),
        },
        None => None,
    };
    let note = MrNote {
        schema: "mr/v1".into(),
        id: notes::identity(repo, tip.clone(), base.clone(), None)?,
        level: lvl.into(),
        signer: id.clone(),
        tier: GITHUB.into(),
        report: report.clone(),
        report_hash,
        waiver_hash: None,
        change_hash: change_hash(repo, &base, &tip)?,
        sig: None,
        review: Some(review.clone()),
    };
    notes::store_all(repo, &[("mr", tip.as_str(), serde_json::to_string(&note).map_err(|e| e.to_string())?)])?;
    println!("mr/v1 on {} - github {id} approved it (review {}), attesting {}", short(&tip), review.id, report.as_deref().unwrap_or("no report"));
    Ok(0)
}

/// R50 (E-GHFLOW2.W3): before `keel mr` runs any git, its checkout's git links must be as git makes
/// them, by scripts/router-gh.sh's rule - workers can write the shared git dir, and the owner's
/// unsandboxed sign-off would follow whatever they wrote there. From `start` up to the first `.git`:
/// a directory (a main checkout) holds no `commondir`; a file (a linked checkout) names an admin dir
/// under `<git dir>/worktrees/` whose `commondir` is `../..` and whose `gitdir` names this `.git`
/// back; neither the git dir nor the admin dir holds a `config.worktree`. No link is followed.
/// `Some(why)` refuses.
pub(crate) fn checkout_problem(start: &Path) -> Option<String> {
    let owner = "the owner checks and removes it; nothing was run";
    let exists = |p: &Path| std::fs::symlink_metadata(p).is_ok();
    let (root, dotgit) = start.ancestors().find_map(|d| exists(&d.join(".git")).then(|| (d.to_path_buf(), d.join(".git"))))?;
    let meta = std::fs::symlink_metadata(&dotgit).ok()?;
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    let (common, admin) = if meta.file_type().is_symlink() {
        return Some(format!("{} is a symbolic link; git makes a directory or a gitdir file: {owner}", dotgit.display()));
    } else if meta.is_dir() {
        (dotgit.clone(), None)
    } else {
        let link = std::fs::read_to_string(&dotgit).unwrap_or_default();
        let Some(named) = link.strip_prefix("gitdir: ").map(|l| l.trim_end_matches('\n')).filter(|l| !l.is_empty() && !l.contains('\n')) else {
            return Some(format!("{} is not a gitdir link: {owner}", dotgit.display()));
        };
        let named = root.join(named);
        let bad = |why: &str| {
            Some(format!("{} names {}, which is not this checkout's admin dir as git makes one ({why}): {owner}", dotgit.display(), named.display()))
        };
        if std::fs::symlink_metadata(&named).map(|m| !m.is_dir()).unwrap_or(true) {
            return bad("not a directory");
        }
        let admin = canon(&named)?;
        if admin.parent().and_then(Path::file_name).is_none_or(|n| n != "worktrees") {
            return bad("not under <git dir>/worktrees/");
        }
        if std::fs::read_to_string(admin.join("commondir")).ok().as_deref().map(|c| c.strip_suffix('\n').unwrap_or(c)) != Some("../..") {
            return bad("its commondir is not ../..");
        }
        // git writes it absolute, or relative to the admin dir under worktree.useRelativePaths
        let back = std::fs::read_to_string(admin.join("gitdir")).ok().map(|g| admin.join(g.strip_suffix('\n').unwrap_or(&g)));
        if back.as_deref().and_then(|g| Some((g.file_name()? == ".git", canon(g.parent()?)?))) != Some((true, canon(&root)?)) {
            return bad("its gitdir does not name this checkout's .git");
        }
        (admin.join("../.."), Some(admin))
    };
    if exists(&common.join("commondir")) {
        return Some(format!("{} exists: a main git dir has none, so a worker may have written it: {owner}", common.join("commondir").display()));
    }
    for dir in std::iter::once(&common).chain(admin.as_ref()) {
        let f = dir.join("config.worktree");
        if exists(&f) {
            return Some(format!("{} exists: this repository uses none, so a worker may have written it: {owner}", f.display()));
        }
    }
    None
}

/// commit-msg hook: a staged change to a rule-bearing path needs a `Reviewed-By` trailer naming a
/// signer HEAD's roster lists. The trailer is typed text, so this stops an honest agent only (DEC-24).
pub fn commit_msg(repo: &Path, cfg: &Config, msg_file: &str) -> Result<i32, String> {
    // A merge authors nothing new; its commits were authorised on their branch.
    if git(repo, &["rev-parse", "--verify", "-q", "MERGE_HEAD"], &[]).is_ok() {
        return Ok(0);
    }
    let staged = notes::zpaths(&git(repo, &["diff", "--cached", "--name-only", "--no-renames", "-z"], &[])?);
    let hits: Vec<&str> = staged.iter().map(String::as_str).filter(|p| rule_bearing(&cfg.rule_bearing, p)).collect();
    if hits.is_empty() {
        return Ok(0);
    }
    let msg = std::fs::read_to_string(msg_file).map_err(|e| format!("keel: cannot read {msg_file}: {e}"))?;
    // HEAD's roster: a commit that edits the roster is judged by the one it replaces.
    let allowed = match roster(repo, cfg, "HEAD", "") {
        Ok((a, _)) => a,
        Err(e) => {
            eprintln!("keel: commit refused - {e}");
            return Ok(1);
        }
    };
    let id = msg.lines().filter_map(|l| l.strip_prefix(TRAILER)?.trim_start().strip_prefix(':')).next_back().and_then(|v| v.split_whitespace().next_back());
    if id.is_some_and(|id| typed_tier(&allowed, id).is_some()) {
        return Ok(0);
    }
    eprintln!("keel: commit refused - it touches rule-bearing {} with no {TRAILER} trailer from a listed signer\n  fix -> {NOT_AUTHORISED}", hits.join(", "));
    Ok(1)
}

pub fn run(repo: &Path, cfg: &Config, args: &[&str]) -> Result<i32, String> {
    match args {
        ["setup", rest @ ..] => setup(repo, cfg, rest),
        ["commit", rest @ ..] => commit(repo, cfg, rest),
        ["confirm", rest @ ..] => confirm(repo, cfg, rest),
        _ => Err("usage: keel mr <setup --as <id> [--key <path>] | commit -m <msg> [--as <id>] | confirm <wu|epic> [<tip>] [--as <id>] [--target R] [--report <path>] [--roster-change]>".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        approves, landed, parse_roster, payload, pick_approval, positionals, printable, review_problem, roster_change, rule_bearing, sha256, stale_change,
        tier_of, typed_tier, waive_reason, waiver_code, waiver_drift, waiver_listing,
    };
    use crate::notes::{CrNote, Review};
    use serde_json::json;

    #[test]
    fn a_github_line_is_a_numeric_id_and_a_login_and_nothing_else() {
        let all = tiers(&["username", "github", "key"]);
        let got = parse_roster("github: 5499546 lesslucifer\nusername: vu\n", &all, "r").unwrap();
        assert_eq!(got.iter().map(|e| (e.tier.as_str(), e.identity.as_str())).collect::<Vec<_>>(), [("github", "5499546"), ("username", "vu")]);
        assert!(
            parse_roster("github: 5499546 lesslucifer\n", &tiers(&["username"]), "r").unwrap().is_empty(),
            "a tier the policy does not accept authorises nothing"
        );
        for bad in [
            "github: 5499546",
            "github: lesslucifer 5499546",
            "github: 05499546 lesslucifer",
            "github: 5499546 less_lucifer",
            "github: 5499546 -less",
            "github: 5499546 less-",
            "github: 5499546 le--ss",
            "github: 5499546 lesslucifer extra",
            "github: 12345678901234567890 x",
            &format!("github: 1 {}", "a".repeat(40)),
        ] {
            let e = parse_roster(&format!("{bad}\n"), &all, "HEAD:.keel/mr-allowed").err().unwrap_or_else(|| panic!("{bad} parsed"));
            assert!(e.contains("line 1") && e.contains("github: <user id> <login>"), "{bad}: {e}");
        }
        // generated lines (seeded LCG): a github line parses exactly when its id is digits without a
        // leading zero (1 to 19 of them) and its login is GitHub's shape
        let alphabet = b"0123456789az-_Z";
        let mut x: u64 = 0x9e7;
        for _ in 0..2000 {
            let mut word = |n: u64| -> String { (0..(lcg(&mut x) % n)).map(|_| alphabet[(lcg(&mut x) % alphabet.len() as u64) as usize] as char).collect() };
            let (id, login) = (word(22), word(42));
            let id_ok = (1..=19).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()) && !id.starts_with('0');
            let login_ok = (1..=39).contains(&login.len())
                && login.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !login.starts_with('-')
                && !login.ends_with('-')
                && !login.contains("--");
            let parsed = parse_roster(&format!("github: {id} {login}\n"), &all, "r");
            assert_eq!(parsed.is_ok(), id_ok && login_ok, "github: {id} {login}");
        }
    }

    #[test]
    fn a_github_id_never_signs_by_typing_and_a_typed_id_never_by_approval() {
        let r = parse_roster("github: 77 owner\nusername: vu\nusername: 88\ngithub: 88 both\n", &tiers(&["username", "github"]), "r").unwrap();
        assert_eq!((tier_of(&r, "77"), typed_tier(&r, "77"), approves(&r, "77")), (Some("github"), None, true));
        assert_eq!((typed_tier(&r, "vu"), approves(&r, "vu")), (Some("username"), false));
        assert_eq!((typed_tier(&r, "88"), approves(&r, "88")), (Some("username"), true), "listed twice: each path finds its own line");
    }

    fn rv(id: u64, user: serde_json::Value, state: &str, commit: &str, url: &str) -> serde_json::Value {
        json!({"id": id, "user": {"id": user}, "state": state, "commit_id": commit, "html_url": url})
    }

    #[test]
    fn an_approval_signs_only_under_githubs_review_decision_on_the_pull_request() {
        let allowed = parse_roster("github: 7 a\ngithub: 8 b\nusername: 9\n", &tiers(&["username", "github"]), "r").unwrap();
        let (tip, old) = ("t".repeat(40), "o".repeat(40));
        let page = |id: u64| format!("https://github.com/o/r/pull/3#pullrequestreview-{id}");
        let at = |id: u64, user: u64, state: &str, commit: &str| rv(id, json!(user), state, commit, &page(id));
        let pick = |list: serde_json::Value| pick_approval(&list, &tip, &allowed, None).map(|(who, r)| (who, r.id));
        let list = json!([at(5, 7, "APPROVED", &tip), at(9, 8, "APPROVED", &tip), at(4, 8, "APPROVED", &tip)]);
        let (who, r) = pick_approval(&list, &tip, &allowed, None).unwrap();
        assert_eq!((who.as_str(), r), ("8", Review { id: 9, url: page(9), commit: tip.clone() }));
        assert!(pick_approval(&json!({"id": 1}), &tip, &allowed, None).is_none(), "not a list");
        // a signer's later change request withdraws their approval; a later approval restores it
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &tip), at(6, 7, "CHANGES_REQUESTED", &tip)])), None);
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &tip), at(6, 7, "CHANGES_REQUESTED", &tip), at(7, 7, "APPROVED", &tip)])), Some(("7".into(), 7)));
        // the order GitHub lists them in does not matter: the latest is the highest id
        assert_eq!(pick(json!([at(6, 7, "CHANGES_REQUESTED", &tip), at(5, 7, "APPROVED", &tip)])), None);
        // GitHub's decision is per pull request (security review round 2, F3, F5): a listed signer's
        // change request on an OLDER commit still blocks, until that signer's own later decision
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &tip), at(6, 8, "CHANGES_REQUESTED", &old)])), None);
        assert_eq!(pick(json!([at(4, 8, "CHANGES_REQUESTED", &old), at(5, 7, "APPROVED", &tip)])), None);
        assert_eq!(pick(json!([at(4, 8, "CHANGES_REQUESTED", &old), at(5, 8, "APPROVED", &tip)])), Some(("8".into(), 5)));
        assert_eq!(pick(json!([at(4, 7, "CHANGES_REQUESTED", &old), at(5, 7, "APPROVED", &tip)])), Some(("7".into(), 5)));
        assert_eq!(pick(json!([at(4, 8, "CHANGES_REQUESTED", &old), at(5, 8, "DISMISSED", &old), at(6, 7, "APPROVED", &tip)])), Some(("7".into(), 6)));
        // an approval counts only on exactly the tip: a signer whose latest approval is on an older commit signs nothing
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &old)])), None);
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &tip), at(6, 7, "APPROVED", &old)])), None, "the signer's latest decision approved another commit");
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &old), at(6, 7, "APPROVED", &tip)])), Some(("7".into(), 6)));
        // another listed signer's change request blocks; an unlisted id's does not
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &tip), at(6, 8, "CHANGES_REQUESTED", &tip)])), None);
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &tip), at(6, 9, "CHANGES_REQUESTED", &tip), at(7, 42, "CHANGES_REQUESTED", &tip)])), Some(("7".into(), 5)));
        // a dismissed approval approves nothing; a comment after an approval changes nothing
        assert_eq!(pick(json!([at(5, 7, "DISMISSED", &tip)])), None);
        assert_eq!(pick(json!([at(5, 7, "APPROVED", &tip), at(6, 7, "COMMENTED", &tip)])), Some(("7".into(), 5)));
        // a change request blocks whatever else it holds (F4): a broken page, no commit, no review id,
        // or no user GitHub could name
        let approved = at(5, 7, "APPROVED", &tip);
        for (why, edit) in [
            ("a broken page", json!({"html_url": "x"})),
            ("a null commit", json!({"commit_id": null})),
            ("no commit", json!({"commit_id": "drop"})),
            ("a null review id", json!({"id": null})),
            ("a null user", json!({"user": null})),
            ("a string user id", json!({"user": {"id": "8"}})),
        ] {
            let mut cr = at(6, 8, "CHANGES_REQUESTED", &tip);
            for (k, v) in edit.as_object().unwrap() {
                if v == "drop" {
                    cr.as_object_mut().unwrap().remove(k);
                } else {
                    cr[k] = v.clone();
                }
            }
            assert_eq!(pick(json!([approved.clone(), cr])), None, "{why}");
        }
        // generated lists (seeded LCG) against the rule written out: a change request no listed id can be
        // told from blocks; else per listed id its latest (highest id) APPROVED, CHANGES_REQUESTED or
        // DISMISSED on any commit; none CHANGES_REQUESTED, and the newest latest-APPROVED on exactly
        // the tip with its own page is signed
        let states = ["APPROVED", "COMMENTED", "CHANGES_REQUESTED", "DISMISSED", "PENDING", "approved"];
        let mut x: u64 = 0x5160;
        for _ in 0..3000 {
            let n = lcg(&mut x) % 7;
            let mut ids: Vec<u64> = (1..=60).collect();
            let list: Vec<serde_json::Value> = (0..n)
                .map(|_| {
                    let id = ids.remove((lcg(&mut x) % ids.len() as u64) as usize);
                    let user = match lcg(&mut x) % 6 {
                        0 => json!(7),
                        1 => json!(9),
                        2 => json!("8"),
                        3 => json!(null),
                        _ => json!(8),
                    };
                    let commit = match lcg(&mut x) % 6 {
                        0 | 1 => json!(old),
                        2 => json!(null),
                        _ => json!(tip),
                    };
                    let url = if lcg(&mut x).is_multiple_of(5) { page(id + 100) } else { page(id) };
                    json!({"id": id, "user": {"id": user}, "state": states[(lcg(&mut x) % states.len() as u64) as usize], "commit_id": commit, "html_url": url})
                })
                .collect();
            let listed = |v: &serde_json::Value| matches!(v["user"]["id"].as_u64(), Some(7 | 8));
            let unknown = |v: &serde_json::Value| v["user"]["id"].as_u64().is_none();
            let mut latest: std::collections::BTreeMap<u64, &serde_json::Value> = Default::default();
            for v in list.iter().filter(|v| listed(v) && ["APPROVED", "CHANGES_REQUESTED", "DISMISSED"].contains(&v["state"].as_str().unwrap())) {
                let u = v["user"]["id"].as_u64().unwrap();
                if latest.get(&u).is_none_or(|w| w["id"].as_u64() < v["id"].as_u64()) {
                    latest.insert(u, v);
                }
            }
            let blocked = list.iter().any(|v| v["state"] == "CHANGES_REQUESTED" && unknown(v)) || latest.values().any(|v| v["state"] == "CHANGES_REQUESTED");
            let want = if blocked {
                None
            } else {
                latest
                    .values()
                    .filter(|v| v["state"] == "APPROVED" && v["commit_id"] == json!(tip) && v["html_url"] == json!(page(v["id"].as_u64().unwrap())))
                    .map(|v| v["id"].as_u64().unwrap())
                    .max()
            };
            assert_eq!(pick_approval(&json!(list), &tip, &allowed, None).map(|(_, r)| r.id), want, "{list:?}");
        }
    }

    #[test]
    fn a_review_binds_only_its_own_page_on_a_github_pull_request_and_the_tip() {
        let tip = "a".repeat(40);
        let r = |url: &str, commit: &str| Review { id: 42, url: url.into(), commit: commit.into() };
        assert_eq!(review_problem(&r("https://github.com/own/Keel/pull/7#pullrequestreview-42", &tip), &tip, None), None);
        assert!(review_problem(&r("https://github.com/own/Keel/pull/7#pullrequestreview-42", &"b".repeat(40)), &tip, None).unwrap().contains("not this tip"));
        for bad in [
            "https://github.com/own/Keel/pull/7#pullrequestreview-43",
            "https://github.com/own/Keel/pull/7#pullrequestreview-042",
            "http://github.com/own/Keel/pull/7#pullrequestreview-42",
            "https://github.com.evil/own/Keel/pull/7#pullrequestreview-42",
            "https://github.com/own/Keel/issues/7#pullrequestreview-42",
            "https://github.com/own/Keel/pull/7#pullrequestreview-42\n",
            "https://github.com/own/Keel/pull/7/#pullrequestreview-42",
            "https://github.com/own/Keel/pull/0#pullrequestreview-42",
        ] {
            assert!(review_problem(&r(bad, &tip), &tip, None).unwrap().contains("is not review 42's page"), "{bad}");
        }
        // with [github] repo set, the page must be on that repository (GitHub's names ignore case)
        let page = r("https://github.com/own/Keel/pull/7#pullrequestreview-42", &tip);
        assert_eq!(review_problem(&page, &tip, Some("own/Keel")), None);
        assert_eq!(review_problem(&page, &tip, Some("OWN/keel")), None);
        for other in ["other/Keel", "own/Keel2", "own/Kee", "own"] {
            assert!(review_problem(&page, &tip, Some(other)).unwrap().contains("not on"), "{other}");
        }
    }

    fn lcg(x: &mut u64) -> u64 {
        *x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *x >> 33
    }

    fn tiers(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_roster_reads_the_same_from_any_tree_and_a_bad_line_is_an_error() {
        let text = "# owners\n\nemail: a@b.c\nusername: vu\nkey: vu ssh-ed25519 AAAA comment here\n";
        let all = parse_roster(text, &tiers(&["email", "username", "key"]), "r").unwrap();
        let got: Vec<_> = all.iter().map(|e| (e.tier.as_str(), e.identity.as_str(), e.key.as_deref())).collect();
        assert_eq!(got, [("email", "a@b.c", None), ("username", "vu", None), ("key", "vu", Some("ssh-ed25519 AAAA"))]);
        let keys = parse_roster(text, &tiers(&["key"]), "r").unwrap();
        assert_eq!(keys.iter().map(|e| e.tier.as_str()).collect::<Vec<_>>(), ["key"], "a tier the policy does not accept authorises nothing");
        for (bad, line) in
            [("key: vu cert-authority ssh-ed25519 AAAA", 2), ("username: v*u", 2), ("key: vu", 2), ("username: vu extra", 2), ("no colon here", 2)]
        {
            let e = parse_roster(&format!("username: ok\n{bad}\n"), &tiers(&["username", "key"]), "HEAD:.keel/mr-allowed")
                .err()
                .unwrap_or_else(|| panic!("{bad} parsed"));
            assert!(e.contains(&format!("HEAD:.keel/mr-allowed line {line}")), "{bad}: {e}");
        }
        // generated rosters (seeded LCG) under generated policies: every entry's tier is in the policy
        let names = ["email", "username", "key"];
        let mut x: u64 = 0x5eed;
        for _ in 0..300 {
            let policy: Vec<String> = names.iter().filter(|_| lcg(&mut x).is_multiple_of(2)).map(|s| s.to_string()).collect();
            let roster: String = (0..lcg(&mut x) % 6)
                .map(|i| match lcg(&mut x) % 3 {
                    0 => format!("email: u{i}@x.y\n"),
                    1 => format!("username: u{i}\n"),
                    _ => format!("key: u{i} ssh-ed25519 AAAA{i}\n"),
                })
                .collect();
            for e in parse_roster(&roster, &policy, "r").unwrap() {
                assert!(policy.contains(&e.tier), "{} under {policy:?}", e.tier);
            }
        }
    }

    #[test]
    fn the_roster_path_is_rule_bearing_whatever_the_policy_lists() {
        assert!(rule_bearing(&[], ".keel/mr-allowed"));
        for p in [".keel/mr-allowed.bak", "x/.keel/mr-allowed", ".mr-allowed", ".keel/config.toml"] {
            assert!(!rule_bearing(&[], p), "{p}");
        }
        assert!(rule_bearing(&tiers(&[".keel/"]), ".keel/config.toml"));
    }

    #[test]
    fn a_roster_change_counts_exactly_the_lines_the_parser_reads() {
        let tiers: Vec<String> = crate::mr::KNOWN_TIERS.iter().map(|t| t.to_string()).collect();
        for t in ["", "  \n\t\n", "# c\n  # indented\n", "username: a\n", " \tusername:  a \n# c\n", "email: a@b\n\n"] {
            let read = parse_roster(t, &tiers, "t").unwrap().len();
            assert_eq!(roster_change("", t).len(), read, "{t:?}");
        }
    }

    #[test]
    fn a_roster_change_lists_every_added_and_removed_line_once() {
        let old = "key: vu ssh-ed25519 AAAA\nusername: gone\n";
        let new = "# a comment\n\nkey: two  ssh-ed25519 BBBB\nkey: two ssh-ed25519 BBBB\n  key: vu ssh-ed25519 AAAA\n";
        assert_eq!(roster_change(old, new), ["+ key: two ssh-ed25519 BBBB", "- username: gone"]);
        assert!(roster_change("a: b\nc: d\n", "# x\n\nc:   d\na: b\n").is_empty(), "comments, blanks, spacing and order are no change");
        // generated pairs (seeded LCG): empty exactly when both sides list the same set of lines
        let mut x: u64 = 0xace;
        for _ in 0..500 {
            let mut side = || -> Vec<String> { (0..lcg(&mut x) % 4).map(|_| format!("username: u{}", lcg(&mut x) % 3)).collect() };
            let (a, b) = (side(), side());
            let set = |v: &[String]| v.iter().cloned().collect::<std::collections::BTreeSet<_>>();
            assert_eq!(roster_change(&a.join("\n"), &b.join("\n")).is_empty(), set(&a) == set(&b), "{a:?} {b:?}");
        }
    }

    #[test]
    fn landed_refuses_exactly_when_the_tip_is_its_own_base() {
        let t = "84ba8e3d0123456789abcdef0123456789abcdef";
        let r = landed(t, t, "epic/e").unwrap();
        assert!(r.contains("84ba8e3d01 is already contained in epic/e") && r.contains("fix -> nothing to sign"), "{r}");
        // generated pairs (seeded LCG), equal and unequal alike: Some iff the two shas are equal
        let mut x: u64 = 0x5eed;
        for _ in 0..500 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let a = format!("{:040x}", (x >> 33) % 4);
            let b = format!("{:040x}", (x >> 13) % 4);
            assert_eq!(landed(&a, &b, "main").is_some(), a == b, "{a} {b}");
        }
    }

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
        let with = payload("wu", "b", "t", "c", Some(("docs/r.html", "h")), None);
        assert_eq!(with, "keel-mr/v2\nlevel: wu\nbase: b\ntip: t\nchange: c\nreport: docs/r.html\nreport_hash: h\n");
        let without = payload("epic", "b", "t", "c", None, None);
        assert_eq!(without, "keel-mr/v2\nlevel: epic\nbase: b\ntip: t\nchange: c\n");
        assert!(!without.contains("report"));
    }

    #[test]
    fn the_payload_gains_the_review_line_exactly_when_a_waiver_is_accepted() {
        for report in [None, Some(("docs/r.html", "h"))] {
            let plain = payload("wu", "b", "t", "c", report, None);
            let waived = payload("wu", "b", "t", "c", report, Some("sha256:ab"));
            assert_eq!(waived, format!("{plain}review: WAIVED sha256:ab\n"), "the line is appended, every earlier byte unchanged");
            assert!(!plain.contains("review"), "{plain}");
        }
    }

    #[test]
    fn check_8_holds_the_sign_off_to_the_waiver_the_note_carries_now() {
        assert_eq!(waiver_drift(None, None, None, "t"), None);
        assert_eq!(waiver_drift(Some("sha256:a"), Some("sha256:a"), Some("sha256:a"), "t"), None, "a waiver whose text is its reason");
        for (signed, current, reason, needle) in [
            (Some("sha256:a"), Some("sha256:b"), Some("sha256:r"), "changed after"),
            (None, Some("sha256:b"), Some("sha256:r"), "accepted no waiver"),
            (Some("sha256:a"), None, None, "no longer carries"),
            (Some("sha256:r"), Some("sha256:b"), Some("sha256:r"), "older keel"),
        ] {
            let cause = waiver_drift(signed, current, reason, "t").unwrap_or_else(|| panic!("{signed:?} {current:?} passed"));
            assert!(cause.contains(needle) && cause.contains("waiver"), "{cause}");
        }
    }

    #[test]
    fn confirm_reads_every_flag_value_and_only_roster_change_as_valueless() {
        assert_eq!(positionals(&["wu", "--accept-waiver", "0123456789ab", "--as", "vu"]), ["wu"]);
        assert_eq!(positionals(&["--roster-change", "epic", "tip", "--report", "r.html", "--accept-waiver", "c"]), ["epic", "tip"]);
        assert_eq!(positionals(&["wu", "--target", "epic/e", "t"]), ["wu", "t"]);
    }

    #[test]
    fn the_waiver_listing_shows_the_reason_what_is_open_and_every_round() {
        let cr = |v: &str, extra: serde_json::Value| -> CrNote {
            let mut j = serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
                "timestamp": 0, "verdict": v, "blockers": 1, "importants": 0, "code_diff_hash": "h"});
            j.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            serde_json::from_value(j).unwrap()
        };
        let tip = cr(
            "WAIVED",
            serde_json::json!({"waiver": "review cap: 5 rounds, 1 blockers open",
            "findings": [{"severity": "blocker", "location": "src/a.rs:3", "claim": "a() is wrong"}, {"severity": "important", "claim": "no location"},
                {"severity": "important", "location": "x:1", "claim": "waived one", "waived": "ok"}, "bare text", 7],
            "walk_waivers": [{"seam": "abc123", "why": " safe "}]}),
        );
        let history = vec![
            (
                "1111111111aa".to_string(),
                Ok(cr("WAIVED", serde_json::json!({"waiver": "review cap: 5 rounds, 1 blockers open"}))),
                " [round 2, full]".to_string(),
            ),
            (
                "4444444444dd".to_string(),
                Ok(cr("PASS", serde_json::json!({"fixes": "2222222222bb2222222222bb2222222222bb2222"}))),
                " [fix-PASS of round 1 on 2222222222; no reviewers ran; changed: src/a.rs]".to_string(),
            ),
            ("2222222222bb".to_string(), Ok(cr("FAIL", serde_json::json!({}))), " [round 1, full]".to_string()),
            ("3333333333cc".to_string(), Err("unreadable".into()), String::new()),
        ];
        let l = waiver_listing("1111111111aa", &tip, &history);
        for needle in [
            "the review of 1111111111 is WAIVED: review cap: 5 rounds, 1 blockers open",
            "open: 1 blockers, 0 importants",
            "- blocker src/a.rs:3: a() is wrong",
            "- important: no location",
            "- bare text",
            "- 7",
            "walk waiver abc123: safe",
            "1111111111 WAIVED blockers 1 importants 0 - review cap: 5 rounds, 1 blockers open [round 2, full]\n",
            "4444444444 PASS blockers 1 importants 0 [fix-PASS of round 1 on 2222222222; no reviewers ran; changed: src/a.rs]\n",
            "2222222222 FAIL blockers 1 importants 0 [round 1, full]\n",
            "3333333333 unreadable cr note",
        ] {
            assert!(l.contains(needle), "missing '{needle}' in:\n{l}");
        }
        assert!(!l.contains("waived one"), "a waived finding is not open:\n{l}");
        assert!(l.find("1111111111 WAIVED").unwrap() < l.find("2222222222 FAIL").unwrap(), "newest first");
        let unreviewed = waiver_listing("t", &cr("WAIVED", serde_json::json!({"waiver": "r", "unreviewed": true})), &[]);
        assert!(unreviewed.contains("not reviewed") && !unreviewed.contains("blockers,") && !unreviewed.contains("    - "), "{unreviewed}");
    }

    #[test]
    fn the_waiver_code_names_exactly_what_was_shown_and_will_be_written() {
        let note = |refuted: &str, ts: u64| -> CrNote {
            serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
                "timestamp": ts, "verdict": "WAIVED", "waiver": "r", "blockers": 0, "importants": 0, "code_diff_hash": "h", "refuted": [refuted]}))
            .unwrap()
        };
        let c = waiver_code("t", "b", "listing", &note("x", 1));
        assert_eq!(c.len(), 12);
        assert!(c.chars().all(|x| x.is_ascii_hexdigit() && !x.is_ascii_uppercase()), "{c}");
        assert_eq!(c, waiver_code("t", "b", "listing", &note("x", 99)), "the time of writing is not part of it");
        for (t, b, l, r) in [("u", "b", "listing", "x"), ("t", "c", "listing", "x"), ("t", "b", "listing2", "x"), ("t", "b", "listing", "y")] {
            assert_ne!(c, waiver_code(t, b, l, &note(r, 1)), "{t} {b} {l} {r}");
        }
    }

    #[test]
    fn recorded_text_reaches_the_terminal_inert() {
        assert_eq!(printable("plain text: ok, café, 日本"), "plain text: ok, café, 日本");
        for c in [
            '\u{202e}', '\u{202a}', '\u{2066}', '\u{2069}', '\u{200e}', '\u{200f}', '\u{61c}', '\u{200b}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{2028}',
            '\u{2029}', '\u{ad}',
        ] {
            let out = printable(&format!("a{c}b"));
            assert_eq!(out, format!("a\\u{{{:x}}}b", c as u32), "{:x}", c as u32);
        }
        assert_eq!(printable("a\u{1b}[2Kb\u{7}c\rd\ne\u{9b}f"), r"a\u{1b}[2Kb\u{7}c\u{d}d\u{a}e\u{9b}f");
        let cr: CrNote = serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null,
            "committer": "c", "timestamp": 0, "verdict": "WAIVED", "waiver": "r\u{1b}[1A", "blockers": 1, "importants": 0, "code_diff_hash": "h",
            "findings": [{"severity": "blocker\u{1b}", "location": "l", "claim": "c\u{1b}[2K"}], "walk_waivers": [{"seam": "s\u{7}", "why": "w\u{1b}"}]}))
        .unwrap();
        let history = vec![("1111111111".to_string(), Ok(round("FAIL\u{1b}")), " [round 1, full]".to_string())];
        fn round(v: &str) -> CrNote {
            serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
                "timestamp": 0, "verdict": v, "waiver": "x\u{1b}", "blockers": 0, "importants": 0, "code_diff_hash": "h"}))
            .unwrap()
        }
        let l = waiver_listing("t", &cr, &history);
        assert!(l.chars().all(|c| c == '\n' || !c.is_control()), "{l:?}");
        assert_eq!(l.matches('\n').count(), 6, "one line per entry, none injected: {l}");
    }

    #[test]
    fn waive_review_takes_the_signers_own_non_blank_reason() {
        assert_eq!(waive_reason(&["wu", "--as", "vu"]), Ok(None));
        assert_eq!(waive_reason(&["wu", "--waive-review", "  ruling  ", "--as", "vu"]), Ok(Some("ruling".into())));
        for bad in [&["wu", "--waive-review"][..], &["wu", "--waive-review", ""], &["wu", "--waive-review", " \t"], &["wu", "--waive-review", "--as", "vu"]] {
            let e = waive_reason(bad).unwrap_err();
            assert!(e.contains("fix ->") && e.contains("--waive-review"), "{bad:?}: {e}");
        }
        assert!(waive_reason(&["wu", "--waive-review", "r", "--accept-waiver"]).unwrap_err().contains("together"));
        assert_eq!(positionals(&["wu", "--waive-review", "r", "--as", "vu", "--confirm-waiver", "abc"]), ["wu"], "flag values are never positionals");
    }

    /// B2: every character either reaches the terminal as itself and is plainly visible, or is escaped.
    #[test]
    fn printable_lets_through_only_what_it_can_show_and_escapes_everything_else() {
        let visible = |c: char| c == ' ' || c.is_ascii_graphic() || (c.is_alphanumeric() && !matches!(c, '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}'));
        for cp in 0..=0x10ffffu32 {
            let Some(c) = char::from_u32(cp) else { continue };
            let out = printable(&c.to_string());
            if c == '\\' {
                assert_eq!(out, r"\\");
            } else if out != c.to_string() {
                assert_eq!(out, format!("\\u{{{cp:x}}}"), "{cp:x}");
            } else {
                assert!(visible(c) && c != '\\', "{cp:x} passes unescaped");
            }
        }
        for cp in [0xe0001u32, 0xe0041, 0xe007f, 0xfe00, 0xfe0f, 0xe0100, 0xe01ef, 0x180b, 0x1bca0, 0x13430, 0x115f, 0x3164, 0x2800, 0xe000, 0x2014] {
            let c = char::from_u32(cp).unwrap();
            assert_eq!(printable(&c.to_string()), format!("\\u{{{cp:x}}}"), "{cp:x}");
        }
        assert_eq!(printable(r"\u{1b}"), r"\\u{1b}", "a typed escape never reads as a real one");
        assert_eq!(printable("café 日本 x9"), "café 日本 x9");
    }
}
