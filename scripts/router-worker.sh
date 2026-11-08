#!/bin/sh
# Router-only launcher for headless `claude -p` workers (E-ROUTER Q2/Q3 override, 2026-11-08).
# The router's Bash hook allows ONLY this script's two shapes; every argument is validated
# here, and the prompt is fixed text, so no untrusted string ever reaches `claude -p`.
#   router-worker.sh spawn  <name> <worktree> <brief.md> <model>
#   router-worker.sh resume <name> <session-uuid> <worktree> <note.md> <model>
# brief/note must be a regular (non-symlink) .md file under <repo>/.router/briefs/; worktree must be a registered
# git worktree OTHER than the router's own checkout. The file is copied to a read-only snapshot
# .router/runs/<name>-<ts>-<mode>/ and the worker reads the snapshot; sha256 + path are printed so the router can verify.
# resume must name the same worktree the session was spawned in (recorded in the spawn's run dir).
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
top=$(cd "$(git rev-parse --show-toplevel)" && pwd -P) || die "not in a git repo"
[ "$wt" != "$ROOT" ] && [ "$wt" != "$top" ] || die "worker may not run in the router's own checkout ($wt)"
main=$(git -C "$ROOT" worktree list --porcelain | sed -n '1s/^worktree //p')
main=$(cd "$main" 2>/dev/null && pwd -P) || die "cannot resolve main checkout"
[ "$wt" != "$main" ] || die "worker may not run in the repository's main checkout ($wt)"
f=$(cd "$(dirname "$file")" 2>/dev/null && pwd -P)/$(basename "$file")
case "$f" in "$B"/*.md) [ -f "$f" ] && [ ! -L "$f" ] || die "no such regular file (symlinks refused)";; *) die "file must be a .md under $B";; esac
mkdir -p "$O" "$ROOT/.router/runs"
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
f="$R/brief.md"
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
# survive a 2s startup window (poll 0.2s); an early exit reports its real exit code.
# alive() excludes zombies: an exited-but-unreaped child still answers `ps -p`.
alive() { st=$(ps -o stat= -p "$pid" 2>/dev/null) || return 1; case "$st" in Z*|"") return 1;; esac; }
n=0; while [ $n -lt 10 ] && alive; do sleep 0.2; n=$((n+1)); done
if alive; then echo "RUNNING name=$name pid=$pid session=$sid out=$O/$name.out snapshot=$f sha256=$sha worktree=$wt"
else rc=0; wait "$pid" || rc=$?; echo "DEAD name=$name exit=$rc session=$sid snapshot=$f sha256=$sha"; tail -5 "$O/$name.out"; exit 1; fi
