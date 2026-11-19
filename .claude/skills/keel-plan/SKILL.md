---
name: keel-plan
description: How to write the four governed docs of Keel's workflow - epic plan, WU plan, WU result report, epic result report - each a short htmldoc page for the owner plus a YAML data file beside it that keel verify-doc checks at authoring time, field by field, with every page under its word cap. Use before writing or editing any plan or result doc under docs/work/, when a verify-doc hook reports a red doc, or when collapsing a one-WU epic's docs.
---

# Plans and result reports

Every epic produces two governed docs and every WU two more. Each one is a
**pair**: a page for the owner and a data file for agents and checks.

- **The page**, `docs/work/YYYY-MM-DD-<slug>-plan.html` (or `…-result.html`):
  HTML via the **htmldoc** skill, short and plain, written by skill
  `keel-write`. It carries no checklist markup; the owner reads, comments on
  and signs it.
- **The data file**, the same name ending `.yaml`, beside it: the checklist as
  YAML fields, plus any exact facts an agent needs (files, functions, tests,
  evidence). `keel verify-doc` checks it.

A pair is one doc: **T3** (skill `keel-distill`), distilled and archived
together. The checklist constrains **substance** (fields that must exist),
never the page's **form**. Worked example: skill `keel-write`'s
`references/example.html` and `references/example.yaml`, a ready epic plan
pair.

## 1 · The four outputs

The data file's `artifact` field says which doc it is:

| Output | `artifact` | Written |
|---|---|---|
| Epic plan | `epic-plan` | before the epic branch carries any WU work; holds only a *summary* per WU |
| WU plan | `wu-plan` | its own pair, before that WU's code |
| WU result report | `wu-result` | at the WU's completion, before its merge; tracked at the WU tip — `keel mr confirm wu` finds it through its data file and attests the page's bytes (unless `[mr] report = "none"`) |
| Epic result report | `epic-result` | at epic close, in `docs/.archive/` (untracked, the data file beside its page) — `keel mr confirm epic --report <page>` names it and attests the page's bytes (unless `[mr] report = "none"`) |

**One-part epic**: a one-WU epic may combine the plans
(`artifact: [epic-plan, wu-plan]`) and the results
(`artifact: [epic-result, wu-result]`). The data file holds the union of both
roles' fields, a shared field once, and the page takes the epic's shape and
cap. The doc is the epic's: its `id` is the epic id, and it has no `epic`
field. No other roles share a doc.

## 2 · The data file

One YAML document, a mapping of fields. Write each value out: anchors and
aliases (`&name`, `*name`) are refused.

The fields every role knows (each required unless its row says otherwise):

| Field | Holds |
|---|---|
| `artifact` | the role (§1); a list for a one-part epic |
| `status` | `draft` while authoring — the whole pair is silent; `ready` — checked on every write. Flipping back to draft to silence a red doc is visible to every reviewer |
| `id` | the epic id (`E-CAP`), or for a WU role the WU id (`E-CAP.W2`); a one-part epic's doc takes the epic id |
| `epic` | for a WU role only (never a one-part epic's doc): its epic's id; `id` must be `<epic>.W<n>` |
| `title` | the page's title, as text |
| `date` | a quoted string, `"YYYY-MM-DD"` (a bare date is read as a date, not text) |
| `page` | the page's file name, beside the data file |
| `owner-changes` | the today/after table: a list of `when`, `today`, `after`. Required in an epic plan, optional elsewhere |
| `details` | free: anything else an agent needs. Never checked |

Any other field the role does not list is red, so a misspelt field is caught
(`unknown field "expcted"` next to `missing field "expected"`). A text field
must not be empty; "none" is an answer where the field says so.

## 3 · The fields per role

**Epic plan** — `intent` (what user-visible change, and why now) ·
`owner-changes` · `use-cases` (the `UC-*` contracts delivered or changed; a
process epic touching none says so) · `expected` (observable acceptance a
reader who wasn't there can check) · `non-goals` · `wu-summaries` · `e2e-plan`
(the flow(s) proving the epic; a waiver intent stated here) ·
`distillation-plan` (which T2/T1 docs absorb the epic's truth) ·
`open-questions` (each with a proposed default, or "none").

`wu-summaries` is a list, one entry per WU, each with five fields: `wu` (its
id, `<epic>.W<n>`, listed once), `wu-summary`, `wu-maps-to`, `wu-expected`,
`wu-deps` ("none", or the ids of other WUs in this list).

**WU plan** — `requirement` (one testable sentence) · `maps-to` (epic and
UC/requirement, consistent with its summary entry) · `expected` (observable
outcome of this WU alone) · `test-plan`, a mapping with one entry per level:
`unit`, `integration`, `e2e` — the tests, or "none: <why>" for a waiver;
other keys under it are free.

**WU result** — `outcome` (delivered / deviated / dropped against the
requirement) · `deviations` (every difference from the plan, with why, or
"none") · `evidence` (tests added, docs touched, commits; the review runs after
the report is committed, so its verdict and nits live on the cr note, not here)
· `distillation` (what went to T2/T1, or "nothing citable") · `follow-ups` (or
"none").

**Epic result** — `outcome` (against intent, use cases, expected) ·
`wu-rollup` (each planned WU's outcome; unplanned work accounted for) ·
`deviations` · `evidence` (e2e outcome, review verdicts, landed tips) ·
`distillation` (the T2/T1 docs now carrying the truth, checked against the
distillation plan; plans and reports archived) · `follow-ups`.

Every follow-up, in either report, names the WU, epic or backlog row that
takes it (skill `keel-workflow` §2).

## 4 · The page

The page holds only what the owner needs to approve or sign. Each role has a
shape and a word cap for a ready page (counted as a reader sees the text of
`<main>`, else `<body>`: a word has a letter or digit, inline tags like
`<code>` join the text around them, block tags separate it; diagram source
counts):

| Page | In order | Cap |
|---|---|---|
| Epic plan | the problem · what changes for you (a today/after table) · an example in steps, when the flow is new · terms · **the parts, in build order** (a diagram and a table: what each builds, what you get, after which part) · what you decide · how you'll know it worked · not in this epic | 700; the **top** at most 300 |
| WU plan | the problem · what you get · how it works, or how you approve · what it makes · what you decide · how you'll know it worked | 500 |
| WU result | what happened · a checklist of the plan's "how you'll know", each met or not · an example of the main case, run for real, in steps · differences from the plan · what is left, and who takes it | 500 |
| Epic result | the same, plus one line per part | 700 |

A one-part epic's page takes the epic cap. The epic plan's **top** runs from
the start of the page to the heading of its parts, which carries
`id="work"`: `<h2 id="work">The parts, in build order</h2>`. With no such
heading the whole page is the top.

The requirement, maps-to, file lists, test plans per level, function names
and evidence stay off the page: they live in the data file. A plan carries an
**open-points table** (question · answer first · why), so the owner can
review asynchronously through htmldoc comments while work proceeds on the
stated defaults.

## 5 · The verification loop — `keel verify-doc`

```
keel verify-doc <path> [<path>…]   # a page or a data file names its pair · exit 0 green / draft · exit 1 red
keel verify-doc                    # every page and data file under docs.root, tracked or untracked, that git does not ignore
```

`docs/.archive/` is ignored, so check an epic result there by its path.

It checks exactly this, nothing cleverer. **The data file**: every field its
role requires, present and not empty; no unknown field; `artifact` a known
role or an allowed one-part pair; `status` draft or ready; `date` a quoted
date; `page` naming the page beside it; a WU's `id` under its `epic`; each
`wu-summaries` entry complete, its `wu` under the epic, listed once, its
`wu-deps` naming WUs of the plan; each `test-plan` level given. **The page**,
when the pair is not a draft: its word cap and, for an epic plan, its top;
**every relative link (`<a href>`) resolves** (absolute URLs, `mailto:`, root-relative
paths and fragments are not checked); and **every Mermaid flowchart node and
edge label is in double quotes** (node shapes like `A["…"]`, `Q{"…"}`, and
edge labels `-->|"…"|`, `-- "…" -->`, spaced or not). The label check is a
**best-effort lint** for htmldoc output: refusing a valid diagram, or a fix
hint that changes the diagram, is a bug; a missed label is a known limit.
Known misses: a bracketless subgraph title, `N@{ … }` shapes, other diagram
kinds, a `>…]` shape after an id ending in a non-ASCII or punctuation
character, and a statement after a `;` on a `classDef`, `style`, `linkStyle`,
`direction` or `accTitle` line. A target that was archived is cited as plain
text marked archived, never linked into `docs/.archive/`.

A page with no data file is a plain page: links and labels, and the cap
`[docs.caps]` in `.keel/config.toml` sets for its path, if any. It is never a
plan or result, whatever it carries.

The trigger is the write, not a review or a merge. The host runs
`keel verify-doc <path>` after every first-party write or edit of a page or a
data file under `docs.root`, and `keel verify-doc` over all docs at turn end —
through a shell hook where the host has one, through a plugin where it has
none. A red doc comes straight back with its problem lines: the write's result
fails, and at turn end the problems steer the writer into another step, so it
is fixed in the same turn. The bytes have already landed; no post-write check
can prevent that. A review or a merge is too late and no substitute. Drafts
iterate in silence; flipping to `ready` is the checkpoint. Manual runs work
anytime.

This is the only place a document's **form** is checked. The merge reads doc
**facts** instead: every changed doc listed in `doc/v1` (a pair counts once),
and the result page's bytes matching the human's `mr/v1` signature (skill
`keel-workflow`, checks 6 and 8). The sign-off finds a result report through
its data file — the page whose data file's `artifact` carries the level's
result role — never by file name and never by a page with no data file;
`--report` names the page, never the `.yaml`. A typed `--report` is taken as
named (only a `.yaml` is refused): the owner names the page whose data file
carries the level's result role. The search, the approval path and
`keel notes write report` check the data file. Depth of content is judgment —
the owner's comment round and the reviews read these docs.

## 6 · Writing one

1. Decide the tier first — a plan or result is T3, so `docs/work/`.
2. Write the data file first, with `status: draft`: every field its role
   requires, exact facts under `details`.
3. Write the page per the htmldoc skill (mint the id, skeleton, snapshot), in
   its role's shape, by skill `keel-write`. Set `page` to its file name.
4. Run that skill's self-check: a file list, function name, test list or tag
   on the page moves to the data file. Set `status: ready`; fix whatever
   `keel verify-doc` reports, on either file.
5. Commit both files together on the work branch: a WU plan before the WU's
   code, a WU result before its notes (it is part of the diff the notes and
   the signature bind).
