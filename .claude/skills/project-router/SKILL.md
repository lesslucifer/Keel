---
name: project-router
description: Project facts for the router role in this repository (TEMPLATE - fill in the <placeholders>). Use with the keel-router skill when coordinating worker sessions here - who runs what, owner identity, sign-off sequences, what no gate reviews, worker spawn settings.
---

# Project router — TEMPLATE

Written by `keel init` from the keel-router skill's
`references/project-router-template.md`. Load the `keel-router` skill first; this
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
| Plans, code, tests, results, distillation, `keel notes write`, `keel preflight`, `keel verify-dod`, the keel-code-review skill | worker |
| Independent read-only review of what no gate reviews; reading CI runs and git state | router (strongest-model subagent) |
| `keel mr commit` / `keel mr confirm` | **owner only** — the router stages, never runs |
| `git merge --no-ff`, `git push`, tags, `<CI dispatch, secret uploads>` | owner |

## Sign-off sequences
Default per the keel-workflow skill; exact owner commands in the keel-router skill's `references/commands.md`. Record only where this project differs:
- **WU**: `<default: worker gates → router verifies → owner keel mr confirm wu <tip> --as <id> --target epic/<E> → owner merge --no-ff into the epic → next worker>`.
- **WU with a rule-bearing file**: `<default: only that file staged → owner keel mr commit → fresh worker re-gates the new tip → owner confirms and merges>`.
- **Epic close**: `<project-specific steps, or "default">`.

## What no gate reviews
The cr gate covers only `code_paths` (`<list them>`). Before the owner signs a WU touching any of these, run an independent review (checklists: the keel-router skill's `references/review-checklists.md`): `<e.g. CI workflows, build scripts, lockfiles, native/FFI code, release config>`.

## Workers
- Spawn with `<absolute agent CLI path>`, always `--model <model id>`, `--permission-mode <mode>`; names `<project>-<area>-<phase>`; briefs in `<durable briefs dir>`.
- Briefs: the keel-router skill's `references/brief-skeleton.md`; this project adds these hard rules: `<machine-safety rules: temp HOME, held ports, real data dirs>`.

## Never touch
`<real user data dirs, system settings, keys the owner holds alone>`.
