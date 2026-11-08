#!/bin/sh
# Router-only launcher for headless `claude -p` workers (E-ROUTER Q2/Q3 override, 2026-11-08).
# The router's Bash hook allows ONLY this script's two shapes; every argument is validated
# here, and the prompt is fixed text, so no untrusted string ever reaches `claude -p`.
#   router-worker.sh spawn  <name> <worktree> <brief.md> <model>
#   router-worker.sh resume <name> <session-uuid> <worktree> <note.md> <model>
# brief/note must be a .md file under <repo>/.router/briefs/; worktree must be a registered git worktree.
set -eu
die() { echo "router-worker: $*" >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
B="$ROOT/.router/briefs" O="$ROOT/.router/out"
C=${ROUTER_CLAUDE:-$HOME/.local/bin/claude}
[ -x "$C" ] || die "claude not found at $C (set ROUTER_CLAUDE)"
mode=${1:-}; [ $# -ge 1 ] && shift
case "$mode" in spawn) [ $# -eq 4 ] || die "spawn <name> <worktree> <brief.md> <model>";;
  resume) [ $# -eq 5 ] || die "resume <name> <uuid> <worktree> <note.md> <model>";; *) die "spawn|resume";; esac
name=$1; shift
expr "$name" : '^[a-z0-9][a-z0-9-]\{0,40\}$' >/dev/null || die "bad name"
if [ "$mode" = resume ]; then sid=$1; shift
  expr "$sid" : '^[0-9a-f]\{8\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{12\}$' >/dev/null || die "bad session id"
else sid=$(uuidgen | tr 'A-F' 'a-f'); fi
wt=$1 file=$2 model=$3
case "$model" in opus|sonnet|haiku|fable) ;; *) die "model must be opus|sonnet|haiku|fable";; esac
wt=$(cd "$wt" 2>/dev/null && pwd -P) || die "no such worktree"
git -C "$ROOT" worktree list --porcelain | grep -Fxq "worktree $wt" || die "not a registered worktree of this repo"
f=$(cd "$(dirname "$file")" 2>/dev/null && pwd -P)/$(basename "$file")
case "$f" in "$B"/*.md) [ -f "$f" ] || die "no such file";; *) die "file must be a .md under $B";; esac
mkdir -p "$O"
if [ "$mode" = spawn ]; then
  prompt="You are a fresh worker session. Read $f in full and do exactly what it says, in ONE non-stop run (status messages are not stopping points; stop only at your final self-contained report). First action: git branch --show-current, git log --oneline -6, git status --short."
  set -- --session-id "$sid"
else
  prompt="Continue your work. Read $f in full and do exactly what it says, then end with a final self-contained report."
  set -- --resume "$sid"
fi
cd "$wt"
nohup "$C" -p "$prompt" "$@" -n "$name" --model "$model" --agent worker --permission-mode acceptEdits --max-budget-usd 25 \
  >> "$O/$name.out" 2>&1 < /dev/null &
pid=$!; echo "$pid" > "$O/$name.pid"
n=0; until ps -p "$pid" >/dev/null 2>&1 || [ $n -ge 10 ]; do sleep 1; n=$((n+1)); done
if ps -p "$pid" >/dev/null 2>&1; then echo "RUNNING name=$name pid=$pid session=$sid out=$O/$name.out"
else echo "DEAD name=$name session=$sid"; tail -5 "$O/$name.out"; exit 1; fi
