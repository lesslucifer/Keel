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
- **A round past `whole_range_rounds` reviews only the last fix** (the schedule, §5):
  the branch's own changes since `SINCE`, the last reviewed commit. Preflight
  (§2) prints the commands that show them, one `git diff-tree -p` per commit
  of the branch's own line, oldest first, and finders and verifiers get their
  output, run exactly as printed, as their diff: each commit against its first
  parent, a merge against git's automatic merge of its parents, so only what
  the merge changed by hand, never what a back-merge brought from its second
  parent. Its `changed:` list is the files of that diff. Never `git diff $SINCE $HEAD_R`: after a
  back-merge it holds other branches' code. They may read any file, and report findings only on those changes. The
  note still binds the whole range.
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
- **`0·review-cap`** — the schedule (§5) has no such round: past
  `max_rounds`, a round after one from `fix_importants_from` on that found no
  blocker, a round after one from `fix_blockers_from` on, or an earlier cr note
  on the branch that does not parse (it names the commit). Or the target's
  schedule cannot be read: a target that names no commit, a config git cannot
  read, or one the tool refuses (§5).
- **Nothing else**: no cr (this run writes it), no doc note, no `mr/v1` (the
  human signs last).

On green it prints the round and its scope, by the review's level (§5):
`review round <n> of <max_rounds> - scope: the whole range, <base>..<tip>`
(rounds up to `whole_range_rounds`) or `… scope: the last fix, <since>..<tip>:
this branch's own changes, …: <commands>; changed: <files>` (later rounds). Use
that range, and in a later round those commands (§1).

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
- the scope (`$BASE..$HEAD_R`, or in a round past `whole_range_rounds`
  `$SINCE..$HEAD_R` with preflight's own-changes command, §1, plus the working
  tree if in scope) — the finder runs `git diff` (in such a round, that
  command) itself, may read any
  file, and reads the tip's test note
  (`git log -1 --no-show-signature --no-notes --notes=test --format=%N $HEAD_R`;
  a review session may run no `git notes` command, reads included): a declared
  waiver is a review subject, not a free pass. Documents are not read;
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

**Earlier notes.** Before spawning verifiers, collect what earlier notes on
this branch recorded. An earlier note is a cr note on a first-parent commit of
the range, other than the tip (a re-run on the same tip replaces the tip's
note). It may be a reviewed round, a fix-PASS or a WAIVED note: all of them go
in the list. Which notes count as rounds is the schedule's question, not this
list's. An epic reads only its own line, never its WUs' notes. A review
session may run no `git notes` command, so each note is read with `git log`
(an empty `%N` is a commit with no note):

```sh
EARLIER=$(git rev-list --first-parent "$BASE..$HEAD_R" | tail -n +2 \
  | while read -r c; do
      n=$(git log -1 --no-show-signature --no-notes --notes=cr --format=%N "$c") \
        || { echo "unreadable cr note on $c" >&2; continue; }
      [ -n "$n" ] || continue
      printf '%s' "$n" \
        | jq -e -c --arg c "$c" '{commit: $c, findings: (.findings // []), nits: (.nits // []), refuted: (.refuted // [])} | select([.findings, .nits, .refuted] | all(type == "array"))' \
        || echo "unreadable cr note on $c" >&2
    done \
  | jq -s -c 'map(select((.findings + .nits + .refuted) | length > 0))')
```

A note written before these fields existed has none; it counts as empty.
If the reader prints anything on stderr (an `unreadable cr note on <commit>`
line or a jq error), stop before the verifiers and report each named commit to
the caller: never pass a partial list. The owner repairs a named note in
place; never remove it (the schedule counts rounds from these notes). With no
error, `EARLIER` is a JSON array, and `[]` when there are no earlier notes
(round 1).

A verifier receives **only** the finding(s), the scope and `EARLIER` — never the
finder's reasoning. Its prompt:

- try to **refute** the claim against actual behaviour: read the surrounding
  code, trace callers and data flow, run a targeted test when cheap — never
  judge from the hunk alone;
- check `docs/decisions/` (when it exists) before confirming a finding that
  "fixes" something deliberate;
- resolve uncertainty to **false-positive** — with no human filter downstream a
  false positive costs more than a missed nit;
- **judge the severity** against `rubric.md`'s rows rather than inheriting the
  finder's; uncertainty between two rows resolves **upward**;
- **a repeat is refuted** (owner comment 2026-11-12). A claim is a repeat
  only when it is surely the same defect, in the same code, as an entry in
  `EARLIER`, however it is worded. Then:
  - the entry was **fixed** (a verified finding, or a nit with `fixed`): return
    false-positive with `repeat_of: <its commit>`, unless you show, against the
    code at the tip, that the fix does not hold. Name the failing case (the
    input or path, and the wrong result). Then judge it as a finding against
    that fix, at its own severity;
  - the entry was **waived or refuted**: return false-positive with
    `repeat_of: <its commit>` only while the code it judged is unchanged since
    that commit. That code is its own file and every file its `why` or waiver
    reason names (a refutation often rests on other code, such as a caller's
    guard): `git diff <its commit> $HEAD_R -- <its file> <each named file>`
    must show no change to what the judgment relies on. If any of it changed,
    or the judgment rests on code it does not name, the old judgment no longer
    covers the claim: it is new;
  - **in any doubt** whether it is a repeat, it is a new claim, judged by the
    rules above. The false-positive default does not decide this question;
- name in `evidence` every file:line a refutation relies on;
- return one `{verdict: real|false-positive, severity, evidence, repeat_of?}`
  per claim.

Refuted findings are dropped from the verdict, recorded on the note (§6) and
mentioned in one line to the caller.

## 5 · Verdict

- Match the trigger's `--waive` declarations against verified findings; record
  each with its reason (cite a `docs/decisions/` record where one applies).
- **PASS** = `keel check` green and zero verified **blockers or importants** left
  unwaived. Anything else is **FAIL**. Evidence is written either way.
- **A verified nit never fails the review, never starts a new round, and is
  never silently dropped**:
  each gets **exactly one** disposition — `fixed: "<what changed>"` (inside
  the range the note binds, or after the verdict, recorded with a fix-PASS,
  below) or `waived: "<reason>"`.
- **WAIVED** is never a reviewer's verdict, and nobody writes PASS for a
  review that did not run or did not pass. The owner waives a review in the
  sign-off itself (`keel mr confirm … --waive-review "<reason>"`, skill
  `keel-workflow` §5–§6); no session writes an owner's waiver or stages that
  flag. The one waiver a review writes is the cap's (below). A waiver keeps
  the round's note whole; its open findings are the `findings` entries
  without `waived` (§6), and the sign-off prints them.

**The schedule** (UC-REV-3). A round is one run of this skill on one commit,
recorded as a PASS or FAIL cr note. Re-running on the same commit is the same
round. `keel preflight` and `keel notes write cr` count the rounds from the cr
notes on the branch's own first-parent line (an epic never counts its WUs'),
refuse any round the schedule does not reach, and the writer prints the next
step after each round.

The settings are read from the target's committed `.keel/config.toml`; a
branch never sets its own. `[review]` holds the shared settings, and two
optional tables, `[review.wu]` and `[review.epic]`, may set any of them for
one **level**. A review whose target is the main branch (`[branches] main`, by
name or as `refs/heads/<main>`) is an epic review; any other is a WU review.
Preflight, the writer and the sign-off history read the level this one way.
Per setting, the level's table wins, then `[review]`, then the default.

| Setting | What it decides |
|---|---|
| `max_rounds` | the cap: no round runs past it |
| `whole_range_rounds` | rounds up to this one review the whole range; later rounds only the last fix (§1) |
| `fix_importants_from` | from this round on, importants with no blocker are fixed under a fix-PASS, and only a blocker earns another round |
| `fix_blockers_from` | unset unless written. From this round on every counted finding, blockers too, is fixed under a fix-PASS: no round follows, and the cap's waiver is refused |

What follows round `n`:

| Round `n` found | Then |
|---|---|
| only nits | PASS; nits fixed after it under a fix-PASS |
| importants, no blocker | before `fix_importants_from` and `fix_blockers_from`: fix, round `n+1`; from either on: fix under a fix-PASS, done |
| a blocker | from `fix_blockers_from` on: fix under a fix-PASS, done; no waiver, and the owner reads the fix at sign-off. Before it (or with it unset), before `max_rounds`: fix, round `n+1`; in round `max_rounds`: stop, the cap's WAIVED; the owner decides at sign-off |
| a FAIL with nothing counted (`keel check` red, say) | before `fix_importants_from` and `fix_blockers_from`: round `n+1`; else no round and no fix-PASS: only the owner's waiver |

**The defaults** — a setting no table sets takes its value here, at both
levels. This is the only place the rules give them:

| `max_rounds` | `whole_range_rounds` | `fix_importants_from` | `fix_blockers_from` |
|---|---|---|---|
| 3 | 1 | 2 | unset |

A written setting is a whole number of 1 or more that fits a 32-bit unsigned
number, and not above its table's `max_rounds` (a level's own, else
`[review]`'s, else the default). A value a level inherits above its
`max_rounds` is lowered to it; a level cannot unset an inherited
`fix_blockers_from`. The tool refuses, naming the table and the key: a value
that breaks this; a `[review]` key other than the four, `modules` and
`rubric`; a table under `[review]` other than `wu` and `epic`; a key in a
level table that is not one of the four; a `[review]`, `review.wu` or
`review.epic` that is not a table; and a config that does not parse. A bad
value at either level refuses the schedule for both. A refused schedule is red
`0·review-cap` at preflight and refused by the writer; staged, it is refused
by the pre-commit hook and `keel mr commit`. It never blocks the owner's
sign-off: `keel mr confirm` then tags the history's rounds by the defaults and
says so.

- **Fix-PASS**: after the fix is committed and its test and doc notes are
  written, `keel notes write cr --verdict PASS --fixes <reviewed commit>`. No
  reviewers run. The tool allows it only for the latest round, only for the
  findings the schedule fixes without a round (importants: from
  `fix_importants_from`; every counted finding, blockers too: from
  `fix_blockers_from`), and never on top of another fix-PASS when code
  changed since it (a second fix needs a round: where one follows, the next
  round; where none follows, only the owner's waiver — so write the fix-PASS
  only on the final fix). It records
  `fixes`, `round`, `fix_diff_hash` and `answers`, whatever it answers: the
  blockers stay on the reviewed round's note. The sign-off lists it with the
  blockers it answers and the files changed since the reviewed commit. Only a
  session allowed to write cr notes writes it.
- **A later commit that changes no code path** (the result report, the
  owner's skill commit) runs no reviewers and costs no round: it gets a
  fix-PASS answering the same round where the schedule allows one, even on
  top of a fix-PASS. Otherwise it gets the round preflight names. A failed
  round stays failed: a docs-only commit after it fixes nothing.
- **The cap**: when round `max_rounds` fails with a blocker, below
  `fix_blockers_from` or with it unset, the writer says
  `review cap reached`. On the same tip, run `keel notes write cr --verdict
  WAIVED`: the tool writes the reason (`review cap: <max_rounds> rounds, <b>
  blockers open`) and keeps the round's findings on the waiver. It is refused anywhere
  else, and the writer refuses `--reason` with any verdict. The owner accepts
  it, or not, at sign-off (`--accept-waiver`, typed by the owner only).

## 6 · Evidence — the `cr/v1` note on the reviewed tip

With the tip checked out (preflight guaranteed it):

```sh
keel notes write cr --verdict PASS|FAIL [--target <ref>] \
  --findings "$FINDINGS" --refuted "$REFUTED" --nits "$NITS" --fleet "$FLEET"
```

One call writes the whole note. The CLI derives the range, the identity,
`code_diff_hash`, the round and its `scope` itself, and counts `blockers` and
`importants` from `FINDINGS` (entries without a `waived` reason; a typed
`--blockers`/`--importants` that disagrees is refused). It refuses `PASS` with
open blockers or importants, a round the schedule does not reach, and any list
that is not JSON of its shape (arrays; `FLEET` an object): a prose `refuted`
would stop every later round's reader (§4). A refused call writes nothing. The
lists are extra fields the gate ignores. Every round writes them, PASS or
FAIL: the next round's verifiers read them (§4).

- `FINDINGS`: every verified blocker and important, waived or not: `[{"location": "file:line", "claim": "<sentence>", "severity": "blocker|important", "evidence": "<verifier's, one line>"}]`, plus `"waived": "<reason>"` on a waived one
- `REFUTED`: every refuted candidate, never a prose summary: `[{"location": "file:line", "claim": "<sentence>", "why": "<verifier's, one line, naming every file:line it relies on>"}]`, plus `"repeat_of": "<commit>"` on a refuted repeat
- `NITS`: `[{"location": "file:line", "finding": "<summary>", "fixed": "<what changed>"} | {"location": "file:line", "finding": "<summary>", "waived": "<reason>"}]`
- `FLEET`: `{"dimensions": [...], "finders": n, "verifiers": n, "verifier_model": "<id>", "candidates": {"blocker": n, "important": n, "nit": n}, "by_finder": {"correctness": {"found": n, "verified": n}, ...}, "stage_seconds": {"check": n, "finders": n, "verifiers": n, "total": n}, "waivers": [{"finding": "...", "reason": "..."}]}`

Write what you observed and omit what you did not: an absent row is a stage
nobody timed, never a zero. A dimension not spawned has no `by_finder` row.

The gate (checks 3–5) reads `verdict`, `base` and `code_diff_hash`; the
pre-push hook syncs `refs/notes/*`. A docs-only commit on top of a reviewed tip
keeps the hash but moves the tip: the gate wants a cr note **on the new tip**,
never a copied one — a fix-PASS where the schedule allows it (§5), else the
round preflight names.

## 7 · Return to the caller

Report: the round and its scope; the verdict; the verified findings with
severities and verifier evidence; waivers applied; the nits and their
dispositions (they live on the cr note — never edit the result report after
the review just to copy them, that moves the tip); refuted candidates in one
line (on the note they stay the `REFUTED` array, §6); the fleet bill; the note
ref; the next step the writer printed. A **PASS** with nits is done: a nit
never starts another round; the nits marked fixed are fixed after it and
recorded with a fix-PASS (§5). On **FAIL**, the triggering session fixes — a
correctness bug gets its failing, named regression test **first**, cleanups
stay behaviour-preserving with the suite green — then follows the schedule
(§5): the next round on the new tip, a fix-PASS, or, at the cap, the cap's
waiver and the owner's decision at sign-off. Never a round the schedule
refuses.
