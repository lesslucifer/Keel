# Worker operations (Claude Code; shell recipes are macOS/zsh-first)

Two worker modes. Pick per SKILL.md "Workers"; both need an explicit model and
a brief that says "ONE non-stop run, no mid-run questions". Tool names below
were checked on 2026-11-08 against Claude Code 2.1.288 (`Agent`, `SendMessage`,
`TaskStop`, `Monitor`, `ToolSearch` load; `CronCreate` and `ListAgents` did not
load via `ToolSearch` from a subagent session — re-check in yours with
`ToolSearch` before relying on any of them).

## Worktree for a WU (either mode)

A worker, never the router, creates it as the brief's first step:
`git worktree add ../<Repo>-<epic-slug>-<wu-slug> -b <wu_prefix><epic>/<wu-slug> <epic_prefix><epic>`
(prefixes from `.keel/config.toml` `[branches]`). Don't use the `Agent` tool's
`isolation: "worktree"` for WU work: it makes its own `worktree-agent-*` branch,
and E-SEED's first W7 packaging work ended up stranded on such a branch
(`worktree-agent-a6381e6d3c5019ecc`), sharing no history with `epic/E-SEED`.

## Mode 1: in-session subagent (`Agent` + `SendMessage`)

For short one-shot read-only checks, reviews and watches; also the stopgap when
the headless launcher has an open defect (below).
- **Spawn**: `Agent` with `name` (`<epic>-<purpose>`, e.g. `e-router-fix-verify`;
  the name is its address), `model` set explicitly (`opus|sonnet|haiku|fable`),
  `description`, and `prompt` = the full brief (worktree, branch, tip, job, hard
  rules, report protocol). It runs in the background in this session's
  permission mode.
- **Alive**: it is listed in the session's agent roster (addressable via
  `SendMessage`) and you get a notification when it finishes. Don't poll; never
  "are you done?" messages.
- **Report**: the worker ends with `SubagentHandback`; the report reaches only
  you, not the owner. Digest it; never forward it.
- **Follow-up**: `SendMessage({to: "<name>", message})` — a send to a finished
  agent resumes it from its transcript (use for a fix round only when keeping
  its context is the point; otherwise a fresh worker per phase).
- **Retire**: a finished subagent holds no process; nothing to retire. A running
  one stops with `TaskStop` (`task_id` = its name) — only when idle-and-clean
  rules allow, since it cuts the turn. Its worktree is removed by a worker after
  the merge.
- **Limits**: dies with the router session (reboot, exit); not visible to `ps`;
  all of them share the router's permission mode. Say so when it is used as a
  stopgap so it isn't mistaken for the standard.

## Mode 2: headless `claude -p` via the launcher (standard)

The router never types `claude`. A committed launcher validates every argument,
passes a fixed prompt naming a brief file, and fixed flags
(`--agent worker --permission-mode acceptEdits --max-budget-usd 25`). Keel's is
`scripts/router-worker.sh` (on `epic/E-ROUTER`); the project-router skill names
a project's own. `ROOT` = the checkout holding the launcher; its `.router/` is
git-ignored and durable (`briefs/`, `runs/`, `out/`), unlike `/tmp`.

Prerequisites: the router runs in the checkout that holds the launcher (the
hook allows the relative `scripts/router-worker.sh` only); `claude` at
`$HOME/.local/bin/claude` or `ROUTER_CLAUDE`; the target worktree is a
registered `git worktree` other than the router's own checkout and the main
checkout (the launcher refuses both); the target's branch carries
`.claude/agents/worker.md` (open issue 2).

- **Brief**: a worker writes it to `<ROOT>/.router/briefs/<name>.md` (regular
  file, no symlink); you `Read` it before spawning.
- **Spawn**: `scripts/router-worker.sh spawn <name> <worktree> .router/briefs/<name>.md <model>`
  (`<name>` matches `[a-z0-9][a-z0-9-]{0,40}`; model `opus|sonnet|haiku|fable`).
  Prints `RUNNING name=… pid=… session=<uuid> out=… snapshot=… sha256=… worktree=…`,
  or `DEAD name=… exit=<code> …` plus the output tail. Record the session uuid.
  `Read` the snapshot and confirm it is the brief you reviewed. Announce the
  worker only after `RUNNING`.
- **Alive**: `ps aux` and look for the pid in `.router/out/<name>.pid` (`Read`)
  or `-n <name>`; the args show `--model`.
- **Report**: when the process is gone, `Read` `.router/out/<name>.out`; the
  final message is the worker's report.
- **Follow-up**: a worker writes `.router/briefs/<name>-r<n>.md`; then
  `scripts/router-worker.sh resume <name> <session-uuid> <same worktree> .router/briefs/<name>-r<n>.md <model>`
  (the launcher refuses a different worktree than the spawn's).
- **Retire**: none needed — a `-p` run exits when its run ends. A hung or
  mid-turn one needs the owner (or an owner-approved worker) to
  `kill <pid>`; the router has no `kill` (lessons D2).

**Headless: open issues (2026-11-08) — check each is fixed before relying on
this mode; until then use Mode 1 for real work:**
1. *Brief not readable.* The snapshot sits under `<ROOT>/.router/runs/…`,
   outside the worker's worktree; a headless worker cannot approve that read
   (run `persona-fix`: `Read` and `cat` both refused, no work done). The owner
   is applying the fix (the worker suggested `--add-dir` on the run dir, or
   copying the brief into the worktree).
2. *Persona missing.* `--agent worker` fails ("--agent 'worker' not found") in
   a worktree whose branch does not track `.claude/agents/worker.md` (run
   `smoketest` into `Keel-E-SEED`). Fix in flight on
   `wu/E-ROUTER/worker-persona-copy`.
3. *Unverified*: whether a worker's Bash outside its allowlist stalls under
   `acceptEdits` (it cannot answer a prompt). If it does, widen the worker's
   mode or allowlist, never the router's.

## Watch every staged owner action

Under the router posture you don't run shell loops; a watch worker (Mode 1,
cheap explicit model) runs one background `until <probe>; do sleep N; done;
echo <EVENT>` per action (no inner `&`, no `( … ) &`, never a foreground
`sleep`) and reports when it fires. `Monitor` is for a stream of events (it
expires after at most 30 min and can replay handled events: re-arm, dedupe).
Examples from Keel's 2026-11-08 router session: `e-router-push-watch` watched a
push; `w7v2-comment-check` / `w10-comment-check` checked plan-doc comments.

| Owner action | Probe succeeds when |
|---|---|
| signs a WU/epic (`keel mr confirm`) | `[ -n "$(git log -1 --notes=mr --format=%N <tip> 2>/dev/null)" ]` (the router may run the inner `git log` itself: whitelisted) |
| gate green after signing | `keel verify-dod <wu\|epic> <tip>` exits 0 (writes the `dod` receipt) |
| commits | `[ "$(git rev-parse <branch>)" != <old tip> ]` |
| merges | `git merge-base --is-ancestor <tip> <target>` and no `.git/MERGE_HEAD` |
| pushes · tags | `[ "$(git ls-remote origin refs/heads/<b> \| cut -f1)" = <tip> ]` · `[ -n "$(git ls-remote --tags origin <tag>)" ]` |
| publishes a draft | `[ "$(gh release view <tag> --json isDraft -q .isDraft)" = false ]` |
| comments on / resolves an htmldoc doc | open count ≠ last seen (below) |

htmldoc open-comment count (`SERVER` per the htmldoc skill, `<id>` = the doc's
`data-htmldoc-id`; checked against the E-ROUTER plan, id
`6aeede8d9f34851f0dfb0322`, on 2026-11-08):
```sh
until n=$(curl -fsS "$SERVER/api/docs/<id>/comments" | jq '[.[]|select(.status=="open")]|length') && [ "$n" != <last> ]; do sleep 120; done; echo "COMMENTS open=$n"
```
CI runs:
```sh
until [ "$(gh run view <id> --json status -q .status)" = "completed" ]; do sleep 120; done
gh run view <id> --json conclusion,jobs -q '"run: \(.conclusion)", (.jobs[]|"\(.name): \(.conclusion)")'
until id=$(gh run list --workflow <wf>.yml --branch <br> --limit 1 --json databaseId,status,headSha \
  -q '.[0] | select(.status=="completed" and (.headSha|startswith("<sha8>"))) | .databaseId') && [ -n "$id" ]; do sleep 45; done
echo "RUN $id"; gh run view $id --json conclusion,jobs -q '.jobs[]|"\(.name): \(.conclusion) · failed: \([.steps[]|select(.conclusion=="failure")|.name]|join(" | "))"'
```

## Heartbeat (every 10 min)

`/loop 10m <prompt>` (the `loop` skill). Trawler's recipe used `CronCreate`;
use it only if `ToolSearch` loads it in your session. Self-contained prompt:
"Router heartbeat: 1 workers: headless (`ps aux` names + `--model` vs expected;
dead → relaunch from its brief, it keeps and checks uncommitted work; confirm
RUNNING) and in-session (roster; finished but unrelayed reports); stalled (tip
and tree unchanged 3 beats, no report) → `SendMessage` a ping or resume note,
still silent next beat → fresh worker. 2 worktrees: branch, tip, notes on tip
(`git log -1 --notes=<ref> --format=%N`). 3 pending owner actions, runs, doc
comments (relay to a worker), unrelayed reports; re-arm missing watchers.
4 advance anything unblocked (next WU, step-between worker, stage commands).
5 changed or needs the owner → terminal + push ending `What you need to do
now:`; else one line. Never run reserved commands; stay inside the posture."

## Read failures / third-party source (a read-only worker runs these)
```sh
gh run view <id> --log-failed | sed 's/\x1b\[[0-9;]*[mK]//g' > "$S/run-<id>.log"; grep -a -n -E '(error|warning)(\[[A-Za-z0-9_]+\])?:|could not compile|##\[error' "$S/run-<id>.log" | head -40
R=$(gh repo view --json nameWithOwner -q .nameWithOwner); gh api repos/$R/actions/jobs/<job-id>/logs > "$S/job.log"
gh api "repos/<owner>/<repo>/contents/<path>?ref=<tag>" -q .content | base64 -d
```
(`S` = a durable scratch dir, e.g. `<ROOT>/.router/`.)

| Shell pitfall | Fix |
|---|---|
| macOS `sed -i 's/…//'` → "undefined label" | `sed -i '' …` |
| jq `created:createdAt` → "function not defined" | `created: .createdAt` |
| `git rev-parse --short A B` → "Needed a single revision" | one ref per call |
| zsh `--include=*.rs` → "no matches found" | quote: `--include='*.rs'` |
| `head -$n`, n=0 → "illegal line count" | guard n, or `sed -n` |
| `-m "…\n\n…"` → literal `\n` in the commit | `-m $'subject\n\nbody'`, `\'` |
| `cmd \| grep -q` under pipefail → exit 141 | `v=$(cmd)`; compare with `[ ]` |
| `git notes --ref=mr show` denied by the router hook | `git log -1 --notes=mr --format=%N <tip>` (whitelisted; empty = no note) |
