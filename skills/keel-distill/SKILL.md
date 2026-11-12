---
name: keel-distill
description: Keel's documentation tiers (T0-T4) and the distillation process - before a T3 working doc (plan, result, scratch) is archived out of git, absorb its durable truth into the right T1/T2 doc, then git rm it, record the lineage with keel notes write doc --distill, and verify with keel verify-dod. Use when deciding where a new doc goes, when a WU or epic finishes (done = distilled), at epic close, or when check 6·doc-note reports an undistilled or still-tracked T3 doc.
---

# Documentation tiers and distillation

**Prime directive**: source code (T4) is the truth; docs carry only what code
cannot say — intent, rationale, behaviour contracts, the map of the system's
shape. Every fact lives at exactly one tier; elsewhere, link. When a doc and the
code disagree, **code wins** — the session that finds the conflict fixes the doc
in the same session. Docs are for a person first and exact for an agent; no
work estimates.

## 1 · The tiers

Tiers are assigned by path, from `[docs.tiers]` in `.keel/config.toml`
(globs; T3 is matched first so a broad T1 glob never hides a working doc). The
defaults `keel init` writes:

| Tier | Default location | Question | Belongs | Banned |
|---|---|---|---|---|
| T0 | `AGENTS.md` (`CLAUDE.md` if the project keeps one) | What are the fixed rules? | conventions + the tier map, ≤ 200 lines | anything that changes with the code |
| T1 | `docs/*` (flat) | What is the shape of this system? | components, boundaries, data flow, invariants, process standards | per-feature behaviour, anything that changes weekly |
| T2 | `docs/use-cases/` · `docs/decisions/` · `docs/backlog/` | What must it do? · Why is it this way? · What's next? | behaviour contracts · decision records · roadmap and deferred work | implementation detail; restating one KB in another |
| T3 | `docs/work/` (live) · `docs/.archive/` (archived, gitignored) | What is this task figuring out? | plans, results, dead ends — no quality bar | nothing, but nothing here is citable after the task |
| T4 | the code | How does it work, exactly? | everything mechanical | comments restating docs |

- **Decide a doc's tier before creating it** and put it in that tier's folder.
  A plan, spec, test plan or backlog is never T1: T1 must be permanent **and**
  shape-level. Then write it by skill `keel-write`.
- **T0 changes only on an explicit request**, never silently or as a side
  effect, and lands as a `keel mr commit` (it is rule-bearing). T1 flat docs
  are rule-bearing too (`rule_bearing` in config).
- **No code in docs**: path + symbol references only (`src/capture.rs ·
  CaptureSession`); the sole exception is a wire format that defines a contract.
- A doc that matches no tier glob is red at the gate (`6·doc-note`): add a glob
  or move it.

## 2 · T2 knowledge bases

Each KB directory holds topical files behind exactly one `*-index.html`, a
pure routing table (topic file · id prefix · one-line scope · status) with no
knowledge of its own. Read **index-first**: open only the relevant topic files,
synthesise back into only those, never bulk-read a KB. Split a topic file when
it outgrows one comfortable read.

| KB | Entries | Lifecycle |
|---|---|---|
| `use-cases/` | `UC-<AREA>-<n>`: trigger, expected behaviour, edge cases, what counts as failure — contract-level | current truth: updated in place, superseded behaviour deleted |
| `decisions/` | `DEC-<n>`: context, options, the choice, consequences | append-only: a reversal marks the old record *superseded by DEC-m* and adds a new one. Check here before "fixing" anything that looks deliberate |
| `backlog/` | roadmap and deferred work | living: rows added when work is deferred, struck when an epic absorbs them |

Ids are never reused or renumbered. Check every live branch before minting a
new DEC number.

## 3 · Distillation — done = distilled

A task is done not when the code lands but when the checklist below has run and
the affected T2 files and their index (rarely T1) reflect the new truth — in the
same session, never "later". A T3 doc neither distilled nor archived is a bug in
the process.

| If yes… | …write it to |
|---|---|
| Behaviour changed or got defined? | the relevant `use-cases/` topic file (+ index row if new) |
| Non-obvious choice made or reversed? | a new `decisions/` record |
| Work deferred or a follow-up found? | `backlog/` |
| Component map or invariant changed? | the T1 architecture doc |
| Repeating process emerged or changed? | the relevant T1 process doc |
| A T0 rule should change? | surface it to the owner and wait for an explicit request; then its own `keel mr commit` |

## 4 · Archiving a T3 doc

WU plans and results stay tracked until the epic closes (a WU result must be at
its WU tip for the sign-off). At epic close — or earlier for a finished scratch
doc:

1. **Distill** per §3. Write the durable content into the T1/T2 targets and
   commit them; each target must be tracked at the tip that archives the doc.
2. **Archive**: keep the file, drop it from git.
   ```sh
   mkdir -p docs/.archive && cp docs/work/<doc>.html docs/.archive/ && git rm -q docs/work/<doc>.html
   git commit -m "docs: archive <doc> (distilled to <targets>)"
   ```
   (`docs/.archive/` is gitignored; history keeps the content. Sessions never
   read the archive — if it still holds something needed, that fact wasn't
   distilled.)
3. **Record the lineage** on the tip, one `--distill` per archived doc:
   ```sh
   keel notes write doc --distill docs/work/<doc>.html=docs/use-cases/<topic>.html,docs/decisions/<dec>.html
   keel notes write doc --distill docs/work/<scratch>.html="none: dead end, nothing citable"
   ```
   The writer refuses a doc it cannot account for: an archived T3 doc with no
   `--distill`, a target that is not a T1/T2 doc tracked at the tip, a
   `--distill` for a path this range did not archive, or an empty `none:` reason.
4. **Verify**: `keel verify-dod commit <sha>` for a standalone archive commit
   on the epic, or `keel verify-dod epic <tip>` at close. Check `6·doc-note`
   also proves that nothing T3 is still tracked at the epic tip. An archive
   commit is never exempt, even though it touches only docs.

## 5 · Reading protocol

T0 arrives with every session; everything else on demand, index-first.

| Task | Read before touching code |
|---|---|
| New feature | T1 architecture → relevant use-case topics → related decisions; a T3 plan if the task spans sessions |
| Bug fix | the use-case topic (what *should* happen) → code; decisions before "fixing" anything deliberate |
| Refactor | T1 architecture (boundaries to keep) → decisions touching the area |
| "How/why does X…?" | index → one topic file → code; never answer from a previous session's memory |
