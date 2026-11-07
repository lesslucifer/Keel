---
name: project-router
description: Project facts for the router role in this repository (TEMPLATE - fill in the <placeholders>). Use with the Keel router skill when coordinating worker sessions here - who runs what, owner identity, sign-off sequences, what no gate reviews, worker spawn settings.
---

# Project router — TEMPLATE

Written by `keel init` from the Keel router skill's
`references/project-router-template.md`. Load the `router` skill first; this
file adds only this project's facts. Replace every `<placeholder>`, delete
rows that do not apply, and keep it true: when the code or `.keel/config.toml`
disagrees with this file, they win — fix this file.

## Layout
| Thing | Value |
|---|---|
| Main checkout | `<abs path>` (first row of `git worktree list`), branch `<branches.main>`, merge-only |
| Worktrees | `<convention, e.g. ../<repo>-<slug>, one per epic (+ one per parallel WU)>` |
| Remote | `<origin url>` |
| Owner identity | `--as <id>` (from this checkout's `.mr-allowed`; untracked, copied into new worktrees via `.gitworktreecopy`) |
| Shared resources | `<fixed ports, single-instance sockets, real app data dirs>` |

## Who runs what
| Action | Who |
|---|---|
| Plans, code, tests, results, distillation, `keel notes write`, `keel preflight`, `keel verify-dod`, the code-review skill | worker |
| Independent read-only review of what no gate reviews; reading CI runs and git state | router (strongest-model subagent) |
| `keel mr commit` / `keel mr confirm` | **owner only** — the router stages, never runs |
| `git merge --no-ff`, `git push`, tags, `<CI dispatch, secret uploads>` | owner |

## Sign-off sequences
- **WU**: worker gates (notes, preflight, cr PASS, verify-dod red only on `8·manual-review`) → router verifies → owner `keel mr confirm wu <tip> --as <id>` → owner `git switch epic/<slug> && git merge --no-ff <wu branch>` → router verifies → next worker.
- **WU with a rule-bearing doc** (`rule_bearing` in config): only that doc staged → owner `keel mr commit -m $'…' --as <id>` → a fresh worker writes notes and runs the review on the new tip → owner confirms and merges.
- **Epic close**: `<project-specific steps; default per the workflow skill: distill → archive → epic notes → preflight epic → epic review vs main → owner confirm epic --report docs/.archive/<…>-result.html → owner merge into main → owner push>`.

## What no gate reviews
The cr gate covers only `code_paths` (`<list them>`). Before the owner signs a WU touching any of these, run an independent review: `<e.g. CI workflows, build scripts, lockfiles, native/FFI code, release config>`.

## Workers
- Spawn with `<absolute agent CLI path>`, always `--model <model id>`, `--permission-mode <mode>`; names `<project>-<area>-<phase>`; briefs in `<durable briefs dir>`.
- Brief skeleton: worktree/branch/tip and first commands · owner rulings · what to read · the job · hard rules (never run `keel mr`, push, tag; never amend a gated branch; never copy a gate note; report denials verbatim; `<project's machine-safety rules>`) · report to the router's current name: what, doc, full sha, ordered cd-prefixed owner commands, gate state, what had no independent review.

## Never touch
`<real user data dirs, system settings, keys the owner holds alone>`.
