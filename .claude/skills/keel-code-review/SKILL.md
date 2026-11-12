---
name: keel-code-review
description: Run Keel's multi-agent code review - a read-only run over a caller-supplied scope; mechanical preflight (keel preflight), two or three isolated finders over the diff's code paths (from .keel/config.toml), adversarial verifiers, a severity-decided verdict, and a cr/v1 note bound to the reviewed code. The review judges code and nothing else; a diff that changes no code path asks for no review. Use when a WU or epic completes, before its sign-off and merge, or to review recent commits. The caller defines the scope (WU vs its epic, epic vs main, or recent commits) - never assume a base.
---

# Code review — orchestration

A review run is **read-only**: no agent in it edits the tree or commits.
Fixing belongs to the session that triggered the run (§7). What to look for
lives in `rubric.md` next to this file (universal) and in the project's own
rubric (`[review] rubric` in `.keel/config.toml`, default `.keel/rubric.md`,
optional). Finders read both; this file only sequences the run. The triggering
session orchestrates but **never judges its own diff**.

**The subject is code**: the paths starting with one of `code_paths` in
`.keel/config.toml` — the same definition the gate uses (every path, when the
list is empty). A document is never judged, cited or bound here; a diff that
changes no code path asks for no review, no note and no finders.

## 1 · Resolve scope

- Argument `<base>..<head>` (e.g. `epic/cap..wu/cap/paging`, `main..epic/cap`),
  or a single ref meaning `<ref>^..<ref>`. No argument: `HEAD^..HEAD` plus any
  uncommitted changes.
- **Never widen the scope** on your own — the caller owns it.
- Pin full SHAs: `BASE=$(git merge-base <base> <head>)`,
  `HEAD_R=$(git rev-parse <head>)`. Merge-base, not the base tip.
- The reviewed diff is `git diff $BASE $HEAD_R -- <code_paths…>` — exactly the
  bytes `keel notes write cr` will hash into `code_diff_hash`.
- Optional, repeatable: `--waive "<finding summary>: <reason>"` — the
  trigger's waiver declarations, applied at the verdict (§5).

## 2 · Preflight — abort if red

`keel preflight wu <tip> [--target <epic branch>]` or
`keel preflight epic <tip>` — the same levels and target inference as the merge
gate. This is the last point where a red input is repaired for free: the cr
note pins the code it judged, so anything fixed after the verdict buys the
fleet again. It refuses on exactly:

- **`0·tree`** — `HEAD` is not the tip, the tree has uncommitted changes, or
  `keel check` (the configured lint + test, skipped when this exact tree content
  already passed) is red.
- **`1·notes` / `2·manifest`** — the tip's `test/v1` note (the one the finders
  read) is missing, malformed, lacks the level's layer, or names another tip.
- **Nothing else**: no cr (this run writes it), no doc note, no `mr/v1` (the
  human signs last).

Non-zero exit ⇒ stop, report its blocks to the caller, spawn nothing, write
nothing. An **exempt** diff (no code path) prints `exempt`: report that and
stop.

**WU and epic scopes only.** For any other scope (recent commits, a single
ref, a standalone commit) there is no note to orphan: run `keel check` yourself
and go on.

## 3 · Find — two or three isolated finders, in parallel

| Spawn | When |
|---|---|
| **correctness**, **efficiency** | always |
| **altitude** | the diff's code paths resolve to **more than one module** |

A **module** is defined by `[review] modules` in `.keel/config.toml` (globs,
default `["src/*"]`): each distinct match of a glob's `*` — e.g. `src/<name>.rs`
or `src/<name>/` — is one module. A code path matching no glob is its own
module by its top directory. A test file counts as the module it tests, never
as one of its own, so a change confined to one module and its tests spawns two
finders.

Spawn the finders **in a single message**, fresh context each, one per
dimension, each on an explicitly named model. Each prompt contains exactly:

- its dimension, and the instruction to read `rubric.md` (this skill's
  directory) and the project rubric if it exists — its dimension row, the
  principles, the exclusions;
- the scope (`$BASE..$HEAD_R`, plus the working tree if in scope) — the finder
  runs `git diff` itself, may read any file, and reads the tip's test note
  (`git notes --ref=test show $HEAD_R`): a declared waiver is a review subject,
  not a free pass. Documents are not read;
- the return format: candidate findings, each
  `{location: file:line, claim: <one falsifiable sentence>, severity: blocker|important|nit}`.

**Never merge finders** — one dimension, one agent. Folding dimensions into one
agent moves the work into fewer, slower contexts (Trawler measured +354 s per
merged agent, the stage 152% slower for a 38% token saving).

## 4 · Verify — relevance-batched adversarial verifiers, in parallel

Group the candidate pool by **the code a verifier must read** (same function,
same call path); everything else stays apart. Spawn every batch in one message.

| Rule | Why |
|---|---|
| A proposed **blocker** never joins a batch — one fresh agent each | blockers decide the verdict; the costliest severity keeps its own adversary |
| A batch holds **at most 4** claims | the saving is one shared read, not a smaller fleet; one agent reading every region serially is far slower |
| Each claim is judged on **its own** evidence | grouping puts correlated claims in one context — a confirmed claim lends its neighbour nothing; say so in the prompt |

A verifier receives **only** the finding(s) and the scope — never the finder's
reasoning. Its prompt:

- try to **refute** the claim against actual behaviour: read the surrounding
  code, trace callers and data flow, run a targeted test when cheap — never
  judge from the hunk alone;
- check `docs/decisions/` (when it exists) before confirming a finding that
  "fixes" something deliberate;
- resolve uncertainty to **false-positive** — with no human filter downstream a
  false positive costs more than a missed nit;
- **judge the severity** against `rubric.md`'s rows rather than inheriting the
  finder's; uncertainty between two rows resolves **upward**;
- return one `{verdict: real|false-positive, severity, evidence}` per claim.

Refuted findings are dropped; mention them in one line to the caller.

## 5 · Verdict

- Match the trigger's `--waive` declarations against verified findings; record
  each with its reason (cite a `docs/decisions/` record where one applies).
- **PASS** = `keel check` green and zero verified **blockers or importants** left
  unwaived. Anything else is **FAIL**. Evidence is written either way.
- **A verified nit never fails the review and is never silently dropped**:
  each gets **exactly one** disposition — `fixed: "<what changed>"` (the fix is
  already inside the range the note binds) or `waived: "<reason>"`. Nothing
  licenses a post-verdict fix: a later code commit moves the hash and orphans
  the note.
- **WAIVED** is never a reviewer's verdict, and nobody writes PASS for a
  review that did not run or did not pass. The owner waives a review in the
  sign-off itself (`keel mr confirm … --waive-review "<reason>"`, skill
  `keel-workflow` §5–§6); no session writes an owner's waiver or stages that
  flag. The writer `keel notes write cr --verdict WAIVED --reason "<why>"` is
  for a stop the review tool fixes (the round cap) and has no legitimate
  caller until that cap exists. A waiver keeps the round's note whole; its
  open findings are the `findings` entries without `waived` (§6), and the
  sign-off prints them.

## 6 · Evidence — the `cr/v1` note on the reviewed tip

With the tip checked out (preflight guaranteed it):

```sh
keel notes write cr --verdict PASS|FAIL --blockers <n> --importants <n> [--target <ref>]
```

The CLI derives the range, the identity and `code_diff_hash` itself, and
refuses `PASS` with verified blockers or importants. Then record the nits and
the round's bill on the same note — extra JSON fields the gate ignores, added
the way `walk_waivers` are:

```sh
git notes --ref=cr show "$HEAD_R" \
  | jq -c --argjson nits "$NITS" --argjson fleet "$FLEET" '. + {nits: $nits, fleet: $fleet}' \
  | git notes --ref=cr add -f -F - "$HEAD_R"
```

- `NITS`: `[{"finding": "<summary>", "fixed": "<what changed>"} | {"finding": "<summary>", "waived": "<reason>"}]`
- `FLEET`: `{"dimensions": [...], "finders": n, "verifiers": n, "verifier_model": "<id>", "candidates": {"blocker": n, "important": n, "nit": n}, "by_finder": {"correctness": {"found": n, "verified": n}, ...}, "stage_seconds": {"check": n, "finders": n, "verifiers": n, "total": n}, "waivers": [{"finding": "...", "reason": "..."}]}`

Write what you observed and omit what you did not: an absent row is a stage
nobody timed, never a zero. A dimension not spawned has no `by_finder` row.

The gate (checks 3–5) reads `verdict`, `base` and `code_diff_hash`; the
pre-push hook syncs `refs/notes/*`. A docs-only commit on top of a reviewed tip
keeps the hash but moves the tip: the gate wants a cr note **on the new tip** —
run the review again, never copy a note.

## 7 · Return to the caller

Report: the verdict; the verified findings with severities and verifier
evidence; waivers applied; the nits and their dispositions (they live on the cr note — never edit
the result report after the review just to copy them, that moves the tip); refuted candidates in one line; the
fleet bill; the note ref. On **FAIL**, state the loop: the triggering session
fixes — a correctness bug gets its failing, named regression test **first**,
cleanups stay behaviour-preserving with the suite green — then re-runs this
skill on the new tip until it returns PASS.
