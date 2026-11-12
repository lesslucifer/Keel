---
name: project-router
description: Keel's own project facts for the router role - checkouts and worktrees, owner identity, who runs what, sign-off sequences, what no gate reviews, worker spawn settings. Use with the keel-router skill when coordinating worker sessions in the Keel repository.
---

# Project router — Keel

Filled from `keel init`'s template (the keel-router skill's
`references/project-router-template.md`) by E-ROUTER.W3, 2026-11-08. Load the
`keel-router` skill first; this file adds only Keel's facts. When the code,
`.keel/config.toml` or `git worktree list` disagrees with this file, they win —
fix this file.

## Layout
| Thing | Value |
|---|---|
| Main checkout | `/Users/nguyenvanluong/Documents/proj/misc/Keel` (first row of `git worktree list`), branch `main` (`[branches] main`), merge-only: it stays on `main` and receives only closed epics (DEC-12) |
| Worktrees | siblings of the main checkout, one per epic branch and one per WU branch, each created and removed by the router through the launcher (DEC-46): `scripts/router-worker.sh worktree add epic/<E>` makes `../Keel-<E>` off `main`, `worktree add wu/<E>/<slug>` makes `../Keel-<E>-<slug>` off `epic/<E>`; `worktree remove <branch>` once merged, clean and session-free (the branch stays; it deletes ignored files, so a worker copies `docs/.archive/` out first). A worktree's branch never changes. `.claude/worktrees/` (`[paths] worktrees`) is only for `Agent`-tool isolation worktrees, never WU work |
| Branches | `epic/<E>` and `wu/<E>/<slug>` (`[branches] epic_prefix = "epic/"`, `wu_prefix = "wu/"`); a WU merges `--no-ff` into its epic, the epic `--no-ff` into `main` |
| Remote | `origin` = `https://github.com/lesslucifer/Keel.git` |
| Owner identity | `--as vuluu`; the committed policy is `[mr] tiers = ["username", "key"]` and the committed `.keel/mr-allowed` holds `username: vuluu` (owner ruling 2026-11-12: key-only is deferred, DEC-48). A sign-off is the owner's typed command at a terminal; the agents never run it. The roster arrives in a worktree by checkout; the old untracked `.mr-allowed` files are read by nothing. Only the owner changes the roster (`keel mr setup`, then `keel mr commit`), and a WU that changes it is signed with `--roster-change` |
| Router checkout | the router runs in the checkout holding `scripts/router-worker.sh` (launcher), `scripts/router-watch.sh` (read-only owner-action watch, E-SEED2.W12), `scripts/router-guard.jq` (the one router guard, run fail-closed on every tool by `.claude/settings.json`) and `.claude/agents/router.md`. Since E-ROUTER and E-ROUTERFIX merged into `main` (`0dad93f`), that is the main checkout `/Users/nguyenvanluong/Documents/proj/misc/Keel`, which also holds `.router/` (git-ignored). E-SEED2 merged into `main` (`bfb3eb8`), so `main` holds `router-watch.sh` and the DEC-39 launcher; the E-ROUTER3 launcher reaches `main` when that epic merges |
| Shared resources | one git common dir (`Keel/.git`) for every worktree: the notes refs (`test`, `doc`, `cr`, `mr`, `dod`), `info/exclude`, and `keel-hooks/` (installed: `core.hooksPath` = `/Users/nguyenvanluong/Documents/proj/misc/Keel/.git/keel-hooks`, checked 2026-11-10). A branch is checked out in one worktree only: merge into `epic/<E>` inside the worktree that has it checked out. No ports or app data: Keel is a CLI and its tests run in temp dirs |

Worktrees on 2026-11-10 (snapshot; `git worktree list` wins):

| Path (under `/Users/nguyenvanluong/Documents/proj/misc/`) | Branch | State |
|---|---|---|
| `Keel` | `main` | main checkout, `0dad93f` (E-ROUTERFIX merged); the router's checkout (`.router/` lives here) |
| `Keel-E-SEED2` | `epic/E-SEED2` | E-SEED2 epic |
| `Keel-E-SEED2-w1-release-pipeline` | `wu/E-SEED2/w11-release-hardening` | E-SEED2.W11, merged into the epic |
| `Keel-E-SEED2-w12-owner-watch` | `wu/E-SEED2/w12-owner-watch` | E-SEED2.W12 (owner-action watch) |
| `Keel-E-SEED2-w2-report-binding` | `wu/E-SEED2/w2-report-binding` | E-SEED2 WU |
| `Keel-E-SEED2-w7-opus-policy` | `wu/E-SEED2/w7-opus-policy` | E-SEED2.W7 (DEC-39 worker policy) |

## Who runs what
| Action | Who |
|---|---|
| Plans, code, tests, results, distillation, `keel notes write` (not `cr`), `keel preflight`, `keel verify-dod` | worker (the author) |
| The keel-code-review skill and the `cr` note on an author's tip | a fresh review worker per round, `spawn … --review` in the author's worktree while the author is idle; never the author (DEC-46) |
| Creating and removing worktrees (`scripts/router-worker.sh worktree add|remove <epic/E or wu/E/slug>`) | router |
| Independent read-only review of what no gate reviews (below); reading CI runs and git state beyond the router's whitelist | read-only worker on `opus` at `--effort high`, dispatched by the router |
| Briefs (`<router checkout>/.router/briefs/<name>.md`), spawning via the launcher, whitelisted git/gh/ps reads | router (posture: `.claude/agents/router.md`; no `Agent` tool, so every worker, checker and watcher is a background session) |
| `keel mr commit` / `keel mr confirm` | **owner only** — the router stages, never runs |
| Epic → `main` merge, every `git push` (the epic branch's first push too), tags, release dispatch | owner; no worker pushes (DEC-46) |
| WU → epic merge | owner for gated epics; for E-ROUTER's ungated WUs (owner ruling 2026-11-08) the WU's worker merges `--no-ff` into `epic/E-ROUTER` and does not push |

## Sign-off sequences
Default per the keel-workflow skill; exact owner commands in the keel-router
skill's `references/commands.md` (with `ID=vuluu`). Keel's differences:
- **WU**: default (worker gates → router verifies → owner `keel mr confirm wu <tip> --as vuluu --target epic/<E>` → owner `git merge --no-ff` into the epic → next worker). Always pass `--target epic/<E>` with a sha (commands.md). E-ROUTER exception: its WUs are ungated (no notes, no sign-off); plain commits on `wu/E-ROUTER/<slug>`, merged `--no-ff` into the epic.
- **WU with a rule-bearing file** (`rule_bearing` in `.keel/config.toml`: `AGENTS.md`, `docs/*.html`, `skills/`, `templates/`, `.keel/`, `.claude/skills/`, `.claude/settings.json`, `scripts/router-guard.jq`, `scripts/router-worker.sh`, `scripts/router-watch.sh`, `.claude/agents/`; `.keel/mr-allowed` always): default — only that file staged → owner `keel mr commit … --as vuluu` → fresh worker re-gates the new tip → owner confirms and merges.
- **Epic close**: merge `main` into the epic first (resolve `.claude/settings.json` / `.gitignore` per DEC-12: keep the settings holding the router config, re-run `keel init` on top); distil every T3 doc and archive it out of git (keel-distill skill) so nothing T3 reaches `main`; then owner `keel mr confirm epic <tip> --as vuluu --report docs/.archive/<date>-<slug>-epic-result.html` → `git merge --no-ff epic/<E>` in the main checkout → push. Worked example: E-SEED, signed on `9a8f255`, merged as `e5a04cf`.

## What no gate reviews
The cr gate covers only `code_paths = ["src/", "tests/"]`. Before the owner
signs a WU touching any of these, run an independent review (checklists: the
keel-router skill's `references/review-checklists.md`):
- rule-bearing but outside `code_paths`: `skills/`, `templates/` (both embedded into the `keel` binary by `include_str!`, so they ship), `.keel/`, `.claude/skills/`, `.claude/settings.json` (the router hooks and allowlist), `AGENTS.md`, `docs/*.html`, and since E-ROUTER3.W4 `scripts/router-guard.jq`, `scripts/router-worker.sh`, `scripts/router-watch.sh` (the security path) and `.claude/agents/` (router/worker personas and tool lists);
- neither code nor rule-bearing: `.codex/`, `Cargo.toml` / `Cargo.lock`, `.gitignore`, `.github/` (the dist-generated release workflow, checklist D: a pushed tag matching `**[0-9]+.[0-9]+.[0-9]+*` runs it; a stable tag publishes to GitHub Releases, npm and Homebrew with repo secrets, a prerelease tag (`v0.1.0-rc.1`) only to a GitHub prerelease, and every pull request runs only its `plan` job, which publishes nothing) and `dist-workspace.toml` (its source; `release.yml` is regenerated from it, never hand-edited, DEC-38).

Keel has no `hooks/` or `.claude-plugin/` directory (W7 v2 dropped the plugin;
the git hooks are strings in `src/main.rs`, so the cr gate covers them).

## Workers
- Standard: background sessions (`claude --bg`, E-ROUTER3.W10, DEC-44), `scripts/router-worker.sh spawn <name> <worktree> .router/briefs/<name>.md opus [--effort high]` from the main checkout `/Users/nguyenvanluong/Documents/proj/misc/Keel`. The launcher runs `$HOME/.local/bin/claude` (`/Users/nguyenvanluong/.local/bin/claude`, or `ROUTER_CLAUDE`) with fixed `--bg --agent worker` (always: `.claude/settings.json` sets `"agent": "router"`), `--setting-sources project,local --strict-mcp-config` (no user settings, plugins, connectors or Telegram poller) and the Bash sandbox in `--settings` (writes: the worktree, `Keel/.git` but its `config`, `hooks`, `keel-hooks`, `info`, `~/.cargo` but its `bin`; hosts `htmldoc.wfomobile.com`, `crates.io`, `index.crates.io`, `static.crates.io`, `github.com`; no read of `~/.ssh`, `~/.claude/channels`), a fixed `--allowedTools` grant (`cargo build|test|check|clippy|fmt`, `keel preflight|verify-doc|verify-dod|notes write|lint|check`) and DEC-46's fixed `--disallowedTools` deny (`keel mr|init|hooks`, `cargo run|install|publish|login`, `git push|config|update-ref|symbolic-ref|stash|notes|switch|checkout|rebase|-C|--git-dir|--work-tree`, `git worktree add|remove|move|prune`, `claude`, the launcher; `keel notes write cr` for all but a `--review` worker, which is denied `Edit`, `Write`, `NotebookEdit`, `git add`, `git commit`) on the worker's command line only (DEC-37); no inherited `GIT_*` reaches it; `--permission-mode acceptEdits` by default, or `plan` via a trailing `--permission-mode plan` on spawn (anything else refused; live run 2026-11-12, Claude Code 2.1.288: a plan-mode worker on haiku in the probe clone changed nothing, declined all four writes even when a peer message asked, and reported by `SendMessage`, so the hold is the model obeying plan mode and the harness's own block is untested); model and effort from `.keel/config.toml` `[router]` (DEC-39): currently `opus`, `--effort medium` by default, `--effort high` for design, review and debugging; the launcher reads that policy from the router checkout's HEAD and refuses any other model or effort; there is no resume: a stopped worker is never revived (a plain `claude --bg --resume` drops every launch flag, probe 2026-11-11), the router spawns a fresh one; never into the router's own checkout or the main checkout, a worktree without `.claude/agents/worker.md` or a detached one (the launcher refuses them). `gh` does not work in a worker (keychain, `api.github.com`). The router has no `Agent`-tool fallback (the guard denies it, E-ROUTER close review): checkers are background workers too. Owner actions (commit, sign, merge, push, tag) are watched by the router itself: `scripts/router-watch.sh <kind> <args>` as a background Bash command, `timeout: 7200000` (keel-router `references/worker-ops.md` "Watch"). Workers report to the router by `SendMessage`, and the next task goes to the live worker as a `SendMessage`; `scripts/router-worker.sh stop <name>` retires an idle one (`claude stop`, then `claude rm`), removes its brief copy and prints `drift=<branch>` if its worktree left the spawn's branch. Trust: a `git worktree add` of this repository is trusted at once (the main checkout is trusted); a fresh clone is not, even under `/Users/nguyenvanluong/Documents/proj/misc` (probe 2026-11-11), so workers run only in worktrees of this repository.
- Watchdog (E-ROUTER3.W9, DEC-45): `scripts/router-worker.sh watchdog start <your-name>` once the first worker of a fleet is `RUNNING`, `scripts/router-worker.sh watchdog stop <your-name>` when the last one is retired; it runs on `[router] watchdog_model` (`haiku`) in the router checkout as session `watchdog-<your-name>` and reports every 5 minutes. `<your-name>` is your session's name as `ListAgents` shows it (`[a-z0-9][a-z0-9-]{0,31}`).
- Names: lowercase `<epic>-<area>-<phase>`, e.g. `e-router-w2-kickoff`, `e-seed-close-gates` (the launcher requires `[a-z0-9][a-z0-9-]{0,40}`).
- Briefs: `Keel/.router/briefs/` in the main checkout (git-ignored, durable; run snapshots in `.router/runs/`; a worker's transcript is `~/.claude/projects/*/<session>.jsonl`); skeleton in the keel-router skill's `references/brief-skeleton.md`. Keel's hard rules: never run `keel init` or `keel hooks install` in a real checkout to test them (they write into the shared git common dir) — use a scratch clone; never push; never edit `.keel/mr-allowed`.
- Live probes of the launcher (W8 point 17): in the probe clone `/Users/nguyenvanluong/Documents/proj/misc/Keel-probe` (trusted once by the owner, reused for the epic), never in a worktree of this repository, since a probe writes the git dir. Per run: fetch the WU tip from this repository, check it out detached there, commit a `[router]` policy with a Haiku worker model, `keel hooks install` there, add two worktrees of the clone (router and worker). Whoever orchestrates the WU runs it; the router cannot.

## Never touch
The signer roster `.keel/mr-allowed`; `~/.claude/settings.json` and other user-level
settings; the push channel's token and access files; the owner's ssh keys.
