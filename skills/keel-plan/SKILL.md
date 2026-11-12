---
name: keel-plan
description: How to write the four governed docs of Keel's workflow - epic plan, WU plan, WU result report, epic result report - as htmldoc HTML with the data-artifact / data-status / data-item / data-wu markup that keel verify-doc checks at authoring time. Use before writing or editing any plan or result doc under docs/work/, when a verify-doc hook reports a red doc, or when collapsing a one-WU epic's docs.
---

# Plans and result reports

Every epic produces two governed docs and every WU two more. All are **T3**
working docs (skill `keel-distill`), HTML via the **htmldoc** skill, named
`docs/work/YYYY-MM-DD-<slug>-plan.html` / `…-result.html`, and distilled then
archived like any T3 doc.

The checklist constrains **substance** (items that must exist), never **form**:
a plan may be prose, tables, diagrams or widgets, shaped to its task. There is
no template to copy-paste on purpose. Worked example: skill `keel-write`'s
`references/example.html`, a ready epic plan in this format.

## 1 · The four outputs

| Output | `data-artifact` | Written |
|---|---|---|
| Epic plan | `epic-plan` | before the epic branch carries any WU work; holds only a *summary* per WU |
| WU plan | `wu-plan` | its own doc, before that WU's code |
| WU result report | `wu-result` | at the WU's completion, before its merge; tracked at the WU tip — `keel mr confirm wu` finds it by this role and attests its bytes (unless `[mr] report = "none"`) |
| Epic result report | `epic-result` | at epic close, in `docs/.archive/` (untracked) — `keel mr confirm epic --report <path>` names it (an epic sign-off passes `--report <path>` unless its result doc is committed in the epic's diff) and attests its bytes (unless `[mr] report = "none"`) |

**Collapsed epic**: a one-WU epic may combine the plans
(`data-artifact="epic-plan wu-plan"`) and the results
(`data-artifact="epic-result wu-result"`); the doc must satisfy the union of
both checklists, and a tag both share is satisfied once.

Write every one by skill `keel-write`: short, plain words, the problem first,
details last.

## 2 · The markup contract

Tagging is strict by design — **the tag is the contract**. Content under a
wrong or missing tag is red on purpose. Tag each item as you write it, on
whatever element holds it, in any order.

| Attribute | Where | Meaning |
|---|---|---|
| `data-artifact` | once, on `<html>` | what the doc is (values above, space-separated when combined). No `data-artifact` = ungoverned: no checklist, links still checked |
| `data-status` | on `<html>`, next to `data-artifact` | `draft` while authoring — every check silent, links included. `ready` = verified on every write. Always set it: with no `data-status` the checklist is never run (only links are). Flipping back to draft to silence a red doc is visible to every reviewer |
| `data-item` | the element holding a checklist item | exactly one tag from the tables below, with non-empty text inside |
| `data-wu` | epic plan only, inside `wu-summaries` | one block per WU, value = its id (`E-CAP.W2`), each holding the four `wu-*` sub-items |
| `data-c` | any block | htmldoc's comment anchor (stable id); not checked by Keel |

```html
<html lang="en" data-htmldoc-id="…" data-artifact="wu-plan" data-status="ready">
…
<p data-item="requirement">One explicit, testable sentence.</p>
```

## 3 · The checklists

**Epic plan** — `intent` (one requirement paragraph: what user-visible change,
and why now) · `use-cases` (the `UC-*` contracts delivered or changed; a
process epic touching none says so) · `expected` (observable acceptance a
reader who wasn't there can check) · `non-goals` · `wu-summaries` (one
`data-wu` block per WU with `wu-summary`, `wu-maps-to`, `wu-expected`,
`wu-deps` — "none" when none) · `e2e-plan` (the flow(s) proving the epic; a
waiver intent stated here) · `distillation-plan` (which T2/T1 docs absorb the
epic's truth) · `open-questions` (each with a proposed default, or "none").

**WU plan** — `requirement` (one testable sentence) · `maps-to` (epic and
UC/requirement, consistent with its summary block) · `expected` (observable
outcome of this WU alone) · `test-plan` (concrete per level; waivers named with
reasons).

**WU result** — `outcome` (delivered / deviated / dropped against the
requirement) · `deviations` (every difference from the plan, with why, or
"none") · `evidence` (tests added, docs touched, commits; the review runs after
the report is committed, so its verdict and nits live on the cr note, not here) · `distillation` (what went to T2/T1, or
"nothing citable") · `follow-ups` (or "none").

**Epic result** — `outcome` (against intent, use cases, expected) ·
`wu-rollup` (each planned WU's outcome; unplanned work accounted for) ·
`deviations` · `evidence` (e2e outcome, review verdicts, landed tips) ·
`distillation` (the T2/T1 docs now carrying the truth, checked against the
distillation plan; plans and reports archived) · `follow-ups`.

Beyond the checklist, a plan carries whatever the task needs. The convention in
Keel's own plans: an **open-points table** (`#` · question · answer · why), so
the owner can review asynchronously through htmldoc comments while work
proceeds on the stated defaults. The order of the sections is skill
`keel-write`'s: problem, what changes for the reader, a real case in steps,
terms, the work, open points, details last.

## 4 · The verification loop — `keel verify-doc`

```
keel verify-doc <path> [<path>…]   # exit 0 green / draft / ungoverned · exit 1 red
keel verify-doc                    # every .html under docs.root (tracked or untracked)
```

It checks exactly this, nothing cleverer: every checklist item present with
non-empty content; an epic plan has ≥ 1 `data-wu` block, each with its four
sub-items filled; **no unknown tags** (the typo guard reports
`unknown tag data-item="expcted"` next to `missing item "expected"`); an
unknown `data-artifact` is red; and, on every non-draft doc, governed or not,
**every relative `href` resolves** (absolute URLs, `mailto:`, root-relative
paths and fragments are not checked). A target that was archived is cited as
plain text marked archived, never linked into `docs/.archive/`.

The trigger is the harness, not the agent and not git: the
`verify-doc-post` hook runs it on every written `.html` (red ⇒ problems fed
back, fix the doc now) and `verify-doc-stop` runs it over all docs at turn end
(red ⇒ the turn is refused). Drafts iterate in silence; flipping to `ready` is
the checkpoint. Manual runs work anytime.

This is the only place a document's **form** is checked. The merge reads doc
**facts** instead: every changed doc listed in `doc/v1`, and the result report's
bytes matching the human's `mr/v1` signature (skill `keel-workflow`, checks 6 and 8).
The sign-off finds a result report by its `data-artifact` role, not its file
name, so the role is what makes it the report.
Depth of content is judgment — the owner's comment round and the reviews read
these docs.

## 5 · Writing one

1. Decide the tier first — a plan or result is T3, so `docs/work/`.
2. Create it per the htmldoc skill (mint the id, skeleton, snapshot), with
   `data-artifact` and `data-status="draft"`.
3. Write the content by skill `keel-write`, tagging each checklist item as you
   go.
4. Run that skill's self-check. Flip `data-status="ready"`; fix whatever
   `keel verify-doc` reports.
5. Commit it on the work branch: a WU plan before the WU's code, a WU result
   before its notes (it is part of the diff the notes and the signature bind).
