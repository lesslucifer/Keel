---
name: keel-workflow
description: Keel's workflow standard - how work is planned, tested, gated and merged in a Keel project (epic -> working unit -> commit, one hook-enforced gate per level, completeness proven from git notes, never from an agent's word). Use before starting any epic, WU or commit in a repo with .keel/config.toml; when writing test/doc notes, staging a sign-off, merging, waiving a test layer or a walk seam, or when a Keel gate prints FAIL. Also holds the testing charter (epic -> e2e, WU -> integration, commit -> unit).
---

# Keel workflow

Work decomposes **epic → working unit (WU) → commit**. Each level has a
mandatory test layer, a gate, and evidence stored as git notes on the gated
tip. The merge **is** the gate: a git hook runs `keel verify-dod` and a red gate
makes the merge itself fail. Project policy (code paths, lint/test commands,
main branch, doc tiers, signer tiers) lives in `.keel/config.toml`; this skill
is the rule, the config fits it to the project.

New repo: run `keel init` once (config, `AGENTS.md`, Codex files, docs
skeleton, hooks), then follow the bootstrap and `keel mr setup` lines it prints.

## 1 · Levels, branches, ids

| Level | Branch | Plan artifact (skill `keel-plan`) | Gate | Evidence on the gated tip |
|---|---|---|---|---|
| Epic | `epic/<slug>` off main | epic plan, WU summaries included | verify-dod **epic**, at the merge into main | `test/v1` (e2e) · `cr/v1` PASS · `doc/v1` · `mr/v1` · `dod/v1` |
| WU | `wu/<epic-slug>/<slug>` off its epic | WU plan doc, before code | verify-dod **wu**, at the merge into the epic | `test/v1` (integration) · `cr/v1` PASS · `doc/v1` · `mr/v1` · `dod/v1` |
| Commit | on its WU (or epic) branch | — | hooks; verify-dod **commit** for standalone commits | standalone only: `test/v1` (unit), `doc/v1` when it touches docs |

- **Ids**: epic `E-<CODE>` (short uppercase abbreviation of its slug, e.g.
  `E-INFRA`); WU numbered within its epic, `E-<CODE>.W<m>` (numeric only).
  Git is the registry — no tracker file.
- **Branch names carry the target.** A tip named `wu/<e>/<slug>` is measured
  against `epic/<e>`; anything else against `branches.main`, unless `--target R`
  is passed. Keep the naming exact or pass `--target` everywhere. The `epic/`
  and `wu/` prefixes are `branches.epic_prefix` / `branches.wu_prefix` in
  `.keel/config.toml`; this skill's examples use the defaults.
- **Main is merge-only**: the pre-commit hook refuses every non-merge commit on
  `branches.main`. All work, docs included, starts on an `epic/*` or `wu/*`
  branch.
- **Always `--no-ff`** (`post-checkout` heals `merge.ff=false`) so every gated
  tip stays addressable from main.
- **Collapse rule**: a small epic may be one WU, a small WU one commit — every
  gate is still met explicitly, with all notes written. A one-WU epic may
  combine its plans (`data-artifact="epic-plan wu-plan"`) and its results.
- **Code vs docs/infra**: a path is code when it starts with one of
  `code_paths` (every path, when the list is empty). A diff touching no code path
  is **exempt** from test, cr and doc evidence — never from the human sign-off
  (check 8) or the epic's tracked-doc layout, and never when it archives a T3 doc.
  When `infra_paths` (globs) is set, only paths matching it count as infra: a
  diff is exempt only if every path is outside `code_paths` and matches it.

## 2 · Lifecycle and sign order

Every binding pins what it was made against: `cr/v1` pins the code diff hash,
`mr/v1` the full diff hash and (when `[mr] report` asks for one) the result
report's bytes, every note its tip.
**Any tree change after a binding orphans it**, so the order is not a
preference:

| # | Step | Command |
|---|---|---|
| 1 | the work, its tests, its docs, the result report (skill `keel-plan`) — all committed | — |
| 2 | test + doc notes on the tip | `keel notes write test < judgment.json` · `keel notes write doc` |
| 3 | preflight, then the code review → `cr/v1` PASS on the code paths | `keel preflight wu <tip>` · skill `keel-code-review` |
| 4 | check: red **only** on `8·manual-review` | `keel verify-dod wu <tip>` |
| 5 | **the human** signs last | `keel mr confirm wu <tip> --as <id>` |
| 6 | the human merges | `git switch epic/<slug> && git merge --no-ff wu/<slug>/<x>` |

`keel mr confirm` refuses while checks 1–7 are red on the tip, before any
signing — the human is asked once, at the right moment. Notes are written on
`HEAD`: run `notes write` from the work branch with the tip checked out.

**`mr` is manual review — an agent never runs `keel mr confirm` or
`keel mr commit`.** Stage the exact command (always `--as <id>`, and
`--report <path>` when more than one doc carries the level's result role), print
it, stop.

**The result report** is the doc whose root `data-artifact` carries the level's
role (`wu-result`, `epic-result`; a collapsed doc counts for both), whatever its
file name. `[mr] report` in the target's committed `.keel/config.toml` decides
whether the sign-off must attest one: `required` (default) · `optional` ·
`none`. `keel mr confirm` finds it — a WU's among the html docs under
`docs.root` changed in the range; an epic's also among the untracked ones,
`docs/.archive/` included — and refuses, writing nothing, when a required one is
missing, when several match (it lists them: pass `--report`), or on `--report`
under `none`.

**Epic close**: merge main into the epic if it moved → distill (skill
`distill`) → archive commit (`git rm` every T3 doc, keep the files in
`docs/.archive/`) with `keel notes write doc --distill …` → epic result report
in `docs/.archive/` → epic test note (e2e) → `keel preflight epic <tip>` →
epic-scope review vs main → `keel verify-dod epic <tip>` → the human runs
`keel mr confirm epic <tip> --as <id> --report docs/.archive/<the epic-result doc>`
→ the human merges into main and pushes. (`--report` can be left out only
while the epic's report is the one untracked `epic-result` doc under
`docs.root` — usually a checkout's first epic.)

## 3 · Evidence — five notes, one tip

All JSON in `refs/notes/<kind>`, each carrying the identity block
(`tip`, `base`, `branch`, `committer`, `timestamp`). Hand-writing or copying a
note between tips is a bypass: always use the writer.

| Note | Proves | Written by |
|---|---|---|
| `test/v1` | per layer (`unit`, `integration`, `e2e`): `count`, `covers`, `files`, `languages` — or `waived` | `keel notes write test` reads the judgment on stdin; counts come from the named files at the tip via `tests.adapter` (rust, ts, python, go, regex), never typed |
| `doc/v1` | every changed doc (under `docs.root`, or T0) listed with its tier; archived T3 docs carry `distilled_to` | `keel notes write doc [--distill <t3>=<doc>[,<doc>] \| <t3>=none: <why>]` |
| `cr/v1` | an independent review judged exactly this diff's code paths: `verdict`, `blockers`, `importants`, `code_diff_hash`; optional `walk_waivers` | the `keel-code-review` skill, via `keel notes write cr --verdict PASS\|FAIL` |
| `mr/v1` | a listed human attested this tip: `signer`, `tier`, `change_hash`, `report` + `report_hash` when a report is attested, `sig` when key-backed (payload `keel-mr/v2`; hashes `sha256:<hex>`) | the human: `keel mr confirm` |
| `dod/v1` | the gate ran green here; names the attested `report` + `report_hash`, if any | `keel verify-dod` on a green, non-exempt wu/epic, only |

A sign-off written by a Keel binary older than the `sha256:` hashes no longer
verifies (check 8 names that cause): after upgrading Keel, re-sign every unmerged WU or
epic tip with `keel mr confirm`.

Test judgment (stdin of `keel notes write test`; `files` must be changed in
the range and declare tests; give `files` + `covers`, or `waived`):

```json
{"integration": {"files": ["tests/api.py"], "covers": "list/get endpoints return the stored rows"},
 "unit": {"files": ["tests/test_parse.py"], "covers": "parser edge cases"}}
```

A standalone commit on an epic branch writes its notes with
`--target <sha>^`. Notes sync to the remote on every push (pre-push hook).

## 4 · The gate — `keel verify-dod`

```
keel verify-dod wu <tip> [--target R]   # target from the branch name, else main
keel verify-dod epic <tip> [--target R] # target defaults to main
keel verify-dod commit <sha>            # standalone commit
keel verify-dod hook                    # what pre-merge-commit runs
keel verify-dod range <old> <new>       # what pre-push runs on main/epic updates
keel preflight <wu|epic> <tip>          # the review's first step (0·tree + checks 1-2)
keel check                              # check.lint + check.test, cached by tree content
keel lint                               # check.lint only
```

| # | Check | Levels |
|---|---|---|
| 0 | `0·hooks`: `core.hooksPath` is Keel's (unset or another tool's ⇒ red; fix `keel hooks install`) | preflight, `verify-dod wu\|epic` |
| 0 | `0·tree` (preflight only): checkout is the tip, nothing uncommitted, `keel check` green | preflight |
| 1 | `1·notes`: required notes present, JSON, declaring `<kind>/v1` | all |
| 2 | `2·manifest`: the level's layer (commit ⇒ unit, WU ⇒ integration, epic ⇒ e2e) counted or waived; every note names this tip | all |
| 3 | `3·review`: `cr/v1` verdict PASS | wu, epic |
| 4 | `4·review-base`: review base = current merge-base | wu, epic |
| 5 | `5·diff-binding`: `code_diff_hash` = the code-path diff recomputed now | wu, epic |
| 6 | `6·doc-note`: `doc/v1` lists exactly the changed docs with their `docs.tiers` tier; archived T3 docs carry lineage to tracked T1/T2 docs; at an epic tip nothing T3 (or tier-less) is tracked under `docs.root` | all |
| 7 | `7·walk`: every first-parent seam since the base passes its own gate (merges as WUs, plain commits as standalone); back-merges skipped; octopus refused | epic |
| 8 | `8·manual-review`: a valid `mr/v1` from a signer in this checkout's `.mr-allowed`, bound to tip, base, level, diff and the attested report's bytes (re-hashed whatever the policy); no report under `[mr] report = "required"` is red; signature verified for key-backed signers. Never exempt, never waivable, boundary-only (the walk never re-judges a seam's sign-off) | wu, epic |

Green ⇒ exit 0 and, for a non-exempt wu/epic, the `dod/v1` receipt. Red ⇒ exit 1, one
`FAIL <check> … fix -> …` block per failure, nothing written. Do what the
`fix ->` line says; never bypass.

## 5 · Waivers

- **Test layer**: `{"integration": {"waived": "<why>"}}` replaces a count at any
  level. The gate accepts any non-empty reason; whether it is justified is a
  review finding. **waived** = judged unnecessary, reason recorded ·
  **exempt** = a rule says no evidence is needed (docs/infra-only diffs).
- **Walk seam** (check 7 red on a seam that is safe — e.g. a topology the walk
  misreads): waive that one seam on the record, never `--no-verify` (which skips
  every gate). Edit the epic tip's cr note — `git notes --ref=cr edit <tip>` —
  and add `"walk_waivers": [{"seam": "<sha as printed>", "why": "<why safe>"}]`.
  It rides `cr/v1` because it is a review judgment bound to the reviewed code.
  The waived seam prints as `WAIVED`; check 8 is never waivable.

## 6 · Manual review — the human sign-off

| Level | Required when | Recorded in | Refused at |
|---|---|---|---|
| Commit | the staged diff touches a `rule_bearing` path (default T0 `AGENTS.md` + flat `docs/*.html`) | `Reviewed-By: <tier> <id>` trailer via `keel mr commit -m "<msg>" --as <id>` | commit-msg hook |
| WU | always — attests the `wu-result` report (tracked at the tip) unless `[mr] report = "none"` | `mr/v1` via `keel mr confirm wu` | check 8 |
| Epic | always — attests the `epic-result` report (may be archived, local) unless `[mr] report = "none"` | `mr/v1` via `keel mr confirm epic` | check 8 |

`.mr-allowed` is a **local, untracked** trust store (`email:`, `username:`,
`key: <id> <ssh-pubkey>` lines; accepted tiers in `[mr] tiers`), created per
checkout by `keel mr setup --as <id> [--key <path>]`. Readers refuse it while
tracked. A `key` identity must sign (`ssh-keygen -Y`, verified offline); identity
tiers are a string match. List it in `.gitworktreecopy` to seed new worktrees.
**Accepted limitation**: "signed" means a key or identity listed in this
checkout's roster signed — an agent with filesystem access could add itself, so
the roster is the human's to maintain.

## 7 · Hooks

`keel hooks install` (once per repo; `keel init` runs it) writes the hooks into
`<git-common-dir>/keel-hooks/` — inside the git dir, **never tracked**, shared
by every worktree — and points `core.hooksPath` at that absolute path, so no
branch can ship or merge in a neutered hook. Never commit hooks.

Keel runs alone, never alongside another hook manager. `keel hooks install`
refuses, changing nothing, when `core.hooksPath` (any scope) is another tool's,
or, with `core.hooksPath` unset, when `.git/hooks/` holds anything but `*.sample`
files; it names what it
would replace. `keel hooks install --force` replaces it — the other tool's
hooks stop running, so that is the owner's call, never an agent's. `keel init`
writes its files, then stops on the same refusal (its `-f` is not `--force`).
If another tool later takes or unsets `core.hooksPath`, the gate is silently
off: `keel preflight` and `keel verify-dod wu|epic` are red on `0·hooks`,
`keel check` warns.

| Hook | Enforces |
|---|---|
| `pre-commit` | refuses non-merge commits on `branches.main` (merge-only). Runs no lint — run `keel lint` / `keel check` yourself |
| `commit-msg` | a staged change to a `rule_bearing` path needs a `Reviewed-By` trailer naming a listed signer; merge commits skipped |
| `pre-merge-commit` | `keel verify-dod hook`, judged by the target's **committed** `.keel/config.toml`: into `epic/*` ⇒ wu gate, into main ⇒ epic gate then `keel check` on the merged tree; other targets ungated; a back-merge of landed history into an epic passes; an octopus or an unknowable merge head (`git pull`) is refused — fetch, then `git merge --no-ff <branch>` |
| `pre-push` | `keel check` on every pushed tree (clean HEAD in place, others in a scratch worktree) · `verify-dod range` on main/`epic/*` updates · pushes `refs/notes/*` |
| `post-checkout` | heals `merge.ff=false` · copies `.gitworktreecopy` entries into a new worktree |
| `verify-doc-post` · `verify-doc-stop` | harness hooks (PostToolUse / Stop): `keel verify-doc` on each written doc / every doc at turn end (skill `keel-plan`) |

- **Conflicts**: completing a conflicted merge with `git commit` never runs
  pre-merge-commit (and on main pre-commit refuses it). Resolve conflicts by
  merging the target into the work branch first, so the gated merge is clean.
- **Bypass honesty**: `--no-verify` and fast-forwards skip hooks, but the epic
  walk re-verifies every seam, so smuggled work turns the epic gate red before
  it reaches main. Never amend or rebase a gated branch — it orphans the notes.

## 8 · Testing charter

Every line is written by an AI agent and no human reads the diff before it
lands, so the tests are the reviewer and the executable spec. Untested
behaviour is not guaranteed; every documented quirk becomes a named regression
test.

| Level | Definition | Mandatory test | Done when |
|---|---|---|---|
| Epic | a big, user-visible feature | **e2e** covering the full epic flow | the e2e passes, every WU is done, each documented quirk has a named regression test |
| WU | a slice with one explicit, testable requirement; independently shippable | **integration** test(s) asserting the WU's contract at its real boundary | its requirement sentence is written and an integration test asserts it; new wire contracts are snapshot-locked |
| Commit | the smallest coherent increment | **unit** tests covering all code it adds or changes | lint and tests green, all changed code unit-covered |

**Levels compose, they never substitute**: a green e2e never excuses a missing
WU integration test, nor a WU test missing unit tests. A one-commit WU meets
both obligations, explicitly.

| Layer | Boundary — may touch |
|---|---|
| Unit | one function/type/branch in isolation; fabricated inputs; no sockets or filesystem |
| Integration | the WU's contract at its real boundary — public surface, in-process server, temp-dir filesystem; not the full product |
| E2E | the epic's full flow through the running system on ephemeral ports, against local test dependencies |

The five rules every test obeys:

| Rule | Meaning |
|---|---|
| Deterministic | same input → same result; no sleep-as-synchronization; assert presence of timestamps, never exact values; seed randomness |
| Hermetic | no public network, no shared global files; ephemeral ports; temp dirs, never repo paths |
| Behaviour, not implementation | assert observable contracts so refactors stay green; test names read as specifications |
| Invariant-first | properties that hold for all inputs get a generated-input (property) test, not three hand-picked cases |
| Fast and isolated | each test owns its state and runs in parallel; the unit layer runs in milliseconds |
