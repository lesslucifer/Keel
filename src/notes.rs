//! Versioned note schemas + `keel notes write <test|doc|cr>`.
//! Notes live in refs/notes/<kind> (same naming as Trawler), JSON, attached to the tip commit.
use crate::{git, git_raw, ok_bytes, ok_text, review, short, Config};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// Shared identity block: what was judged (tip over base), on which branch, by whom, when.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Identity {
    pub tip: String,
    pub base: String,
    pub branch: Option<String>,
    pub committer: String,
    /// unix seconds
    pub timestamp: u64,
}

impl Identity {
    /// An identity with every field empty: a placeholder the writer replaces before storing.
    pub(crate) fn blank() -> Identity {
        Identity { tip: String::new(), base: String::new(), branch: None, committer: String::new(), timestamp: 0 }
    }

    /// The JSON keys the identity block flattens into every note, from the type itself: whoever
    /// strips the identity from a note (the waiver code) never keeps its own copy of the list.
    pub(crate) fn keys() -> Vec<String> {
        serde_json::to_value(Identity::blank()).ok().and_then(|v| v.as_object().map(|m| m.keys().cloned().collect())).unwrap_or_default()
    }
}

/// How every fix line names the owner's waiver: the owner's own act, never an agent's.
pub(crate) const OWNER_WAIVES: &str =
    "the owner waives a review at sign-off (keel mr confirm ... --waive-review \"<reason>\", typed by the owner, never staged)";

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Layer {
    Counted { count: u32, covers: String, files: Vec<String>, languages: BTreeMap<String, u32> },
    Waived { waived: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TestNote {
    pub schema: String, // "test/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub unit: Option<Layer>,
    pub integration: Option<Layer>,
    pub e2e: Option<Layer>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DocNote {
    pub schema: String, // "doc/v1"
    #[serde(flatten)]
    pub id: Identity,
    /// One entry per path of `doc_paths` (UC-DOC-3).
    pub changes: Vec<DocChange>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DocChange {
    pub path: String,
    pub tier: String,
    /// Archived T3 docs only: the T1/T2 docs that absorbed it, or "none: <why>".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distilled_to: Option<DistilledTo>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum DistilledTo {
    Paths(Vec<String>),
    DeadEnd(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CrNote {
    pub schema: String, // "cr/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub verdict: String, // PASS | FAIL | WAIVED
    /// WAIVED only: why no review passed - the owner's ruling, or the review cap's reason (UC-REV-2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<String>,
    /// On WAIVED: the findings left open.
    pub blockers: u32,
    pub importants: u32,
    /// git blob hash of `git diff base tip -- <code_paths>`: binds the verdict to the code reviewed.
    pub code_diff_hash: String,
    /// Epic tips only: seams of the epic walk waived on the record (edited in by hand).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub walk_waivers: Vec<WalkWaiver>,
    /// The review schedule's fields (UC-REV-3), keel's own: typed here, so the writer and every reader
    /// read them one way. Declared before `extra`, which keeps only what no typed field took.
    #[serde(flatten)]
    pub schedule: ScheduleFields,
    /// Every other field the review skill records (`findings`, `refuted`, `nits`, `fleet`, ...): kept
    /// as read, so a note rewritten in place (a waiver at sign-off) never drops what a round recorded.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// What keel writes for the schedule: `round` and `scope` on a reviewed note; `fixes`, `round` (the
/// round it answers), `fix_diff_hash` and `answers` on a fix-PASS. A note whose field has another type,
/// or whose `fixes` is not a full commit name, does not parse: it is unreadable, never half-read, and a
/// note's text never reaches git as anything but a commit (code review round 1, B1).
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ScheduleFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "full_commit")]
    pub fixes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_diff_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<Answers>,
}

/// The findings a fix-PASS answers.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Answers {
    pub importants: u32,
    pub nits: usize,
}

fn full_commit<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let s = String::deserialize(d)?;
    if full_sha(&s) {
        Ok(Some(s))
    } else {
        Err(serde::de::Error::custom(format!("its fixes {s:?} is not a full commit name")))
    }
}

/// A full object name: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits.
pub(crate) fn full_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl CrNote {
    /// The waiver's reason (possibly blank, when hand-edited) iff the verdict is WAIVED.
    pub(crate) fn waiver(&self) -> Option<&str> {
        (self.verdict == "WAIVED").then(|| self.waiver.as_deref().unwrap_or(""))
    }

    /// `sha256:<hex>` of the whole waiver iff the verdict is WAIVED: what a sign-off accepts and signs,
    /// and what check 8 rebuilds - one rule for both. It covers the reason, the open counts, whether a
    /// review ran, each open finding and each walk waiver (owner, E-SEED3 open point 2), as one JSON
    /// text, so no reason can pass for another field. Anything else on the note is not signed.
    pub(crate) fn waiver_hash(&self) -> Option<String> {
        let reason = self.waiver()?;
        let walk: Vec<serde_json::Value> = self.walk_waivers.iter().map(|w| serde_json::json!({"seam": w.seam, "why": w.why})).collect();
        let signed = serde_json::json!({"waiver": "keel-waiver/v2", "reason": reason, "blockers": self.blockers, "importants": self.importants,
            "unreviewed": self.unreviewed(), "open_findings": self.open_findings(), "walk_waivers": walk});
        Some(sha256(signed.to_string().as_bytes()))
    }

    /// `sha256:<hex>` of the reason alone: what a keel before E-SEED3.W6 signed. Check 8 names that cause.
    pub(crate) fn reason_hash(&self) -> Option<String> {
        self.waiver().map(|w| sha256(w.as_bytes()))
    }

    /// A waiver written where no review ran on the tip: its counts of 0 mean nothing.
    pub(crate) fn unreviewed(&self) -> bool {
        self.extra.get("unreviewed") == Some(&serde_json::Value::Bool(true))
    }

    /// The round's verified findings left open: the review skill's `findings` entries with no `waived`
    /// reason in words (a blank, null or non-string `waived` waives nothing, as everywhere else).
    pub(crate) fn open_findings(&self) -> Vec<&serde_json::Value> {
        self.extra.get("findings").and_then(|f| f.as_array()).into_iter().flatten().filter(|f| !waived_in_words(f)).collect()
    }

    /// A fix-PASS's reviewed commit: the round it answers without a new one (UC-REV-3).
    pub(crate) fn fixes(&self) -> Option<&str> {
        self.schedule.fixes.as_deref()
    }
}

/// A finding or nit is waived only by a `waived` reason in words.
pub(crate) fn waived_in_words(f: &serde_json::Value) -> bool {
    f.get("waived").and_then(|w| w.as_str()).is_some_and(|w| !w.trim().is_empty())
}

/// `sha256:<hex>`: the algorithm is named in the data.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// The one rule for a waiver's reason, whoever writes it: trimmed, not blank, and never the next flag
/// (`--reason --blockers 2` names no reason).
pub(crate) fn waiver_reason(raw: Option<&str>) -> Result<String, String> {
    match raw.map(str::trim) {
        Some(r) if !r.is_empty() && !r.starts_with("--") => Ok(r.into()),
        _ => Err("a waiver needs its reason, in words (a following flag is not a reason)".into()),
    }
}

/// The one cr/v1 waiver writer, for the owner's sign-off and the review tool's cap alike, given the
/// tip's cr note as read (`existing`: `None` when there is none, `Err` when it cannot be read or
/// parsed). Never over a note it cannot
/// parse, nor over a PASS. Over a failed or waived round of this very range (same base and code hash)
/// the note is kept whole (counts, walk waivers, every field the review recorded) and only identity,
/// verdict, reason and binding change. A round recorded against another range is no review of this one: the waiver is written
/// `unreviewed`, that note kept under `replaced` as history only (whole but for its own `replaced`:
/// one level of history, never nested), and only its `walk_waivers`
/// (they name seams, which a later code change does not move) carried. With no note at all it is
/// `unreviewed` too, so its 0/0 never reads as a clean review. `binds`: whether a stored code hash binds
/// this range's code, the one binding rule (`code_binds`), so a round bound in git's old short form is
/// this range's round too (B29). Err = (what, fix).
pub(crate) fn waive(
    existing: Option<Result<CrNote, String>>,
    id: Identity,
    reason: Option<&str>,
    code_diff_hash: String,
    binds: &dyn Fn(&str) -> Result<bool, String>,
) -> Result<CrNote, (String, String)> {
    let t = short(&id.tip).to_string();
    let reason = waiver_reason(reason).map_err(|w| (w, "give the reason in words".to_string()))?;
    let existing = existing.transpose().map_err(|why| unreadable(&t, &why))?;
    let this_range = match &existing {
        Some(n) if n.verdict != "PASS" && n.id.base == id.base => binds(&n.code_diff_hash).map_err(|e| (e, "fix the repo state, then re-run".to_string()))?,
        _ => false,
    };
    let unreviewed = |replaced: Option<CrNote>| {
        let mut extra = serde_json::Map::new();
        extra.insert("unreviewed".into(), serde_json::Value::Bool(true));
        let walk_waivers =
            replaced.as_ref().map(|r| r.walk_waivers.iter().map(|w| WalkWaiver { seam: w.seam.clone(), why: w.why.clone() }).collect()).unwrap_or_default();
        if let Some(mut r) = replaced {
            // One level of history: a note that itself replaced another drops that, never nesting deeper.
            r.extra.remove("replaced");
            extra.insert("replaced".into(), serde_json::to_value(r).unwrap_or_default());
        }
        CrNote {
            schema: String::new(),
            id: Identity::blank(),
            verdict: String::new(),
            waiver: None,
            blockers: 0,
            importants: 0,
            code_diff_hash: String::new(),
            walk_waivers,
            schedule: ScheduleFields::default(),
            extra,
        }
    };
    let mut n =
        match existing {
            Some(n) if n.verdict == "PASS" => return Err((
                format!("the review of {t} is a PASS; a review that passed is never waived over"),
                "sign it as reviewed (keel mr confirm without --waive-review); if that PASS no longer binds this tip (checks 4-5 red), run a new review on it"
                    .into(),
            )),
            Some(n) if this_range => n,
            other => unreviewed(other),
        };
    n.schema = "cr/v1".into();
    n.id = id;
    n.verdict = "WAIVED".into();
    n.waiver = Some(reason);
    n.code_diff_hash = code_diff_hash;
    Ok(n)
}

/// The one refusal of a waiver over a cr note that cannot be read, whichever writer meets it.
pub(crate) fn unreadable(t: &str, why: &str) -> (String, String) {
    (
        format!("the cr note on {t} is unreadable ({why}); a waiver never overwrites a note it cannot read"),
        "repair that note in place, or run a new review on the tip, then waive again".to_string(),
    )
}

/// The `kind` note on `rev` as stored, unparsed: `Ok(None)` only when there is none (`notes list`
/// exits 1). Any other failure, of the listing or of reading the blob it names, is an error, never
/// "no note". Read through plumbing only - `notes list` names the note's blob, `cat-file blob` prints
/// its bytes - so no display config (`log.*`, `i18n.*`, `format.*`) can re-encode, decorate or drop it.
pub(crate) fn raw(repo: &Path, kind: &str, rev: &str) -> Result<Option<String>, String> {
    let list = ["notes", &format!("--ref={kind}"), "list", rev];
    let o = git_raw(repo, &list, None)?;
    if o.status.code() == Some(1) {
        return Ok(None);
    }
    let blob = ok_text(&list, o)?;
    let body = git_raw(repo, &["cat-file", "blob", &blob], None)?;
    if !body.status.success() {
        return Err(format!("the {kind} note on {rev} names blob {blob}, which git cannot print: {}", String::from_utf8_lossy(&body.stderr).trim()));
    }
    String::from_utf8(body.stdout).map(Some).map_err(|_| format!("the {kind} note on {rev} is not UTF-8"))
}

pub(crate) fn cr_raw(repo: &Path, rev: &str) -> Result<Option<String>, String> {
    raw(repo, "cr", rev)
}

/// The cr note on `rev`, if one reads and parses; the gate's check 1 reports a bad one.
pub(crate) fn cr_note(repo: &Path, rev: &str) -> Option<CrNote> {
    serde_json::from_str(&cr_raw(repo, rev).ok()??).ok()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WalkWaiver {
    pub seam: String,
    pub why: String,
}

/// The human sign-off (W4, `keel mr confirm`). Check 8 rebuilds the signed payload from the repo;
/// only `signer`, `report` and the signed `report_hash` are taken from here.
#[derive(Serialize, Deserialize, Debug)]
pub struct MrNote {
    pub schema: String, // "mr/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub level: String, // wu | epic
    pub signer: String,
    /// Informational: authority is re-derived from the target's committed .keel/mr-allowed at check time.
    pub tier: String,
    /// The attested result report; absent when `[mr] report` asked for none and none was attested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// `sha256:<hex>` of the attested report's bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_hash: Option<String>,
    /// `sha256:<hex>` of the waiver the signer accepted (`CrNote::waiver_hash`: its reason, counts, open
    /// findings, walk waivers and review state); absent when the review was not waived. Check 8
    /// rebuilds it from the tip's current cr note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver_hash: Option<String>,
    /// `sha256:<hex>` of the full `git diff base tip`.
    pub change_hash: String,
    /// Armored SSH SIGNATURE (namespace keel-mr), key-backed signers only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

/// The merge gate's receipt, written on green.
#[derive(Serialize, Deserialize, Debug)]
pub struct DodNote {
    pub schema: String, // "dod/v1"
    #[serde(flatten)]
    pub id: Identity,
    pub level: String,
    pub checks: Vec<String>,
    /// git blob hash of the full `git diff base tip`.
    pub diff_hash: String,
    /// The report the verified mr/v1 sign-off attested, copied from it; absent when it attested none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_hash: Option<String>,
    /// "WAIVED" when the tip's own review was waived; absent after a reviewed PASS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Judgment {
    unit: Option<JLayer>,
    integration: Option<JLayer>,
    e2e: Option<JLayer>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JLayer {
    #[serde(default)]
    files: Vec<String>,
    covers: Option<String>,
    waived: Option<String>,
}

fn refuse<T>(what: String, fix: &str) -> Result<T, String> {
    Err(format!("keel notes: refused - {what}\n  fix -> {fix}"))
}

struct Range {
    id: Identity,
    names: Vec<String>,
    target: String,
}

/// The one reader of the `--target R` form, so the writer and the gate cannot disagree on it.
pub(crate) fn target_flag<'a>(args: &'a [&'a str]) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == "--target").map(|w| w[1])
}

/// The epic branch a `<wu_prefix><e>/…` branch name names. The one branch-to-target derivation.
pub(crate) fn epic_of(cfg: &Config, branch: &str) -> Option<String> {
    let b = &cfg.branches;
    branch
        .strip_prefix(b.wu_prefix.as_str())
        .and_then(|r| r.split_once('/'))
        .filter(|(epic, _)| !epic.is_empty())
        .map(|(epic, _)| format!("{}{epic}", b.epic_prefix))
}

/// The epic branch `<epic_prefix><e>` names, if `name` is one.
pub(crate) fn epic_branch(cfg: &Config, name: &str) -> Option<String> {
    let b = &cfg.branches;
    name.strip_prefix(b.epic_prefix.as_str()).filter(|epic| !epic.is_empty()).map(|_| name.to_string())
}

/// The ref a range is measured against, shared by the writer and the gate so both judge one range:
/// `--target R`, else a `<wu_prefix><e>/..` tip's `<epic_prefix><e>`, else main. A tip that names no
/// branch (a bare commit id) resolves through `target_from_holders` first.
pub(crate) fn target(cfg: &Config, tip_ref: &str, args: &[&str]) -> String {
    match target_flag(args) {
        Some(t) => t.to_string(),
        None => epic_of(cfg, tip_ref).unwrap_or_else(|| cfg.branches.main.clone()),
    }
}

/// The target a WU tip that names no local branch is measured against: the epic its `wu/` holders
/// name, else the one `epic/` branch that holds it. Anything else is a refusal, never main by guess.
pub(crate) fn target_from_holders(cfg: &Config, holders: &[String]) -> Result<String, String> {
    let mut epics: Vec<String> = holders.iter().filter_map(|h| epic_of(cfg, h)).collect();
    epics.sort();
    epics.dedup();
    match epics.as_slice() {
        [epic] => return Ok(epic.clone()),
        [] => {}
        _ => return Err(format!("its wu branches name more than one epic: {}", epics.join(", "))),
    }
    let mut epic_branches: Vec<String> = holders.iter().filter_map(|h| epic_branch(cfg, h)).collect();
    epic_branches.sort();
    epic_branches.dedup();
    match epic_branches.as_slice() {
        [epic] => Ok(epic.clone()),
        [] => Err("no wu or epic branch holds it, and main is not a guess this gate makes for a WU".into()),
        _ => Err(format!("more than one epic branch holds it: {}", epic_branches.join(", "))),
    }
}

/// Paths from a `-z` listing: raw bytes, never C-quoted (a plain listing quotes non-ASCII paths).
pub(crate) fn zpaths(out: &str) -> Vec<String> {
    out.split('\0').map(|s| s.trim_matches('\n')).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Paths changed base..tip within `pathspecs` (none = all). No rename pairing: moving a file out of
/// a path is a change to that path. `--end-of-options`: a revision is never read as an option, even
/// one taken from a note.
pub(crate) fn names(repo: &Path, base: &str, tip: &str, pathspecs: &[String]) -> Result<Vec<String>, String> {
    let mut a = vec!["diff", "--name-only", "--no-renames", "-z", "--end-of-options", base, tip, "--"];
    a.extend(pathspecs.iter().map(String::as_str));
    Ok(zpaths(&git(repo, &a, &[])?))
}

/// One diff of the branch's own changes: what commit `to` changed against `from`.
#[derive(Debug, PartialEq)]
pub(crate) struct OwnDiff {
    pub from: String,
    pub to: String,
}

/// The branch's own changes after `since` up to `tip`, one diff per first-parent commit, oldest first (owner ruling
/// 2026-11-13, epic code review round 3): a commit with one parent against that parent; a merge of
/// two against git's own automatic merge of them (`git merge-tree --write-tree`, conflict markers and
/// all), so only what the merge commit changed by hand counts, never what its second parent brought;
/// an octopus merge whole, against its first parent (the gate refuses one anyway). Needs git 2.40 or
/// later (`merge-tree --write-tree`, `--attr-source`); with an older git it refuses rather than guess.
pub(crate) fn own_diffs(repo: &Path, since: &str, tip: &str) -> Result<Vec<OwnDiff>, String> {
    let line = git(repo, &["rev-list", "--reverse", "--first-parent", "--parents", "--end-of-options", &format!("{since}..{tip}")], &[])?;
    let mut diffs = vec![];
    for l in line.lines() {
        let ids: Vec<&str> = l.split(' ').collect();
        let from = match ids[1..] {
            [p1, p2] => auto_merge(repo, p1, p2)?,
            [p1, ..] => p1.to_string(),
            [] => return Err(format!("keel: {} has no parent, so the branch's own changes cannot be told", short(ids[0]))),
        };
        diffs.push(OwnDiff { from, to: ids[0].to_string() });
    }
    Ok(diffs)
}

/// The tree git's automatic merge of `p1` and `p2` gives: `merge-tree --write-tree` prints it first,
/// exiting 0 when clean and 1 with conflicts (the tree then holds the conflict markers). Attributes
/// come from `p1`'s tree, the checkout the real merge was made on, never from the running checkout's
/// worktree (epic code review round 4, B-1); `$GIT_DIR/info/attributes` applies to both alike.
fn auto_merge(repo: &Path, p1: &str, p2: &str) -> Result<String, String> {
    let attrs = format!("--attr-source={p1}");
    let args = [attrs.as_str(), "merge-tree", "--write-tree", "--end-of-options", p1, p2];
    let o = git_raw(repo, &args, None)?;
    let tree = String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or_default().to_string();
    match o.status.code() {
        Some(0 | 1) if full_sha(&tree) => Ok(tree),
        _ => Err(format!(
            "keel: git merge-tree --write-tree failed on the merge of {} and {} ({}); telling a merge's own changes needs git 2.40 or later",
            short(p1),
            short(p2),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
    }
}

/// The argv of one own diff, in output `format` (`--name-only -z` for the check, `-p` for reviewers):
/// one argv, so the files the check lists are the files of the diff reviewers read.
fn own_diff_argv(d: &OwnDiff, format: &[&str], pathspecs: &[String]) -> Vec<String> {
    let mut a: Vec<String> = ["diff-tree", "-r", "--no-renames"].into_iter().chain(format.iter().copied()).map(String::from).collect();
    a.extend(["--end-of-options".into(), d.from.clone(), d.to.clone(), "--".into()]);
    a.extend(pathspecs.iter().cloned());
    a
}

/// The paths within `pathspecs` that `diffs` change: their union, sorted, as the raw bytes git prints
/// under `-z` (never C-quoted, never decoded), so two paths stay two whatever their encoding. One
/// `diff-tree` per diff (backlog B16). `shown` turns them into text for the signer.
pub(crate) fn diff_names(repo: &Path, diffs: &[OwnDiff], pathspecs: &[String]) -> Result<Vec<Vec<u8>>, String> {
    let mut outputs = vec![];
    for d in diffs {
        let argv = own_diff_argv(d, &["--name-only", "-z"], pathspecs);
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        outputs.push(ok_bytes(&argv, git_raw(repo, &argv, None)?)?);
    }
    Ok(union_of_z(&outputs))
}

/// The union of `-z` path listings, sorted, each path its raw bytes.
fn union_of_z(outputs: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = outputs.iter().flat_map(|o| o.split(|&b| b == 0)).filter(|p| !p.is_empty()).map(<[u8]>::to_vec).collect();
    names.sort();
    names.dedup();
    names
}

/// The one `changed:` text, for preflight and the sign-off history alike: the paths the branch's own
/// changes in `range` touch, "nothing", or "unknown (why)" when git failed or a path is not UTF-8
/// (the decision never depends on the text: `diff_names`' bytes do).
pub(crate) fn changed_text(names: Result<Vec<Vec<u8>>, String>, range: &str) -> String {
    let shown = names.and_then(|n| {
        n.into_iter()
            .map(String::from_utf8)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("a path is not UTF-8; it is among the branch's own changes in {range}"))
    });
    match shown {
        Ok(n) if n.is_empty() => "nothing".into(),
        Ok(n) => n.join(", "),
        Err(why) => format!("unknown ({why})"),
    }
}

/// Paths within `pathspecs` the branch itself changed after `since` up to `tip` (`own_diffs`).
pub(crate) fn own_names(repo: &Path, since: &str, tip: &str, pathspecs: &[String]) -> Result<Vec<Vec<u8>>, String> {
    diff_names(repo, &own_diffs(repo, since, tip)?, pathspecs)
}

/// The commands that print `diffs` as patches, shell-quoted, joined into one line for reviewers:
/// exactly `diff_names`' argv with `-p` in place of `--name-only -z`.
pub(crate) fn own_diff_commands(diffs: &[OwnDiff], pathspecs: &[String]) -> String {
    let sh = |argv: Vec<String>| std::iter::once("git".to_string()).chain(argv.iter().map(|a| shell_quote(a))).collect::<Vec<_>>().join(" ");
    diffs.iter().map(|d| sh(own_diff_argv(d, &["-p"], pathspecs))).collect::<Vec<_>>().join(" ; ")
}

/// An argument the shell passes through unchanged: as is when it holds only [A-Za-z0-9/._-], else
/// single-quoted (no expansion of `~`, `=`, `^` or anything else).
fn shell_quote(a: &str) -> String {
    if !a.is_empty() && a.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)) {
        a.to_string()
    } else {
        format!("'{}'", a.replace('\'', "'\\''"))
    }
}

/// A commit and its note's text as stored, or why git cannot print it.
pub(crate) type StoredNote = (String, Result<String, String>);

/// The `cr` notes on the first-parent commits of `base..tip`, newest first, as stored: each commit and
/// its note's text, or why git cannot print it; `skip`: a commit whose note the caller already read.
/// Three git processes whatever the length of the line:
/// `rev-list` names the commits, `notes list` every note's blob, one `cat-file --batch` prints the
/// line's blobs. Plumbing only, so no display config changes a note.
pub(crate) fn cr_line(repo: &Path, base: &str, tip: &str, skip: Option<&str>) -> Result<Vec<StoredNote>, String> {
    let commits = git(repo, &["rev-list", "--first-parent", "--end-of-options", &format!("{base}..{tip}")], &[])?;
    let listed = git(repo, &["notes", "--ref=cr", "list"], &[])?;
    let blobs: std::collections::HashMap<&str, &str> = listed.lines().filter_map(|l| l.split_once(' ')).map(|(blob, commit)| (commit, blob)).collect();
    let on_line: Vec<(&str, &str)> = commits.lines().filter(|c| Some(*c) != skip).filter_map(|c| blobs.get(c).map(|b| (c, *b))).collect();
    if on_line.is_empty() {
        return Ok(vec![]);
    }
    let input: String = on_line.iter().map(|(_, b)| format!("{b}\n")).collect();
    let args = ["cat-file", "--batch"];
    let out = ok_bytes(&args, git_raw(repo, &args, Some(input.as_bytes()))?)?;
    let bodies = parse_batch(&out, on_line.len());
    Ok(on_line.into_iter().zip(bodies).map(|((c, _), body)| (c.to_string(), body)).collect())
}

/// `git cat-file --batch` output, one result per object asked for: its bytes as text, or why not.
fn parse_batch(mut out: &[u8], n: usize) -> Vec<Result<String, String>> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let Some(nl) = out.iter().position(|&b| b == b'\n') else {
            v.push(Err("git printed nothing for it".into()));
            continue;
        };
        let header = String::from_utf8_lossy(&out[..nl]).into_owned();
        out = &out[nl + 1..];
        let fields: Vec<&str> = header.split(' ').collect();
        let size = match fields[..] {
            [_, _, size] => size.parse::<usize>().ok().filter(|s| out.len() > *s),
            _ => None,
        };
        match (&fields[..], size) {
            ([_, "blob", _], Some(s)) => v.push(String::from_utf8(out[..s].to_vec()).map_err(|_| "its blob is not UTF-8".into())),
            ([_, kind, _], Some(_)) => v.push(Err(format!("it names a {kind}, not a blob"))),
            ([_, "missing"], _) => v.push(Err("its blob is missing from the object store".into())),
            _ => {
                v.push(Err(format!("git printed '{header}' for it")));
                out = &[];
                continue;
            }
        }
        if let Some(s) = size {
            out = &out[s + 1..];
        }
    }
    v
}

pub(crate) fn identity(repo: &Path, tip: String, base: String, branch: Option<String>) -> Result<Identity, String> {
    // "Name <email> 1700000000 +0100"
    let ident = git(repo, &["var", "GIT_COMMITTER_IDENT"], &[])?;
    let (who, ts) = ident.split_once("> ").ok_or("keel: cannot parse committer identity")?;
    let timestamp = ts.split(' ').next().and_then(|t| t.parse().ok()).ok_or("keel: bad committer timestamp")?;
    Ok(Identity { tip, base, branch, committer: format!("{who}>"), timestamp })
}

fn range(repo: &Path, cfg: &Config, args: &[&str]) -> Result<Range, String> {
    let branch = git(repo, &["symbolic-ref", "--short", "-q", "HEAD"], &[]).ok();
    let target = target(cfg, branch.as_deref().unwrap_or("HEAD"), args);
    let tip = git(repo, &["rev-parse", "HEAD"], &[])?;
    let base = git(repo, &["merge-base", &target, "HEAD"], &[])?;
    if base == tip {
        return refuse(
            format!("HEAD is already on '{target}'; the range since the merge-base is empty"),
            "write notes from the work branch, with commits not yet merged (or pass --target <ref>)",
        );
    }
    let names = names(repo, &base, &tip, &[])?;
    Ok(Range { id: identity(repo, tip, base, branch)?, names, target })
}

/// UC-GATE-5 docs/infra-only: the range changes nothing under `code_paths` - git pathspecs, the very
/// diff cr binds (empty = every path is code) - and, when `infra_paths` is set, every changed path
/// matches one of its globs.
pub(crate) fn exempt(repo: &Path, cfg: &Config, base: &str, tip: &str, names: &[String]) -> Result<bool, String> {
    Ok(self::names(repo, base, tip, &cfg.code_paths)?.is_empty() && cfg.infra_paths.as_ref().is_none_or(|g| names.iter().all(|n| g.iter().any(|p| glob(p, n)))))
}

/// `*` = within one segment, `**` = any depth, `?` = one char.
pub(crate) fn glob(pattern: &str, path: &str) -> bool {
    glob_re(pattern).is_some_and(|r| r.is_match(path))
}

/// `glob`'s pattern compiled once, for a caller that matches it against many paths.
pub(crate) fn glob_re(pattern: &str) -> Option<regex::Regex> {
    let mut re = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                re.push_str(".*");
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    regex::Regex::new(&re).ok()
}

/// T0-T3 by `docs.tiers` globs (default: Trawler's layout under docs.root).
/// T3 is tried first: a broad T1 glob must never hide a working doc from the archive check.
pub(crate) fn tier_of(cfg: &Config, path: &str) -> Option<&'static str> {
    let r = format!("{}/", cfg.docs.root.trim_end_matches('/'));
    let default;
    let t = match &cfg.docs.tiers {
        Some(t) => t,
        None => {
            default = crate::Tiers {
                t0: vec!["AGENTS.md".into(), "CLAUDE.md".into()],
                t1: vec![format!("{r}*")],
                t2: ["use-cases", "decisions", "backlog"].map(|d| format!("{r}{d}/**")).to_vec(),
                t3: vec![format!("{r}work/**"), format!("{r}.archive/**")],
            };
            &default
        }
    };
    let tier =
        [("T3", &t.t3), ("T2", &t.t2), ("T1", &t.t1), ("T0", &t.t0)].into_iter().find(|(_, globs)| globs.iter().any(|g| glob(g, path))).map(|(name, _)| name);
    tier
}

pub(crate) fn at_tip(repo: &Path, tip: &str, path: &str) -> bool {
    git(repo, &["cat-file", "-e", &format!("{tip}:{path}")], &[]).is_ok()
}

fn archived(repo: &Path, cfg: &Config, tip: &str, path: &str) -> bool {
    tier_of(cfg, path) == Some("T3") && !at_tip(repo, tip, path)
}

/// The docs doc/v1 must account for: changed paths under docs.root or in T0, plus T3
/// docs deleted anywhere in the range - one created and archived inside it nets out
/// of the endpoint diff, but its lineage is still owed. Shared by the writer and check 6.
pub(crate) fn doc_paths(repo: &Path, cfg: &Config, base: &str, tip: &str, names: &[String]) -> Result<Vec<String>, String> {
    let mut v: Vec<String> = names.iter().filter(|n| n.starts_with(&cfg.docs.root) || tier_of(cfg, n) == Some("T0")).cloned().collect();
    // Plumbing (rev-list into diff-tree), so no display config can add a line: git log under
    // log.showSignature prints signature text into the very listing read as paths.
    let commits = git(repo, &["rev-list", &format!("{base}..{tip}")], &[])?;
    let args = ["diff-tree", "--stdin", "-r", "--no-renames", "-z", "--diff-filter=D", "--name-only", "--no-commit-id"];
    let gone = ok_text(&args, git_raw(repo, &args, Some(format!("{commits}\n").as_bytes()))?)?;
    for p in zpaths(&gone) {
        if !v.contains(&p) && archived(repo, cfg, tip, &p) {
            v.push(p);
        }
    }
    Ok(v)
}

pub(crate) fn archives_t3(repo: &Path, cfg: &Config, tip: &str, docs: &[String]) -> bool {
    docs.iter().any(|p| archived(repo, cfg, tip, p))
}

/// The other file of the doc a path names, by name (`verify_doc::partner_named`, docs.root's
/// `in_docs`): the data file for a page under docs.root, the page for a data file; `None` for a page
/// outside docs.root or any other path.
pub(crate) fn partner(path: &str, in_docs: impl Fn(&Path) -> bool) -> Option<String> {
    crate::verify_doc::partner_named(Path::new(path), in_docs).map(|q| q.to_string_lossy().into_owned())
}

/// A doc archived by half (E-DOCSPLIT.W3): the other file of the archived `path`'s pair (`partner`)
/// when it is still at the tip (`at_tip`), named so it can be git rm'd too; `None` when the pair went
/// whole, or the path has no partner.
pub(crate) fn half_archived(path: &str, in_docs: impl Fn(&Path) -> bool, at_tip: impl Fn(&str) -> bool) -> Option<String> {
    partner(path, in_docs).filter(|q| at_tip(q))
}

/// The `--distill` value an archived doc takes (DEC-69, E-DOCSPLIT.W3): its own, else the one its
/// pair's other file (`partner`) names when that file is archived too (`gone`), so one line covers a
/// page and its data file; each keeps its own when both are named.
fn distill_of<'a>(path: &str, in_docs: impl Fn(&Path) -> bool, gone: &[String], distill: &BTreeMap<&str, &'a str>) -> Option<&'a str> {
    distill.get(path).or_else(|| partner(path, in_docs).filter(|q| gone.contains(q)).and_then(|q| distill.get(q.as_str()))).copied()
}

const REWRITE: &str = "keel notes write doc   (on the tip)";

/// Check 6 and the doc writer's refusals: (what, fix) per problem; empty = green.
pub(crate) fn doc_problems(repo: &Path, cfg: &Config, tip: &str, docs: &[String], changes: &[DocChange]) -> Vec<(String, String)> {
    let mut out = vec![];
    for p in docs.iter().filter(|p| !changes.iter().any(|c| &c.path == *p)) {
        out.push((format!("{p} changed in the range but is not in doc/v1"), REWRITE.into()));
    }
    for c in changes {
        let p = &c.path;
        if !docs.contains(p) {
            out.push((format!("doc/v1 lists {p} but the range does not touch it"), REWRITE.into()));
            continue;
        }
        let Some(tier) = tier_of(cfg, p) else {
            out.push((format!("{p} matches no docs.tiers pattern"), "add a glob for it under [docs.tiers] in .keel/config.toml, or move it".into()));
            continue;
        };
        if c.tier != tier {
            out.push((format!("{p} is tier {tier} by docs.tiers, doc/v1 says {}", c.tier), REWRITE.into()));
        }
        if tier != "T3" || at_tip(repo, tip, p) {
            continue;
        }
        if let Some(left) = half_archived(p, crate::verify_doc::under_root(&cfg.docs.root), |q| at_tip(repo, tip, q)) {
            out.push((
                format!("{p} is archived but {left}, the other half of its doc, is still at the tip"),
                format!("git rm {left} too, in the archive commit itself (git commit --amend, before its notes): a page and its data file are one doc, archived together (DEC-69); a later commit leaves this one red in the epic walk"),
            ));
        }
        let why = match &c.distilled_to {
            Some(DistilledTo::DeadEnd(s)) if s.strip_prefix("none:").is_some_and(|w| !w.trim().is_empty()) => None,
            Some(DistilledTo::Paths(ts)) if !ts.is_empty() => ts
                .iter()
                .find(|t| !matches!(tier_of(cfg, t), Some("T1" | "T2")) || !at_tip(repo, tip, t))
                .map(|t| format!("with distilled_to {t}, which is not a T1/T2 doc tracked at the tip")),
            _ => Some("without distillation lineage (distilled_to)".into()),
        };
        if let Some(why) = why {
            out.push((
                format!("{p} is archived {why}"),
                format!("distill it into a T1/T2 doc, then keel notes write doc --distill {p}=<doc>[,<doc>]   (or =none: <why nothing survives>)"),
            ));
        }
    }
    out
}

/// One function for the writer (W2) and check 5 (W3), so the two can never disagree.
pub(crate) fn code_diff_hash(repo: &Path, cfg: &Config, base: &str, tip: &str) -> Result<String, String> {
    diff_hash(repo, base, tip, &cfg.code_paths)
}

/// Check 5's and the waiver's side of `code_diff_hash`: whether a stored cr hash binds `base..tip`'s code (`binds`).
pub(crate) fn code_binds(repo: &Path, cfg: &Config, base: &str, tip: &str, stored: &str) -> Result<bool, String> {
    binds(repo, base, tip, &cfg.code_paths, stored, &|text| hash_blob(repo, text))
}

/// git blob hash of `git diff base tip -- <pathspecs>` (none = the full diff), independent of the
/// local diff config and of the store's size so every machine hashes the same bytes (cr and dod bindings).
pub(crate) fn diff_hash(repo: &Path, base: &str, tip: &str, pathspecs: &[String]) -> Result<String, String> {
    hash_blob(repo, &diff_text(repo, base, tip, pathspecs)?)
}

/// The bytes every diff binding hashes (cr and dod as a git blob, mr as SHA-256): `git diff base tip -- <pathspecs>`,
/// local diff config off, full object names in the index lines (`--full-index`), so the text never depends on how
/// many objects the store holds (B29).
pub(crate) fn diff_text(repo: &Path, base: &str, tip: &str, pathspecs: &[String]) -> Result<String, String> {
    diff_text_as(repo, base, tip, pathspecs, None)
}

/// Git's short-hash lengths a binding written before E-APPLIST.W14 may hash (B29): a keel before it
/// ran `git diff` with git's automatic abbreviation, whose length grows with the store, one character
/// per fourfold growth. 7 is git's floor for it; 12 covers stores up to 2^24 (16.7M) objects.
pub(crate) const LEGACY_ABBREV: std::ops::RangeInclusive<usize> = 7..=12;

/// `diff_text`'s argv for one form: `None` the stable one (`--full-index`), `Some(n)` git's short form of `n`
/// characters (`--abbrev=n`), which only a binding written before E-APPLIST.W14 hashes.
fn diff_argv(base: &str, tip: &str, pathspecs: &[String], abbrev: Option<usize>) -> Vec<String> {
    let ids = abbrev.map_or_else(|| "--full-index".to_string(), |n| format!("--abbrev={n}"));
    let mut a: Vec<String> = ["diff", "--no-color", "--no-ext-diff", "--no-textconv"].map(String::from).into();
    a.extend([ids, base.to_string(), tip.to_string(), "--".to_string()]);
    a.extend(pathspecs.iter().cloned());
    a
}

fn diff_text_as(repo: &Path, base: &str, tip: &str, pathspecs: &[String], abbrev: Option<usize>) -> Result<String, String> {
    let a = diff_argv(base, tip, pathspecs, abbrev);
    git(repo, &a.iter().map(String::as_str).collect::<Vec<_>>(), &[])
}

/// The one binding rule, for cr (check 5 and the owner's waiver) and mr (check 8) alike: a stored hash
/// binds `base..tip -- <pathspecs>` when `hash` of the diff's stable text gives it, or of git's short form
/// at a length in `LEGACY_ABBREV` (a note written before E-APPLIST.W14, B29). Any other diff fails.
pub(crate) fn binds(
    repo: &Path,
    base: &str,
    tip: &str,
    pathspecs: &[String],
    stored: &str,
    hash: &dyn Fn(&str) -> Result<String, String>,
) -> Result<bool, String> {
    bound(stored, |abbrev| hash(&diff_text_as(repo, base, tip, pathspecs, abbrev)?))
}

/// `binds`'s order, git aside: the stable form first, then each legacy length, stopping at the first
/// match, so a binding a current keel wrote costs one diff.
fn bound(stored: &str, mut form: impl FnMut(Option<usize>) -> Result<String, String>) -> Result<bool, String> {
    for abbrev in std::iter::once(None).chain(LEGACY_ABBREV.map(Some)) {
        if form(abbrev)? == stored {
            return Ok(true);
        }
    }
    Ok(false)
}

fn count_tests(adapter: &str, body: &str, pattern: Option<&str>) -> Result<u32, String> {
    let starts = |forms: &[&str]| body.lines().filter(|l| forms.iter().any(|f| l.trim_start().starts_with(f))).count() as u32;
    Ok(match adapter {
        // ponytail: line-prefix rules; a declaration written another way is not counted (use the regex adapter)
        "rust" => starts(&["#[test]", "#[tokio::test"]),
        "ts" => starts(&["it(", "test(", "it.each(", "test.each("]),
        "python" => starts(&["def test_", "async def test_"]),
        "go" => starts(&["func Test"]),
        "regex" => {
            let p = pattern.ok_or("keel: tests.adapter = \"regex\" needs tests.pattern")?;
            let re = regex::Regex::new(p).map_err(|e| format!("keel: bad tests.pattern: {e}"))?;
            re.find_iter(body).count() as u32
        }
        other => return Err(format!("keel: unknown tests.adapter '{other}' (rust|ts|python|go|regex)")),
    })
}

fn adapter_for(cfg: &Config, file: &str) -> Result<String, String> {
    if let Some(a) = &cfg.tests.adapter {
        return Ok(a.clone());
    }
    match file.rsplit('.').next().unwrap_or("") {
        "rs" => Ok("rust".into()),
        "ts" | "tsx" | "js" | "jsx" => Ok("ts".into()),
        "py" => Ok("python".into()),
        "go" => Ok("go".into()),
        _ if cfg.tests.pattern.is_some() => Ok("regex".into()),
        _ => refuse(
            format!("no test counter for {file}; counting it as zero would be a false note"),
            "set tests.pattern (and optionally tests.adapter) in .keel/config.toml",
        ),
    }
}

fn layer(repo: &Path, cfg: &Config, r: &Range, name: &str, j: &JLayer) -> Result<Layer, String> {
    let waived = j.waived.as_deref().map(str::trim).unwrap_or("");
    if !waived.is_empty() {
        if !j.files.is_empty() {
            return refuse(format!("{name} has both files and a waiver"), "give files or waived, not both");
        }
        return Ok(Layer::Waived { waived: waived.into() });
    }
    let covers = j.covers.as_deref().map(str::trim).unwrap_or("");
    if j.files.is_empty() || covers.is_empty() {
        return refuse(
            format!("{name} needs `files` and a `covers` sentence, or a `waived` reason"),
            "list the test files that hold the tests and say what they establish",
        );
    }
    let (mut count, mut languages) = (0, BTreeMap::new());
    for f in &j.files {
        if !r.names.contains(f) {
            return refuse(format!("{name} claims {f}, but the range does not touch it"), "name only test files changed in this range, or waive the layer");
        }
        let body = git(repo, &["show", &format!("{}:{f}", r.id.tip)], &[])
            .or_else(|_| refuse(format!("{f} is not a file at the tip"), "name a file that exists at HEAD"))?;
        let a = adapter_for(cfg, f)?;
        let n = count_tests(&a, &body, cfg.tests.pattern.as_deref())?;
        count += n;
        *languages.entry(a).or_insert(0) += n;
    }
    if count == 0 {
        return refuse(format!("{name} claims tests but its files declare none"), "name files with tests, or waive the layer");
    }
    Ok(Layer::Counted { count, covers: covers.into(), files: j.files.clone(), languages })
}

pub(crate) fn store<T: Serialize>(repo: &Path, kind: &str, tip: &str, note: &T) -> Result<(), String> {
    let json = serde_json::to_string(note).map_err(|e| e.to_string())?;
    git(repo, &["notes", &format!("--ref={kind}"), "add", "-f", "-m", &json, tip], &[])?;
    println!("wrote refs/notes/{kind} on {}", tip.get(..8).unwrap_or(tip));
    Ok(())
}

/// One note's move in `store_all`'s transaction: its real ref, the commit it moves to, the commit it
/// was read at (`None`: absent), and the pending ref that carried the write.
struct Move {
    real: String,
    new: String,
    old: Option<String>,
    pending: String,
}

/// `git update-ref --stdin`'s input for `moves`: one transaction that moves each real ref only from
/// the value it was read at (created only if still absent) and deletes each pending ref.
fn transaction(moves: &[Move]) -> String {
    let mut t = String::from("start\n");
    for m in moves {
        match &m.old {
            Some(old) => t.push_str(&format!("update {} {} {old}\n", m.real, m.new)),
            None => t.push_str(&format!("create {} {}\n", m.real, m.new)),
        }
        t.push_str(&format!("delete {} {}\n", m.pending, m.new));
    }
    t.push_str("commit\n");
    t
}

/// Several notes stored all or none (B17: the owner's waiver never lands without the sign-off).
/// Each `(kind, tip, note)` is written by `git notes` to a pending ref that starts at its kind's
/// current commit - git refuses notes refs outside `refs/notes/` - then one ref transaction moves
/// every real ref and deletes the pending refs. On any failure the pending refs are deleted and no
/// real ref has moved. Kinds are distinct.
pub(crate) fn store_all(repo: &Path, notes: &[(&str, &str, String)]) -> Result<(), String> {
    let mut moves = vec![];
    let result = (|| {
        for (kind, tip, json) in notes {
            let real = format!("refs/notes/{kind}");
            let pending = format!("refs/notes/keel-pending-{kind}-{}", std::process::id());
            let old = git(repo, &["rev-parse", "--verify", "-q", &real], &[]).ok();
            moves.push(Move { real, new: String::new(), old: old.clone(), pending: pending.clone() });
            match &old {
                Some(old) => git(repo, &["update-ref", &pending, old], &[])?,
                None => git(repo, &["update-ref", "-d", &pending], &[])?,
            };
            git(repo, &["notes", &format!("--ref={pending}"), "add", "-f", "-m", json, tip], &[])?;
            let new = git(repo, &["rev-parse", "--verify", "-q", &pending], &[])?;
            if let Some(m) = moves.last_mut() {
                m.new = new;
            }
        }
        let input = transaction(&moves);
        let args = ["update-ref", "--stdin"];
        ok_text(&args, git_raw(repo, &args, Some(input.as_bytes()))?).map(|_| ())
    })();
    if result.is_err() {
        for m in &moves {
            let _ = git(repo, &["update-ref", "-d", &m.pending], &[]);
        }
    }
    result?;
    for (kind, tip, _) in notes {
        println!("wrote refs/notes/{kind} on {}", tip.get(..8).unwrap_or(tip));
    }
    Ok(())
}

pub(crate) fn hash_blob(repo: &Path, data: &str) -> Result<String, String> {
    let args = ["hash-object", "--stdin"];
    let id = ok_text(&args, git_raw(repo, &args, Some(data.as_bytes()))?)?;
    if id.is_empty() {
        return Err("keel: git hash-object --stdin failed".into());
    }
    Ok(id)
}

pub(crate) const NOTES_USAGE: &str = "usage: keel notes write <test|doc|cr>";
pub(crate) const TEST_USAGE: &str = "usage: keel notes write test [--target R] < judgment.json";
pub(crate) const DOC_USAGE: &str = "usage: keel notes write doc [--target R] [--distill <t3-path>=<doc>[,<doc>] | <t3-path>=none: <why>]...";
pub(crate) const CR_USAGE: &str = "usage: keel notes write cr --verdict PASS|FAIL [--blockers N] [--importants N] [--findings <json>] [--refuted <json>] [--nits <json>] [--fleet <json>] [--target R]\n       keel notes write cr --verdict PASS --fixes <reviewed commit>   (a fix-PASS: no new round)\n       keel notes write cr --verdict WAIVED   (the review cap: the tool writes the reason)";

pub(crate) const TARGET: &str = "--target";

/// Each kind's flags, every one taking the next word as its value.
const TEST_FLAGS: &[&str] = &[TARGET];
const DOC_FLAGS: &[&str] = &[TARGET, "--distill"];
const CR_FLAGS: &[&str] = &["--verdict", "--blockers", "--importants", "--findings", "--refuted", "--nits", "--fleet", "--fixes", TARGET];

/// The one strict reader of a keel flag list (K2), for the notes writer and the gate alike: `valued`
/// flags take the next word, `bare` ones none, and only `repeatable` ones may come twice. A value is
/// never one of the known flags, and a `--target` value never starts with `-` (no ref does): so the
/// flag-by-flag readers (`target_flag`, `write_cr`'s, the `--distill` filter, `cr_lists`), which take
/// the word after a flag's first appearance, read every argv this accepts as it pairs them (review
/// round 1, I2). `Some(what is wrong)`: an unknown argument, a stray word, a repeat, a missing value.
pub(crate) fn args_problem(args: &[&str], valued: &[&str], bare: &[&str], repeatable: &[&str]) -> Option<String> {
    let mut seen = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        let takes_value = match (valued.contains(&a), bare.contains(&a)) {
            (true, _) => true,
            (false, true) => false,
            (false, false) => return Some(format!("unknown argument '{a}'")),
        };
        if seen.contains(&a) && !repeatable.contains(&a) {
            return Some(format!("{a} given twice"));
        }
        if takes_value {
            match args.get(i + 1) {
                Some(v) if !valued.contains(v) && !bare.contains(v) && !(a == TARGET && v.starts_with('-')) => i += 1,
                _ if a == TARGET => return Some("--target needs a value: --target <ref>".into()),
                _ => return Some(format!("{a} needs a value")),
            }
        }
        seen.push(a);
        i += 1;
    }
    None
}

/// `notes write`'s arguments checked before the range is computed or stdin read (K2), so a typo never
/// writes a note. `cr` with `--reason` is left to `cr_verdict`'s own refusal (B18).
fn known_args(kind: &str, args: &[&str]) -> Result<(), String> {
    let (flags, usage) = match kind {
        "test" => (TEST_FLAGS, TEST_USAGE),
        "doc" => (DOC_FLAGS, DOC_USAGE),
        "cr" if args.contains(&"--reason") => return Ok(()),
        "cr" => (CR_FLAGS, CR_USAGE),
        _ => return Err(NOTES_USAGE.into()),
    };
    match args_problem(args, flags, &[], &["--distill"]) {
        Some(what) => refuse(format!("notes write {kind}: {what}"), usage),
        None => Ok(()),
    }
}

/// What a `keel notes write cr` call records.
#[derive(Debug, PartialEq)]
enum CrWrite {
    /// A reviewed round, PASS or FAIL.
    Review,
    /// The review cap's waiver, the writer's only one: the tool writes the reason (B18).
    Cap,
}

/// The cr writer's rules on its flags. `reason`: `None` when `--reason` is absent, `Some("")` when it
/// has no value: the writer takes none, with any verdict (B18). `blockers`/`importants`: the round's
/// open counts, typed or taken from `--findings`.
fn cr_verdict(verdict: &str, reason: Option<&str>, blockers: u32, importants: u32) -> Result<CrWrite, String> {
    match (verdict, reason) {
        (_, Some(_)) => refuse(
            format!("--reason given with --verdict {verdict}: this writer records no reason of a session's own"),
            &format!("drop --reason. The one waiver it writes is the review cap's, whose reason the tool writes; {OWNER_WAIVES}"),
        ),
        ("PASS", None) if blockers + importants > 0 => {
            refuse("PASS claimed with verified blockers/importants".into(), "record FAIL, or fix the findings first")
        }
        ("PASS" | "FAIL", None) => Ok(CrWrite::Review),
        ("WAIVED", None) => Ok(CrWrite::Cap),
        _ => Err(format!("{CR_USAGE}\n       an owner's waiver is not written here: {OWNER_WAIVES}")),
    }
}

/// The round's lists the review skill records (`--findings`, `--refuted`, `--nits` as JSON arrays,
/// `--fleet` as an object), as note fields; anything else is refused before a note is written.
fn cr_lists(args: &[&str]) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let mut out = serde_json::Map::new();
    for (flag, array) in [("--findings", true), ("--refuted", true), ("--nits", true), ("--fleet", false)] {
        let Some(i) = args.iter().position(|a| *a == flag) else { continue };
        let shape = if array { "a JSON array" } else { "a JSON object" };
        let v: serde_json::Value = match args.get(i + 1).map(|raw| serde_json::from_str(raw)) {
            Some(Ok(v)) => v,
            Some(Err(e)) => return refuse(format!("{flag} is not JSON ({e})"), &format!("give {flag} {shape}")),
            None => return refuse(format!("{flag} has no value"), &format!("give {flag} {shape}")),
        };
        if (array && !v.is_array()) || (!array && !v.is_object()) {
            return refuse(format!("{flag} must be {shape}, not {v}"), &format!("give {flag} {shape}; never a prose summary"));
        }
        out.insert(flag.trim_start_matches("--").to_string(), v);
    }
    Ok(out)
}

/// The round's open counts. With `--findings`, they are its entries without a waiver in words, and a
/// typed count that disagrees is refused; without, the typed counts (absent = 0).
fn cr_counts(findings: Option<&serde_json::Value>, blockers: Option<u32>, importants: Option<u32>) -> Result<(u32, u32), String> {
    let Some(list) = findings.and_then(|f| f.as_array()) else { return Ok((blockers.unwrap_or(0), importants.unwrap_or(0))) };
    let mut open = (0, 0);
    for (i, f) in list.iter().enumerate() {
        match f.get("severity").and_then(|s| s.as_str()) {
            Some("blocker") => open.0 += u32::from(!waived_in_words(f)),
            Some("important") => open.1 += u32::from(!waived_in_words(f)),
            other => {
                return refuse(
                    format!("--findings entry {i} has severity {}: findings hold verified blockers and importants only", other.unwrap_or("none")),
                    "give each finding \"severity\": \"blocker\" or \"important\"; nits go in --nits",
                )
            }
        }
    }
    for (flag, typed, derived) in [("--blockers", blockers, open.0), ("--importants", importants, open.1)] {
        if typed.is_some_and(|t| t != derived) {
            return refuse(
                format!("{flag} {} disagrees with --findings, which leaves {derived} open", typed.unwrap_or(0)),
                &format!("drop {flag}: the count comes from --findings"),
            );
        }
    }
    Ok(open)
}

/// `args` are the flags after `notes write <kind>`.
pub fn write(repo: &Path, cfg: &Config, kind: &str, args: &[&str]) -> Result<i32, String> {
    known_args(kind, args)?;
    let r = range(repo, cfg, args)?;
    let tip = r.id.tip.clone();
    match kind {
        "test" => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
            let j: Judgment = serde_json::from_str(&s).map_err(|e| format!("keel: bad test judgment on stdin: {e}"))?;
            let one = |n: &str, l: &Option<JLayer>| l.as_ref().map(|l| layer(repo, cfg, &r, n, l)).transpose();
            let (unit, integration, e2e) = (one("unit", &j.unit)?, one("integration", &j.integration)?, one("e2e", &j.e2e)?);
            if unit.is_none() && integration.is_none() && e2e.is_none() {
                return refuse("the judgment claims no test layer".into(), "give unit, integration and/or e2e");
            }
            store(repo, "test", &tip, &TestNote { schema: "test/v1".into(), id: r.id, unit, integration, e2e })?;
        }
        "doc" => {
            let docs = doc_paths(repo, cfg, &r.id.base, &tip, &r.names)?;
            let mut distill = BTreeMap::new();
            for v in args.windows(2).filter(|w| w[0] == "--distill").map(|w| w[1]) {
                let (p, to) = v.split_once('=').ok_or("usage: --distill <t3-path>=<doc>[,<doc>] | <t3-path>=none: <why>")?;
                distill.insert(p, to);
            }
            let gone: Vec<String> = docs.iter().filter(|p| archived(repo, cfg, &tip, p)).cloned().collect();
            let in_docs = crate::verify_doc::under_root(&cfg.docs.root);
            let lineage = |p: &str| distill_of(p, &in_docs, &gone, &distill);
            let changes: Vec<DocChange> = docs
                .iter()
                .map(|p| DocChange {
                    path: p.clone(),
                    tier: tier_of(cfg, p).unwrap_or("?").into(),
                    distilled_to: gone.contains(p).then(|| lineage(p)).flatten().map(|to| {
                        if to.starts_with("none:") {
                            DistilledTo::DeadEnd(to.into())
                        } else {
                            DistilledTo::Paths(to.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect())
                        }
                    }),
                })
                .collect();
            distill.retain(|p, _| !gone.iter().any(|g| g == p));
            if let Some(p) = distill.keys().next() {
                return refuse(format!("--distill names {p}, which is not a T3 doc archived in this range"), "--distill only the T3 docs this range git rm's");
            }
            if let Some((what, fix)) = doc_problems(repo, cfg, &tip, &docs, &changes).into_iter().next() {
                return refuse(what, &fix);
            }
            store(repo, "doc", &tip, &DocNote { schema: "doc/v1".into(), id: r.id, changes })?;
        }
        "cr" => write_cr(repo, cfg, r, args)?,
        _ => return Err(NOTES_USAGE.into()),
    }
    Ok(0)
}

fn write_cr(repo: &Path, cfg: &Config, r: Range, args: &[&str]) -> Result<(), String> {
    let tip = r.id.tip.clone();
    let base = r.id.base.clone();
    let flag = |k: &str| args.windows(2).find(|w| w[0] == k).map(|w| w[1]);
    let num = |k: &str| flag(k).map(|v| v.parse::<u32>().map_err(|_| format!("keel: {k} needs a number"))).transpose();
    let verdict = flag("--verdict").unwrap_or("");
    let (typed_blockers, typed_importants) = (num("--blockers")?, num("--importants")?);
    let reason = flag("--reason").or_else(|| args.contains(&"--reason").then_some(""));
    let lists = cr_lists(args)?;
    let (blockers, importants) = cr_counts(lists.get("findings"), typed_blockers, typed_importants)?;
    let write = cr_verdict(verdict, reason, blockers, importants)?;
    let code_diff_hash = code_diff_hash(repo, cfg, &base, &tip)?;
    // The schedule exactly as preflight reads it (review::Schedule, one place for both).
    let schedule = || {
        review::Schedule::read(repo, &r.id.base, &tip, &r.target, &cfg.code_paths)
            .or_else(|(what, fix)| refuse(what.trim_start_matches("keel: ").to_string(), &fix))
    };
    let fixes = flag("--fixes").or_else(|| args.contains(&"--fixes").then_some(""));
    if fixes.is_some() && verdict != "PASS" {
        return refuse("--fixes given with a verdict other than PASS".into(), "a fix-PASS is --verdict PASS --fixes <reviewed commit>");
    }
    if write != CrWrite::Review || fixes.is_some() {
        if let Some(k) = lists.keys().next() {
            return refuse(
                format!("--{k} given where no reviewers ran: a fix-PASS or a waiver records no findings of its own"),
                "record the round's lists on its own reviewed note (PASS or FAIL); a waiver keeps them",
            );
        }
    }
    let (note, said) = match (write, fixes) {
        (CrWrite::Review, Some(fixes)) => {
            let sha = git(repo, &["rev-parse", "--verify", "-q", "--end-of-options", &format!("{fixes}^{{commit}}")], &[])
                .or_else(|_| refuse(format!("--fixes {fixes} names no commit"), "give the reviewed commit of the round this fix answers"))?;
            if sha == tip {
                return refuse(
                    format!("--fixes names the tip {} itself; a fix-PASS goes on a later commit than the round it answers", short(&tip)),
                    "commit the fix, then write the fix-PASS on it",
                );
            }
            let mut s = schedule()?;
            // The tip's own note, read with the line: a reviewed one is the latest round, and a fix-PASS never replaces it.
            let own = s.own.take().transpose().or_else(|why| {
                refuse(
                    format!("the cr note on {} is unreadable ({why}); a fix-PASS never overwrites a note it cannot read", short(&tip)),
                    "the owner repairs that note in place, then write the fix-PASS again",
                )
            })?;
            let fix = review::fix_pass(own.as_ref(), &s.earlier, &sha, &s.policy, |c| s.unchanged(c)).or_else(|(what, fix)| refuse(what, &fix))?;
            let said = format!("fix-PASS of round {} on {}: no reviewers run, no new round", fix.round, short(&sha));
            let schedule = ScheduleFields {
                round: Some(fix.round),
                fix_diff_hash: Some(self::code_diff_hash(repo, cfg, &sha, &tip)?),
                answers: Some(Answers { importants: fix.importants, nits: fix.nits }),
                fixes: Some(sha),
                scope: None,
            };
            (
                CrNote {
                    schema: "cr/v1".into(),
                    id: r.id,
                    verdict: "PASS".into(),
                    waiver: None,
                    blockers: 0,
                    importants: 0,
                    code_diff_hash,
                    walk_waivers: vec![],
                    schedule,
                    extra: serde_json::Map::new(),
                },
                said,
            )
        }
        (CrWrite::Review, None) => {
            let s = schedule()?;
            let next = s.next().or_else(|(what, fix)| refuse(what, &fix))?;
            let schedule = ScheduleFields { round: Some(next.round), scope: Some(next.scope()), ..ScheduleFields::default() };
            let note = CrNote {
                schema: "cr/v1".into(),
                id: r.id,
                verdict: verdict.into(),
                waiver: None,
                blockers,
                importants,
                code_diff_hash,
                walk_waivers: vec![],
                schedule,
                extra: lists,
            };
            let said = format!(
                "review round {} of {} recorded ({})\n  next -> {}",
                next.round,
                s.policy.max_rounds,
                next.scope(),
                review::next_step(&note, next.round, &s.policy, &tip)
            );
            (note, said)
        }
        (CrWrite::Cap, _) => {
            if typed_blockers.is_some() || typed_importants.is_some() {
                return refuse("counts given with the cap's waiver".into(), "drop --blockers/--importants: the cap keeps the counts of the round it stops");
            }
            let mut s = schedule()?;
            let own = s.own.take().transpose().or_else(|why| {
                let (what, fix) = unreadable(short(&tip), &why);
                refuse(what, &fix)
            })?;
            let reason = review::cap(own.as_ref(), &s.earlier, &s.policy, &tip).or_else(|(what, fix)| refuse(what, &fix))?;
            let said = format!("{reason}: the owner decides at sign-off (keel mr confirm shows the waiver and every round)");
            (
                waive(own.map(Ok), r.id, Some(&reason), code_diff_hash, &|h| code_binds(repo, cfg, &base, &tip, h))
                    .or_else(|(what, fix)| refuse(what, &fix))?,
                said,
            )
        }
    };
    store(repo, "cr", &tip, &note)?;
    if !said.is_empty() {
        println!("{said}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_batch_read_gives_each_blob_or_why_not_in_order() {
        let out = b"aaa blob 5\nhello\nbbb missing\nccc tree 3\nxyz\nddd blob 2\n\xff\xfe\n";
        let got = parse_batch(out, 4);
        assert_eq!(got[0], Ok("hello".to_string()));
        assert!(got[1].as_ref().unwrap_err().contains("missing"));
        assert!(got[2].as_ref().unwrap_err().contains("tree"));
        assert!(got[3].as_ref().unwrap_err().contains("UTF-8"));
        let short = parse_batch(b"aaa blob 50\nhello\n", 2);
        assert!(short.iter().all(Result::is_err) && short.len() == 2, "a truncated read never invents a note");
        assert_eq!(parse_batch(b"", 1).len(), 1);
    }

    #[test]
    fn the_printed_commands_are_the_checks_argv_with_patches_and_quote_all_but_plain_characters() {
        let d = [OwnDiff { from: "aaa".into(), to: "bbb".into() }, OwnDiff { from: "ccc".into(), to: "ddd".into() }];
        let paths = ["src/".to_string(), "it's ~dir/".to_string()];
        assert_eq!(
            own_diff_commands(&d, &paths),
            "git diff-tree -r --no-renames -p --end-of-options aaa bbb -- src/ 'it'\\''s ~dir/' ; git diff-tree -r --no-renames -p --end-of-options ccc ddd -- src/ 'it'\\''s ~dir/'"
        );
        assert_eq!(own_diff_argv(&d[0], &["--name-only", "-z"], &paths)[3..5], ["--name-only", "-z"]);
        for (a, q) in [("src/a_b-c.rs", "src/a_b-c.rs"), ("~x", "'~x'"), ("a=b", "'a=b'"), ("x^", "'x^'"), ("", "''"), ("$(x)", "'$(x)'")] {
            assert_eq!(shell_quote(a), q);
        }
    }

    #[test]
    fn paths_stay_their_bytes_and_one_that_is_not_utf8_is_never_shown_as_another() {
        let names = union_of_z(&[b"src/b\xff.rs\0src/a.rs\0".to_vec(), b"src/b\xfe.rs\0src/a.rs\0".to_vec()]);
        assert_eq!(names, [b"src/a.rs".to_vec(), b"src/b\xfe.rs".to_vec(), b"src/b\xff.rs".to_vec()], "two paths stay two");
        assert_eq!(changed_text(Ok(names), "s..t"), "unknown (a path is not UTF-8; it is among the branch's own changes in s..t)");
        assert_eq!(changed_text(Ok(union_of_z(&[b"src/b.rs\0src/a.rs\0".to_vec()])), "s..t"), "src/a.rs, src/b.rs");
        assert_eq!(changed_text(Ok(union_of_z(&[vec![]])), "s..t"), "nothing");
        assert_eq!(changed_text(Err("git failed".into()), "s..t"), "unknown (git failed)");
    }

    use super::{bound, cr_counts, cr_lists, cr_verdict, diff_argv, waive, waiver_reason, CrNote, CrWrite, Identity, LEGACY_ABBREV};
    use super::{changed_text, own_diff_argv, own_diff_commands, parse_batch, shell_quote, union_of_z, OwnDiff};
    use super::{distill_of, half_archived, partner};
    use super::{epic_branch, epic_of, target, target_flag, target_from_holders};
    use super::{known_args, CR_USAGE, DOC_USAGE, NOTES_USAGE, TEST_USAGE};
    use std::collections::BTreeMap;

    fn in_docs(p: &std::path::Path) -> bool {
        p.starts_with("docs")
    }

    #[test]
    fn the_partner_of_a_doc_file_is_the_other_half_of_its_pair() {
        assert_eq!(partner("docs/work/a-plan.html", in_docs).as_deref(), Some("docs/work/a-plan.yaml"));
        assert_eq!(partner("docs/work/a-plan.yaml", in_docs).as_deref(), Some("docs/work/a-plan.html"));
        assert_eq!(partner("docs/.archive/e.result.html", in_docs).as_deref(), Some("docs/.archive/e.result.yaml"), "the last dot only");
        for none in ["docs/work/a.md", "docs/work/a.yml", "docs/work/a", "AGENTS.md", "docs/work/.yaml", "docs/work/.html"] {
            assert_eq!(partner(none, in_docs), None, "{none}");
        }
        // review round 1: the pairing rule is verify_doc's, docs.root included
        assert_eq!(partner("notes/x.html", in_docs), None, "a page outside docs.root has no data file");
        assert_eq!(partner("notes/x.yaml", in_docs), None, "a .yaml outside docs.root is no data file");
    }

    #[test]
    fn one_distill_gives_both_files_of_an_archived_pair_the_same_lineage() {
        let gone: Vec<String> =
            ["docs/work/a-plan.html", "docs/work/a-plan.yaml", "docs/work/b.yaml", "docs/work/b.html", "docs/work/c.html"].map(String::from).to_vec();
        let distill = BTreeMap::from([("docs/work/a-plan.html", "docs/x.html"), ("docs/work/b.yaml", "none: nothing"), ("docs/work/c.yaml", "docs/y.html")]);
        // the page names the pair, or the data file does
        assert_eq!(distill_of("docs/work/a-plan.html", in_docs, &gone, &distill), Some("docs/x.html"));
        assert_eq!(distill_of("docs/work/a-plan.yaml", in_docs, &gone, &distill), Some("docs/x.html"));
        assert_eq!(distill_of("docs/work/b.html", in_docs, &gone, &distill), Some("none: nothing"));
        assert_eq!(distill_of("docs/work/b.yaml", in_docs, &gone, &distill), Some("none: nothing"));
        // a key naming a file that is not archived covers nothing: c.yaml is not gone
        assert_eq!(distill_of("docs/work/c.html", in_docs, &gone, &distill), None);
        // each keeps its own when both are named
        let both = BTreeMap::from([("docs/work/a-plan.html", "docs/x.html"), ("docs/work/a-plan.yaml", "docs/z.html")]);
        assert_eq!(distill_of("docs/work/a-plan.yaml", in_docs, &gone, &both), Some("docs/z.html"));
        assert_eq!(distill_of("docs/work/a-plan.html", in_docs, &gone, &both), Some("docs/x.html"));
        // outside docs.root two same-stem files are two docs, each with its own line
        let gone: Vec<String> = ["notes/x.html", "notes/x.yaml"].map(String::from).to_vec();
        let one = BTreeMap::from([("notes/x.html", "docs/x.html")]);
        assert_eq!(distill_of("notes/x.yaml", in_docs, &gone, &one), None);
    }

    #[test]
    fn a_pair_archived_by_half_names_the_file_left() {
        let tip = ["docs/work/a-result.yaml", "docs/work/b-plan.html", "notes/c.yaml"];
        let at = |q: &str| tip.contains(&q);
        assert_eq!(half_archived("docs/work/a-result.html", in_docs, at).as_deref(), Some("docs/work/a-result.yaml"));
        assert_eq!(half_archived("docs/work/b-plan.yaml", in_docs, at).as_deref(), Some("docs/work/b-plan.html"));
        assert_eq!(half_archived("docs/work/c-plan.html", in_docs, at), None, "the pair went whole, or had no data file");
        assert_eq!(half_archived("docs/work/notes.md", in_docs, |_| true), None, "no pair");
        assert_eq!(half_archived("notes/c.html", in_docs, at), None, "outside docs.root a .yaml left is no half of the page");
    }
    use crate::Config;

    fn cfg_with(branches: &str) -> Config {
        toml::from_str(&format!("[check]\nlint = \"true\"\ntest = \"true\"\n[branches]\nmain = \"main\"\n{branches}[docs]\nroot = \"docs/\"\n")).unwrap()
    }

    #[test]
    fn a_bare_tip_resolves_through_its_wu_holders_else_one_epic_branch_and_refuses_everything_else() {
        let cfg = cfg_with("");
        let target = |holders: &[&str]| {
            let holders: Vec<String> = holders.iter().map(|h| h.to_string()).collect();
            target_from_holders(&cfg, &holders)
        };
        assert_eq!(target(&["wu/e/a"]).unwrap(), "epic/e");
        assert_eq!(target(&["wu/e/a", "wu/e/b", "epic/e", "main"]).unwrap(), "epic/e", "the wu branch's epic decides");
        assert_eq!(target(&["epic/e", "main"]).unwrap(), "epic/e", "a merged WU's epic branch still holds it");
        assert!(target(&[]).is_err());
        assert!(target(&["main"]).is_err(), "main only is a refusal, not a guess");
        let err = target(&["wu/a/x", "wu/b/y"]).unwrap_err();
        assert!(err.contains("epic/a") && err.contains("epic/b"), "{err}");
        assert!(target(&["epic/a", "epic/b"]).is_err(), "two epic branches are ambiguous");
    }

    #[test]
    fn the_resolution_reads_the_configured_prefixes_and_never_picks_main_over_a_holder() {
        let cfg = cfg_with("epic_prefix = \"release/\"\nwu_prefix = \"task/\"\n");
        let pool = ["main", "release/e", "task/e/a", "task/f/b", "docs"];
        for n in 0..(1 << pool.len()) {
            let holders: Vec<String> = pool.iter().enumerate().filter(|(i, _)| n & (1 << i) != 0).map(|(_, h)| h.to_string()).collect();
            // The candidates come from the one grammar, so this test cannot drift with it.
            let mut wu_epics: Vec<String> = holders.iter().filter_map(|h| epic_of(&cfg, h)).collect();
            wu_epics.sort();
            wu_epics.dedup();
            let mut epic_branches: Vec<String> = holders.iter().filter_map(|h| epic_branch(&cfg, h)).collect();
            epic_branches.sort();
            epic_branches.dedup();
            let want = match (wu_epics.len(), epic_branches.len()) {
                (1, _) => Ok(wu_epics[0].clone()),
                (0, 1) => Ok(epic_branches[0].clone()),
                _ => Err(()),
            };
            let got = target_from_holders(&cfg, &holders).map_err(|_| ());
            assert_eq!(got, want, "holders {holders:?}");
            if let Ok(t) = got {
                assert_ne!(t, "main", "holders {holders:?}");
            }
        }
    }

    /// Review round 1 (I1): one grammar, read by the writer, the gate's holder path and the flag.
    #[test]
    fn the_writer_and_the_holder_path_read_one_branch_grammar_and_one_flag() {
        for cfg in [cfg_with(""), cfg_with("epic_prefix = \"release/\"\nwu_prefix = \"task/\"\n")] {
            let pool = ["main", "epic/e", "wu/e/a", "wu/e/b", "wu/f/c", "docs", "", "wu//x", "wu/e"];
            for name in pool {
                let holders = [name.to_string()];
                match epic_of(&cfg, name) {
                    Some(epic) => {
                        assert_eq!(target(&cfg, name, &[]), epic, "a wu branch's target is the epic the holder path names: {name}");
                        assert_eq!(target_from_holders(&cfg, &holders), Ok(epic), "{name}");
                    }
                    None => {
                        assert_eq!(target(&cfg, name, &[]), cfg.branches.main, "only a wu branch name names an epic: {name}");
                        match epic_branch(&cfg, name) {
                            Some(epic) => assert_eq!(target_from_holders(&cfg, &holders), Ok(epic), "{name}"),
                            None => assert!(target_from_holders(&cfg, &holders).is_err(), "{name}"),
                        }
                    }
                }
                // The flag is one reader, and it wins over every tip, as the writer and the gate both read it.
                assert_eq!(target(&cfg, name, &["--target", "epic/x"]), "epic/x");
                assert_eq!(target_flag(&["--target", "epic/x"]), Some("epic/x"));
            }
            assert_eq!(target_flag(&["--target"]), None, "a trailing flag with no value is no flag");
            assert_eq!(target_flag(&["--target", "a", "--target", "b"]), Some("a"), "the first flag wins");
        }
    }

    fn raw(n: &CrNote) -> String {
        serde_json::to_string(n).unwrap()
    }

    /// A note as the waiver writer's callers read it: stored, then parsed.
    fn stored(n: &CrNote) -> Option<Result<CrNote, String>> {
        Some(crate::review::parse_note(&raw(n)))
    }

    /// B18: the writer takes no `--reason`, with any verdict: its one waiver is the cap's.
    #[test]
    fn the_cr_writer_takes_no_reason() {
        assert_eq!(cr_verdict("PASS", None, 0, 0), Ok(CrWrite::Review));
        assert_eq!(cr_verdict("FAIL", None, 2, 1), Ok(CrWrite::Review));
        assert_eq!(cr_verdict("WAIVED", None, 0, 0), Ok(CrWrite::Cap), "no reason: the cap's waiver, whose reason the tool writes");
        for (v, r) in [("WAIVED", "  ruling \n"), ("WAIVED", "cap"), ("WAIVED", ""), ("PASS", "x"), ("FAIL", "x"), ("FAIL", "")] {
            let e = cr_verdict(v, Some(r), 0, 0).unwrap_err();
            assert!(e.contains("refused") && e.contains("--reason") && e.contains("fix ->"), "{v} {r:?}: {e}");
        }
        let e = cr_verdict("PASS", None, 1, 0).unwrap_err();
        assert!(e.contains("refused") && e.contains("fix ->"), "{e}");
        for v in ["", "pass", "WAIVE", "CAPPED"] {
            assert!(cr_verdict(v, None, 0, 0).unwrap_err().starts_with("usage:"), "{v}");
        }
    }

    #[test]
    fn the_rounds_lists_are_json_of_their_shape_and_the_findings_decide_the_counts() {
        let args = |a: &[&'static str]| a.to_vec();
        let got =
            cr_lists(&args(&["--verdict", "FAIL", "--findings", "[]", "--refuted", "[{\"claim\": \"x\"}]", "--nits", "[]", "--fleet", "{\"finders\": 2}"]))
                .unwrap();
        assert_eq!(got.keys().collect::<Vec<_>>(), ["findings", "refuted", "nits", "fleet"]);
        assert!(cr_lists(&args(&["--verdict", "PASS"])).unwrap().is_empty());
        for bad in
            [&["--findings"][..], &["--findings", "{}"], &["--refuted", "\"prose\""], &["--nits", "[1"], &["--fleet", "[]"], &["--refuted", "--fleet", "{}"]]
        {
            let e = cr_lists(bad).unwrap_err();
            assert!(e.contains(bad[0]) && e.contains("fix ->"), "{bad:?}: {e}");
        }
        let findings = serde_json::json!([{"severity": "blocker"}, {"severity": "blocker", "waived": "owner: ok"}, {"severity": "blocker", "waived": " "},
            {"severity": "important"}, {"severity": "important", "waived": "r"}]);
        assert_eq!(cr_counts(Some(&findings), None, None), Ok((2, 1)), "a waiver in words closes a finding, a blank one does not");
        assert_eq!(cr_counts(Some(&findings), Some(2), Some(1)), Ok((2, 1)));
        assert!(cr_counts(Some(&findings), Some(1), None).unwrap_err().contains("--blockers 1 disagrees"));
        assert!(cr_counts(Some(&findings), None, Some(0)).unwrap_err().contains("--importants 0 disagrees"));
        assert!(cr_counts(Some(&serde_json::json!([{"severity": "nit"}])), None, None).unwrap_err().contains("severity nit"));
        assert!(cr_counts(Some(&serde_json::json!([{"claim": "x"}])), None, None).unwrap_err().contains("severity none"));
        assert_eq!(cr_counts(None, Some(3), None), Ok((3, 0)), "with no findings, the typed counts");
    }

    #[test]
    fn a_waiver_is_read_only_from_a_waived_verdict() {
        let note = |v: &str, w: Option<&str>| -> CrNote {
            let mut j = serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
                "timestamp": 0, "verdict": v, "blockers": 0, "importants": 0, "code_diff_hash": "h"});
            if let Some(w) = w {
                j["waiver"] = w.into();
            }
            serde_json::from_value(j).unwrap()
        };
        assert_eq!(note("WAIVED", Some("r")).waiver(), Some("r"));
        assert_eq!(note("WAIVED", None).waiver(), Some(""), "a hand-stripped reason reads blank, never absent");
        assert_eq!(note("PASS", Some("r")).waiver(), None, "a stray field on a PASS waives nothing");
        assert_eq!(note("FAIL", None).waiver(), None);
        let n = note("WAIVED", Some("r"));
        let back = serde_json::to_value(&n).unwrap();
        assert!(back.get("walk_waivers").is_none() && back.get("findings").is_none(), "{back}");
    }

    #[test]
    fn a_cr_note_keeps_the_fields_it_does_not_know_and_reads_its_open_findings() {
        let raw = serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c", "timestamp": 0,
            "verdict": "FAIL", "blockers": 1, "importants": 1, "code_diff_hash": "h",
            "findings": [{"claim": "open", "severity": "blocker"}, {"claim": "waived", "severity": "important", "waived": "r"}],
            "refuted": [{"claim": "x"}], "nits": [], "fleet": {"finders": 2}});
        let n: CrNote = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(n.open_findings().iter().map(|f| f["claim"].as_str().unwrap()).collect::<Vec<_>>(), ["open"]);
        assert_eq!(serde_json::to_value(&n).unwrap(), raw, "a round trip keeps every field");
        let bare: CrNote = serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null,
            "committer": "c", "timestamp": 0, "verdict": "PASS", "blockers": 0, "importants": 0, "code_diff_hash": "h", "findings": "prose"}))
        .unwrap();
        assert!(bare.open_findings().is_empty(), "a non-array findings field lists nothing");
    }

    #[test]
    fn no_refusal_tells_an_agent_to_write_a_waiver_for_the_owner() {
        for (v, r, b) in [("PASS", Some("x"), 0), ("FAIL", Some("x"), 1), ("WAIVED", Some(""), 0), ("WAIVED", Some("ruling"), 0)] {
            let e = cr_verdict(v, r, b, 0).unwrap_err();
            let fix = e.split("fix ->").nth(1).unwrap();
            assert!(fix.contains("--waive-review") && !fix.contains("--verdict WAIVED"), "{v} {r:?}: {e}");
        }
    }

    #[test]
    fn one_reason_rule_for_every_waiver_writer() {
        assert_eq!(waiver_reason(Some("  cap  ")), Ok("cap".into()));
        assert!(waiver_reason(None).is_err());
        for bad in [Some(""), Some(" \t"), Some("--blockers"), Some(" --as")] {
            assert!(waiver_reason(bad).is_err(), "{bad:?}");
            assert!(cr_verdict("WAIVED", bad, 0, 0).is_err(), "the cr writer follows it: {bad:?}");
        }
    }

    #[test]
    fn the_one_waiver_writer_keeps_this_ranges_round_whole_marks_anything_else_unreviewed_and_never_replaces_a_pass() {
        let id = || Identity { tip: "t0123456789".into(), base: "b".into(), branch: None, committer: "o".into(), timestamp: 7 };
        let note = |v: &str, base: &str, hash: &str| -> CrNote {
            serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": base, "branch": null, "committer": "r",
                "timestamp": 0, "verdict": v, "waiver": "cap", "blockers": 2, "importants": 1, "code_diff_hash": hash,
                "findings": [{"severity": "blocker", "claim": "f"}], "refuted": [{"claim": "r"}], "nits": [], "fleet": {"finders": 2},
                "walk_waivers": [{"seam": "s", "why": "w"}]}))
            .unwrap()
        };
        let eq_h = |h: &str| Ok(h == "h");
        let fresh = waive(None, id(), Some("ruling"), "h".into(), &eq_h).unwrap();
        assert_eq!((fresh.verdict.as_str(), fresh.waiver(), fresh.blockers, fresh.importants, fresh.unreviewed()), ("WAIVED", Some("ruling"), 0, 0, true));
        assert_eq!(fresh.reason_hash().unwrap(), super::sha256(b"ruling"), "the older keel's hash: the reason's");
        for v in ["FAIL", "WAIVED"] {
            let same = raw(&note(v, "b", "h"));
            let w = waive(Some(crate::review::parse_note(&same)), id(), Some(" ruling "), "h".into(), &eq_h).unwrap();
            assert_eq!(
                (w.waiver(), w.blockers, w.importants, w.code_diff_hash.as_str(), w.id.base.as_str(), w.id.committer.as_str(), w.id.timestamp),
                (Some("ruling"), 2, 1, "h", "b", "o", 7),
                "{v}"
            );
            assert!(!w.unreviewed(), "{v}");
            let (j, before) = (serde_json::to_value(&w).unwrap(), serde_json::to_value(note(v, "b", "h")).unwrap());
            for f in ["findings", "refuted", "nits", "fleet", "walk_waivers"] {
                assert_eq!(j[f], before[f], "{v}: {f} kept");
            }
            for (base, hash) in [("other", "h"), ("b", "other")] {
                let stale = waive(stored(&note(v, base, hash)), id(), Some("r"), "h".into(), &eq_h).unwrap();
                let j = serde_json::to_value(&stale).unwrap();
                assert!(stale.unreviewed() && (stale.blockers, stale.importants) == (0, 0) && stale.open_findings().is_empty(), "{v} {base} {hash}: {j}");
                assert_eq!(
                    (j["replaced"]["findings"][0]["claim"].as_str(), j["replaced"]["base"].as_str()),
                    (Some("f"), Some(base)),
                    "{v}: history kept whole"
                );
                assert_eq!(j["walk_waivers"][0]["seam"], "s", "{v}: seam waivers carried");
            }
        }
        let once = waive(stored(&note("FAIL", "old", "old")), id(), Some("r"), "h".into(), &eq_h).unwrap();
        let twice = waive(stored(&once), Identity { base: "newer".into(), ..id() }, Some("r"), "h2".into(), &|h| Ok(h == "h2")).unwrap();
        let j = serde_json::to_value(&twice).unwrap();
        assert_eq!(j["replaced"]["waiver"], "r", "the last note is kept");
        assert!(j["replaced"].get("replaced").is_none(), "never nested deeper: {j}");
        // B29: a round whose hash the binding rule says binds this range (git's old short form) is this
        // range's round, kept whole and rebound to the stable hash; an error of the rule refuses, writing nothing.
        let legacy = waive(stored(&note("FAIL", "b", "h7")), id(), Some("r"), "h".into(), &|h| Ok(h == "h" || h == "h7")).unwrap();
        assert_eq!((legacy.unreviewed(), legacy.blockers, legacy.code_diff_hash.as_str()), (false, 2, "h"), "a legacy-form round is kept");
        let (what, _) = waive(stored(&note("FAIL", "b", "h7")), id(), Some("r"), "h".into(), &|_| Err("git failed".into())).unwrap_err();
        assert_eq!(what, "git failed");
        let asked = std::cell::Cell::new(false);
        let other_base = waive(stored(&note("FAIL", "other", "h")), id(), Some("r"), "h".into(), &|_| {
            asked.set(true);
            Ok(true)
        })
        .unwrap();
        assert!(other_base.unreviewed() && !asked.get(), "another base is another range, whatever its hash");
        let (what, fix) = waive(stored(&note("PASS", "b", "h")), id(), Some("ruling"), "h".into(), &eq_h).unwrap_err();
        assert!(what.contains("t012345678 is a PASS") && fix.contains("new review"), "{what} / {fix}");
        assert!(waive(stored(&note("PASS", "old", "old")), id(), Some("r"), "h".into(), &eq_h).is_err(), "a stale PASS is not waived over either");
        assert!(waive(None, id(), Some("--blockers"), "h".into(), &eq_h).is_err());
        let (what, _) = waive(Some(crate::review::parse_note("{not json")), id(), Some("r"), "h".into(), &eq_h).unwrap_err();
        assert!(what.contains("does not parse"), "{what}");
    }

    /// B29: the stable text names objects in full; a legacy form is git's short form of one length,
    /// and nothing else in the argv changes with the form.
    #[test]
    fn the_diff_argv_is_full_index_for_the_stable_form_and_abbrev_n_for_a_legacy_one() {
        let paths = vec!["src/".to_string(), "tests/".to_string()];
        let stable = diff_argv("b", "t", &paths, None);
        assert_eq!(stable, ["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--full-index", "b", "t", "--", "src/", "tests/"]);
        for n in LEGACY_ABBREV {
            let legacy = diff_argv("b", "t", &paths, Some(n));
            assert_eq!(legacy[4], format!("--abbrev={n}"));
            assert_eq!((&legacy[..4], &legacy[5..]), (&stable[..4], &stable[5..]), "only the ids' form differs");
        }
        assert_eq!(diff_argv("b", "t", &[], None).last().map(String::as_str), Some("--"), "no pathspec: the full diff");
    }

    /// B29: the rule tries the stable form, then 7..=12 in order, and stops at the first match; a hash
    /// no form gives (another diff, or a length outside the bound) fails after every form was tried.
    #[test]
    fn the_binding_rule_accepts_the_stable_form_or_a_legacy_length_from_7_to_12_and_nothing_else() {
        assert_eq!(LEGACY_ABBREV, 7..=12);
        let text = |a: Option<usize>| a.map_or_else(|| "full".to_string(), |n| format!("short{n}"));
        let run = |stored: &str| {
            let mut tried = vec![];
            let ok = bound(stored, |a| {
                tried.push(a);
                Ok(text(a))
            })
            .unwrap();
            (ok, tried)
        };
        assert_eq!(run("full"), (true, vec![None]), "a current note costs one diff");
        for n in 7..=12 {
            let (ok, tried) = run(&format!("short{n}"));
            assert!(ok, "{n}");
            assert_eq!(tried, std::iter::once(None).chain((7..=n).map(Some)).collect::<Vec<_>>(), "{n}");
        }
        for stored in ["short6", "short13", "short40", "other diff", ""] {
            let (ok, tried) = run(stored);
            assert!(!ok, "{stored}");
            assert_eq!(tried.len(), 7, "{stored}: every form tried");
        }
        assert_eq!(bound("x", |_| Err("git failed".to_string())), Err("git failed".to_string()), "an error is an error, never a verdict");
    }

    /// B17 (open point 2): the signed waiver covers its reason, counts, review state, open findings and
    /// walk waivers - nothing else - and no reason can pass for another field.
    #[test]
    fn the_waiver_hash_covers_exactly_what_the_owner_signs() {
        let base = serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c", "timestamp": 0,
            "verdict": "WAIVED", "waiver": "r", "blockers": 1, "importants": 0, "code_diff_hash": "h",
            "findings": [{"severity": "blocker", "claim": "open"}, {"severity": "important", "claim": "closed", "waived": "w"}],
            "walk_waivers": [{"seam": "s", "why": "w"}], "refuted": [{"claim": "x"}], "nits": []});
        let hash = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut j = base.clone();
            f(&mut j);
            serde_json::from_value::<CrNote>(j).unwrap().waiver_hash().unwrap()
        };
        let h = hash(&|_| {});
        assert!(h.starts_with("sha256:") && h != super::sha256(b"r"));
        let signed: [&dyn Fn(&mut serde_json::Value); 8] = [
            &|j| j["waiver"] = "r2".into(),
            &|j| j["blockers"] = 2.into(),
            &|j| j["importants"] = 1.into(),
            &|j| j["unreviewed"] = true.into(),
            &|j| j["findings"][0]["claim"] = "softer".into(),
            &|j| j["findings"][0]["waived"] = "closed later".into(),
            &|j| j["walk_waivers"][0]["why"] = "other".into(),
            &|j| j["waiver"] = "r\",\"blockers\":1".into(),
        ];
        for (i, f) in signed.iter().enumerate() {
            assert_ne!(hash(f), h, "edit {i} is signed");
        }
        let free: [&dyn Fn(&mut serde_json::Value); 4] = [
            &|j| j["findings"][1]["claim"] = "closed, reworded".into(),
            &|j| j["refuted"] = serde_json::json!([]),
            &|j| j["committer"] = "other".into(),
            &|j| j["nits"] = serde_json::json!([{"n": 1}]),
        ];
        for (i, f) in free.iter().enumerate() {
            assert_eq!(hash(f), h, "edit {i} is not signed");
        }
        let mut pass = base.clone();
        pass["verdict"] = "PASS".into();
        assert_eq!(serde_json::from_value::<CrNote>(pass).unwrap().waiver_hash(), None);
    }

    /// B17: one transaction moves every real ref only from the value it was read at, creates an
    /// absent one only if still absent, and deletes every pending ref.
    #[test]
    fn the_notes_transaction_moves_each_ref_from_its_read_value_and_drops_the_pending_refs() {
        let m = |real: &str, new: &str, old: Option<&str>| super::Move {
            real: real.into(),
            new: new.into(),
            old: old.map(String::from),
            pending: format!("{real}-pending"),
        };
        assert_eq!(
            super::transaction(&[m("refs/notes/cr", "c2", Some("c1")), m("refs/notes/mr", "m1", None)]),
            "start\nupdate refs/notes/cr c2 c1\ndelete refs/notes/cr-pending c2\ncreate refs/notes/mr m1\ndelete refs/notes/mr-pending m1\ncommit\n"
        );
    }

    #[test]
    fn the_identity_keys_come_from_the_type() {
        let mut k = Identity::keys();
        k.sort();
        assert_eq!(k, ["base", "branch", "committer", "timestamp", "tip"], "Identity changed: every place that strips it follows Identity::keys");
    }

    /// B5: a blank or non-string `waived` waives nothing, as everywhere else.
    #[test]
    fn a_finding_is_waived_only_by_a_waiver_in_words() {
        let n: CrNote = serde_json::from_value(serde_json::json!({"schema": "cr/v1", "tip": "t", "base": "b", "branch": null, "committer": "c",
            "timestamp": 0, "verdict": "FAIL", "blockers": 5, "importants": 0, "code_diff_hash": "h",
            "findings": [{"claim": "null", "waived": null}, {"claim": "false", "waived": false}, {"claim": "blank", "waived": "  "},
                {"claim": "none"}, {"claim": "words", "waived": "owner: fine"}]}))
        .unwrap();
        let open: Vec<_> = n.open_findings().iter().map(|f| f["claim"].as_str().unwrap()).collect();
        assert_eq!(open, ["null", "false", "blank", "none"]);
    }

    #[test]
    fn known_args_accept_each_kinds_flags_with_their_values() {
        for (kind, args) in [
            ("test", &[][..]),
            ("test", &["--target", "epic/e"]),
            ("doc", &["--distill", "a=b", "--target", "epic/e", "--distill", "c=none: why"]),
            ("cr", &["--verdict", "FAIL", "--blockers", "1", "--importants", "0", "--findings", "[]", "--refuted", "[]", "--nits", "[]"]),
            ("cr", &["--verdict", "PASS", "--fixes", "abc", "--fleet", "{}", "--target", "epic/e"]),
        ] {
            assert_eq!(known_args(kind, args), Ok(()), "{kind} {args:?}");
        }
    }

    #[test]
    fn known_args_refuse_an_unknown_flag_a_stray_word_a_missing_value_a_repeat_and_an_unknown_kind() {
        for (kind, args, needle, usage) in [
            ("doc", &["--distil", "x=y"][..], "notes write doc: unknown argument '--distil'", DOC_USAGE),
            ("doc", &["stray"], "unknown argument 'stray'", DOC_USAGE),
            ("doc", &["--target"], "--target needs a value", DOC_USAGE),
            ("test", &["--distill", "a=b"], "notes write test: unknown argument '--distill'", TEST_USAGE),
            ("test", &["--target", "a", "--target", "b"], "--target given twice", TEST_USAGE),
            ("cr", &["--verdict", "PASS", "--blokers", "0"], "unknown argument '--blokers'", CR_USAGE),
            ("cr", &["--verdict", "PASS", "--verdict", "FAIL"], "--verdict given twice", CR_USAGE),
            ("cr", &["--verdict", "PASS", "--help"], "unknown argument '--help'", CR_USAGE),
            // Round 1, I2: a value that is a known flag gives the argv two readings, so it is refused.
            ("doc", &["--distill", "--target", "--target", "epic/x"], "--distill needs a value", DOC_USAGE),
            ("cr", &["--verdict", "PASS", "--findings", "--blockers", "1"], "--findings needs a value", CR_USAGE),
        ] {
            let e = known_args(kind, args).unwrap_err();
            assert!(e.starts_with("keel notes: refused - ") && e.contains(needle) && e.contains(&format!("fix -> {usage}")), "{kind} {args:?}: {e}");
        }
        assert_eq!(known_args("report", &["x.html"]), Err(NOTES_USAGE.to_string()));
    }

    #[test]
    fn a_cr_reason_is_left_to_its_own_refusal() {
        for args in [&["--verdict", "WAIVED", "--reason"][..], &["--verdict", "WAIVED", "--reason", "--blockers", "2"]] {
            assert_eq!(known_args("cr", args), Ok(()), "{args:?}: cr_verdict refuses it, naming --waive-review");
        }
    }
}
