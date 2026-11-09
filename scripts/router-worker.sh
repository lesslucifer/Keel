#!/bin/sh
# Router-only launcher for headless `claude -p` workers (E-ROUTER Q2/Q3 override, 2026-11-08).
# The router's Bash hook allows ONLY this script's three shapes; every argument is validated
# here, and the prompt is fixed text, so no untrusted string ever reaches `claude -p`.
#   router-worker.sh spawn  <name> <worktree> <brief.md> <model>
#   router-worker.sh resume <name> <session-uuid> <worktree> <note.md> <model>
#   router-worker.sh wait   <name> [<stall-min>]
# wait (run by the router as a background Bash command, so its exit wakes the router) blocks
# until the worker exits (EXITED) or stalls (STALLED: on two polls running, no Bash-tool shell
# child and its session transcript untouched for <stall-min> minutes, default 15), prints that one
# line, exits 0. It only reads; the transcript is ${CLAUDE_CONFIG_DIR:-~/.claude}/projects/
# <worktree, non-alnum -> ->/<session>.jsonl plus <session>/subagents/*.jsonl, falling back to the
# pid file's mtime when none exists. Poll: ROUTER_WAIT_POLL s. A background Bash command is killed
# at its timeout (max 2h): the router then wakes without a line and re-arms.
# brief/note must be a regular (non-symlink) .md file under <repo>/.router/briefs/; worktree must be a registered
# git worktree OTHER than the router's own checkout. The file is copied to a read-only snapshot
# .router/runs/<name>-<ts>-<mode>/ (sha256 + path printed so the router can verify), and the worker reads a copy
# of it inside its own worktree (<worktree>/.router-brief.XXXXXX/brief.md, git-ignored).
# resume must name the same worktree the session was spawned in (recorded in the spawn's run dir).
# One live run per name; each run truncates .router/out/<name>.out, so it holds only that run's report.
set -eu
die() { echo "router-worker: $*" >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
B="$ROOT/.router/briefs" O="$ROOT/.router/out"
live() { ps -o stat=,command= -p "$1" 2>/dev/null | grep -v '^ *Z' | grep -qF -- " -n $name "; }
if [ "${1:-}" = wait ]; then
  [ $# -eq 2 ] || [ $# -eq 3 ] || die "wait <name> [<stall-min>]"
  name=$2 stall=${3-15}
  expr "$name" : '^[a-z0-9][a-z0-9-]\{0,40\}$' >/dev/null || die "bad name"
  expr "$stall" : '^[1-9][0-9]\{0,3\}$' >/dev/null || die "stall-min must be 1-9999"
  pid=$(cat "$O/$name.pid" 2>/dev/null) || die "no worker $name ($O/$name.pid missing)"
  run=
  for d in "$ROOT"/.router/runs/*/; do [ "$(cat "${d}name" 2>/dev/null)" = "$name" ] && run=$d; done
  [ -n "$run" ] || die "no recorded run for $name"
  t="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects/$(sed 's/[^A-Za-z0-9]/-/g' "${run}worktree")/$(cat "${run}session").jsonl"
  if stat -c %Y / >/dev/null 2>&1; then mtime() { stat -c '%Y %n' "$@"; }; else mtime() { stat -f '%m %N' "$@"; }; fi
  # "<mtime> <path>" of the newest transcript; subagent turns land in <session>/subagents/*.jsonl
  # and leave <session>.jsonl untouched
  seen() { mtime "$t" "${t%.jsonl}"/subagents/*.jsonl 2>/dev/null | sort -n | tail -1; }
  # a running Bash tool command is a `<shell> -c` child; MCP servers (also children) are not
  busy() { ps -A -o ppid=,command= | awk -v p="$pid" '$1 == p && $2 ~ /(^|\/)[a-z]*sh$/ && $3 == "-c" { f = 1 } END { exit !f }'; }
  # quiet must hold on two consecutive polls: a command that just ended leaves the transcript
  # stale until its result is written, and the worker may be exiting
  quiet=0
  while live "$pid"; do
    if busy; then quiet=0
    else
      at=$(seen)
      [ -n "$at" ] || at=$(mtime "$O/$name.pid")
      src=${at#* } idle=$(( $(date +%s) - ${at%% *} ))
      if [ "$idle" -lt $((stall * 60)) ]; then quiet=0
      elif [ "$quiet" -eq 1 ]; then echo "STALLED name=$name pid=$pid idle=$((idle / 60))m since=$src"; exit 0
      else quiet=1; fi
    fi
    sleep "${ROUTER_WAIT_POLL:-10}"
  done
  echo "EXITED name=$name pid=$pid out=$O/$name.out"; exit 0
fi
C=${ROUTER_CLAUDE:-$HOME/.local/bin/claude}
[ -x "$C" ] || die "claude not found at $C (set ROUTER_CLAUDE)"
mode=${1:-}; [ $# -ge 1 ] && shift
case "$mode" in spawn) [ $# -eq 4 ] || die "spawn <name> <worktree> <brief.md> <model>";;
  resume) [ $# -eq 5 ] || die "resume <name> <uuid> <worktree> <note.md> <model>";; *) die "spawn|resume|wait";; esac
name=$1; shift
expr "$name" : '^[a-z0-9][a-z0-9-]\{0,40\}$' >/dev/null || die "bad name"
if [ "$mode" = resume ]; then sid=$1; shift
  expr "$sid" : '^[0-9a-f]\{8\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{12\}$' >/dev/null || die "bad session id"
else sid=$(uuidgen | tr 'A-F' 'a-f'); fi
wt=$1 file=$2 model=$3
case "$model" in opus|sonnet|haiku|fable) ;; *) die "model must be opus|sonnet|haiku|fable";; esac
wt=$(cd "$wt" 2>/dev/null && pwd -P) || die "no such worktree"
git -C "$ROOT" worktree list --porcelain | grep -Fxq "worktree $wt" || die "not a registered worktree of this repo"
top=$(cd "$(git rev-parse --show-toplevel)" && pwd -P) || die "not in a git repo"
[ "$wt" != "$ROOT" ] && [ "$wt" != "$top" ] || die "worker may not run in the router's own checkout ($wt)"
main=$(git -C "$ROOT" worktree list --porcelain | sed -n '1s/^worktree //p')
main=$(cd "$main" 2>/dev/null && pwd -P) || die "cannot resolve main checkout"
[ "$wt" != "$main" ] || die "worker may not run in the repository's main checkout ($wt)"
f=$(cd "$(dirname "$file")" 2>/dev/null && pwd -P)/$(basename "$file")
case "$f" in "$B"/*.md) [ -f "$f" ] && [ ! -L "$f" ] || die "no such regular file (symlinks refused)";; *) die "file must be a .md under $B";; esac
mkdir -p "$O" "$ROOT/.router/runs"
# one live run per name: a second spawn/resume would interleave <name>.out and orphan the pid
p=$(cat "$O/$name.pid" 2>/dev/null) || p=
if [ -n "$p" ] && live "$p"; then
  die "worker $name is still running (pid $p); wait for it to exit"; fi
if [ "$mode" = resume ]; then
  ok=
  for d in "$ROOT"/.router/runs/*/; do
    [ -f "${d}session" ] && [ "$(cat "${d}session")" = "$sid" ] && [ "$(cat "${d}name")" = "$name" ] || continue
    [ "$(cat "${d}worktree")" = "$wt" ] && ok=1 || die "session $sid was spawned in $(cat "${d}worktree"), not $wt"
  done
  [ -n "$ok" ] || die "no recorded spawn for name=$name session=$sid"
fi
R="$ROOT/.router/runs/$name-$(date +%Y%m%d%H%M%S)-$mode"
mkdir "$R" || die "run dir exists"
cp "$f" "$R/brief.md"
printf '%s\n' "$sid" > "$R/session"; printf '%s\n' "$name" > "$R/name"; printf '%s\n' "$wt" > "$R/worktree"
sha=$(shasum -a 256 "$R/brief.md" | cut -d' ' -f1)
echo "$sha" > "$R/sha256"; chmod 444 "$R"/*; chmod 555 "$R"
# A session may only read its own worktree, so the worker gets a copy inside it, in a fresh
# random-named dir (mktemp -d: never an existing path or symlink) that ignores itself in git.
# Copy-in, not --add-dir "$R": the worker gets no tool access to the router's .router/runs/.
# ponytail: one .router-brief.* dir per run is left in the target until the worktree is removed
# (or `rm -rf .router-brief.*` there); add cleanup if a worktree lives across many runs.
in=$(mktemp -d "$wt/.router-brief.XXXXXX") || die "cannot create brief dir in $wt"
echo '*' > "$in/.gitignore"; cp "$R/brief.md" "$in/brief.md"; chmod 444 "$in/brief.md"
snap="$R/brief.md" f="$in/brief.md"
if [ "$mode" = spawn ]; then
  prompt="You are a fresh worker session. Read $f in full and do exactly what it says, in ONE non-stop run (status messages are not stopping points; stop only at your final self-contained report). First action: git branch --show-current, git log --oneline -6, git status --short."
  set -- --session-id "$sid"
else
  prompt="Continue your work. Read $f in full and do exactly what it says, then end with a final self-contained report."
  set -- --resume "$sid"
fi
cd "$wt"
nohup "$C" -p "$prompt" "$@" -n "$name" --model "$model" --agent worker --permission-mode acceptEdits --max-budget-usd 25 \
  > "$O/$name.out" 2>&1 < /dev/null &
pid=$!; echo "$pid" > "$O/$name.pid"
# survive a 2s startup window (poll 0.2s); an early exit reports its real exit code.
# alive() excludes zombies: an exited-but-unreaped child still answers `ps -p`.
alive() { st=$(ps -o stat= -p "$pid" 2>/dev/null) || return 1; case "$st" in Z*|"") return 1;; esac; }
n=0; while [ $n -lt 10 ] && alive; do sleep 0.2; n=$((n+1)); done
if alive; then echo "RUNNING name=$name pid=$pid session=$sid out=$O/$name.out snapshot=$snap brief=$f sha256=$sha worktree=$wt"
else rc=0; wait "$pid" || rc=$?; echo "DEAD name=$name exit=$rc session=$sid snapshot=$snap brief=$f sha256=$sha"; tail -5 "$O/$name.out"; exit 1; fi
