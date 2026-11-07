# Project rules (T0)

Written by `keel init`. Add the project section (stack, layout, commands) at
the top; keep this file ≤ 200 lines. It changes only on an explicit request,
as a `keel mr commit` (it is rule-bearing).

## AI-first

All code is written by AI agents; no human reads diffs before they land.
Design docs, plans and processes for AI readers and executors. Never produce
work estimates (effort, timelines, sizing).

## Coding conventions

- Write self-documenting code. Avoid comments unless essential: names and
  structure carry intent; a comment earns its place only when the *why* cannot
  be expressed in code.

## Testing (mandatory — no change lands without its test)

Skill `workflow` §8. **Epic → e2e** (full flow) · **Working Unit →
integration** (its bounded contract) · **Commit → unit** (all code it adds or
changes) — additive, never substitutes. Tests are deterministic and hermetic.

## Workflow (mandatory — how work lands)

Skill `workflow`. **Epic → Working Unit → commit**; branches `epic/<slug>` off
main, `wu/<epic-slug>/<slug>` off its epic; main is merge-only. Ids: epic
`E-<CODE>`, WU `E-<CODE>.W<m>`. Plan before code (skill `plan`).

**The merge is the gate**: always `--no-ff`; hooks run `keel verify-dod` and a
red gate refuses the merge — never bypass. Before a merge: `test/v1` + `doc/v1`
notes on the tip (`keel notes write`), `keel preflight`, then the
**code-review** skill → `cr/v1` PASS (never judge your own diff). Never amend or
rebase a gated branch. Policy: `.keel/config.toml`.

**`mr` is manual review — never run it yourself.** Stage the full command,
print it, stop: `keel mr confirm wu|epic <tip> --as <id>` (WU/epic tip) ·
`keel mr commit -m "<msg>" --as <id>` (rule-bearing commit).

## Documentation

All project docs are HTML via the **htmldoc** skill, never markdown (this file
and skill files are infrastructure). Tiers (skill `distill`): **T0** this file
· **T1** `docs/*` shape + process · **T2** `docs/use-cases/` (what),
`docs/decisions/` (why), `docs/backlog/` (next) — index-first · **T3**
`docs/work/` scratch, distill then archive · **T4** code, the truth. Decide a
doc's tier before creating it. One fact per tier; code wins conflicts; never
copy code into docs. **Done = distilled**, in the same session.
