#!/bin/sh
# Router-only launcher for headless `claude -p` workers (E-ROUTER Q2/Q3 override, 2026-11-08).
# The router's Bash hook allows ONLY this script's shapes; every argument is validated
# here, and the prompt is fixed text, so no untrusted string ever reaches `claude -p`.
#   router-worker.sh spawn  <name> <worktree> <brief.md> <model> [--permission-mode acceptEdits|plan] [--effort <level>]
#   router-worker.sh resume <name> <session-uuid> <worktree> <note.md> <model> [--permission-mode acceptEdits|plan] [--effort <level>]
#   router-worker.sh send   <name> <brief.md>
#   router-worker.sh stop   <name>
#   router-worker.sh wait   <name> [<stall-min>]
# A worker is long-running (E-ROUTER3.W2): `claude -p` in stream-json mode reads its turns from
# .router/out/<name>.in, kept open by a `tail -f` feed, so it stays alive between briefs. send appends
# the next brief (snapshotted like any other) to a live worker; stop closes the feed, and the worker
# exits after its current turn. resume restarts a dead session the same way.
# The two options come in either order, once each. The model and effort must satisfy the worker policy,
# [router] in .keel/config.toml at this checkout's HEAD (DEC-39); no policy, no run. The mode defaults
# to acceptEdits and the effort to the policy's default, per run (a resume inherits neither). Every worker gets the fixed
# cargo/keel grant below and is denied `keel mr` and `cargo run`; the grant never reaches the router.
# wait (run by the router as a background Bash command, so its exit wakes the router) blocks
# until the worker ends a turn with every sent brief taken and answered (IDLE; its last result text goes
# to .router/out/<name>.report, removed when the last brief has no answer), exits (EXITED) or stalls mid-turn (STALLED: on two polls running, no
# Bash-tool shell child and its session transcript untouched for <stall-min> minutes, default 15),
# prints that one line, exits 0. It writes only <name>.report, its scan state <name>.scan (and a scratch
# dir it removes); the transcript is ${CLAUDE_CONFIG_DIR:-~/.claude}/projects/
# */<session>.jsonl plus */<session>/subagents/*.jsonl, falling back to the
# pid file's mtime when none exists. Poll: ROUTER_WAIT_POLL s. A background Bash command is killed
# at its timeout (max 2h): the router then wakes without a line and re-arms.
# brief/note must be a regular (non-symlink) .md file under <repo>/.router/briefs/; worktree must be a registered
# git worktree OTHER than the router's own checkout. The file is copied to a read-only snapshot
# .router/runs/<name>-<ts>-<mode>/ (sha256 + path printed so the router can verify), and the worker reads a copy
# of it inside its own worktree (<worktree>/.router-brief.XXXXXX/brief.md, git-ignored).
# resume must name the same worktree the session was spawned in (recorded in the spawn's run dir).
# One live run per name; each run truncates .router/out/<name>.out (the raw stream-json output), so it
# holds only that run. The worker runs as `-n <name>`, the name SendMessage addresses (DEC-42).
set -eu
die() { echo "router-worker: $*" >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
B="$ROOT/.router/briefs" O="$ROOT/.router/out"
# the one worker-name rule: spawn, resume and wait all check it here, and the guard does not repeat it
valid_name() { expr "$1" : '^[a-z0-9][a-z0-9-]\{0,40\}$' >/dev/null || die "bad name"; }
live() { ps -o stat=,command= -p "$1" 2>/dev/null | grep -v '^ *Z' | grep -qF -- " -n $name "; }
# the feed is open while <name>.feed names this worker's own tail of its inbox, never a reused pid
feed_open() { ps -o command= -p "$(cat "$O/$name.feed" 2>/dev/null)" 2>/dev/null | grep -qxF -- "tail -n +1 -f $O/$name.in"; }
newest_run() { run=; for d in "$ROOT"/.router/runs/*/; do [ "$(cat "${d}name" 2>/dev/null)" = "$name" ] && run=$d; done; [ -n "$run" ]; }
brief_file() {
  f=$(cd "$(dirname "$1")" 2>/dev/null && pwd -P)/$(basename "$1")
  case "$f" in "$B"/*.md) [ -f "$f" ] && [ ! -L "$f" ] || die "no such regular file (symlinks refused)";; *) die "file must be a .md under $B";; esac
}
# The brief $f is copied to a read-only snapshot run dir R (kind $1) with its sha256. A session may
# only read its own worktree, so the worker gets a copy inside it, in a fresh random-named dir
# (mktemp -d: never an existing path or symlink) that ignores itself in git. Copy-in, not
# --add-dir "$R": the worker gets no tool access to the router's .router/runs/.
# ponytail: one .router-brief.* dir per run is left in the target until the worktree is removed
# (or `rm -rf .router-brief.*` there); add cleanup if a worktree lives across many runs.
snapshot() {
  mkdir -p "$O" "$ROOT/.router/runs"
  R="$ROOT/.router/runs/$name-$(date +%Y%m%d%H%M%S)-$$-$1"
  mkdir "$R" || die "run dir exists"
  cp "$f" "$R/brief.md"
  printf '%s\n' "$sid" > "$R/session"; printf '%s\n' "$name" > "$R/name"; printf '%s\n' "$wt" > "$R/worktree"
  [ -z "${pm:-}" ] || { printf '%s\n' "$pm" > "$R/permission-mode"; printf '%s\n' "$effort" > "$R/effort"; }
  sha=$(shasum -a 256 "$R/brief.md" | cut -d' ' -f1)
  echo "$sha" > "$R/sha256"; chmod 444 "$R"/*; chmod 555 "$R"
  in=$(mktemp -d "$wt/.router-brief.XXXXXX") || die "cannot create brief dir in $wt"
  echo '*' > "$in/.gitignore"; cp "$R/brief.md" "$in/brief.md"; chmod 444 "$in/brief.md"
  snap="$R/brief.md" f="$in/brief.md"
}
# one stream-json user turn appended to the worker's inbox
turn() { jq -cn --arg t "$1" '{type: "user", message: {role: "user", content: $t}}' >> "$O/$name.in"; }
if [ "${1:-}" = wait ]; then
  [ $# -eq 2 ] || [ $# -eq 3 ] || die "wait <name> [<stall-min>]"
  name=$2 stall=${3-15}
  valid_name "$name"
  expr "$stall" : '^[1-9][0-9]\{0,3\}$' >/dev/null || die "stall-min must be 1-9999"
  pid=$(cat "$O/$name.pid" 2>/dev/null) || die "no worker $name ($O/$name.pid missing)"
  newest_run || die "no recorded run for $name"
  # found by the unique session id under any project dir: Claude Code's dir name (worktree path with
  # non-alnum -> -, truncated past 200 chars plus a hash) is not re-derived here
  P="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects" sid=$(cat "${run}session")
  if stat -c %Y / >/dev/null 2>&1; then mtime() { stat -c '%Y %n' "$@"; }; else mtime() { stat -f '%m %N' "$@"; }; fi
  # "<mtime> <path>" of the newest transcript; subagent turns land in <session>/subagents/*.jsonl
  # and leave <session>.jsonl untouched
  seen() { mtime "$P"/*/"$sid".jsonl "$P"/*/"$sid"/subagents/*.jsonl 2>/dev/null | sort -n | tail -1; }
  # a running Bash tool command is a `<shell> -c` child; MCP servers (also children) are not
  busy() { ps -A -o ppid=,command= | awk -v p="$pid" '$1 == p && $2 ~ /(^|\/)[a-z]*sh$/ && $3 == "-c" { f = 1 } END { exit !f }'; }
  # The stream-json output is read once, incrementally, across every wait of a run: each poll
  # counts only the complete lines added since the scan state in <name>.scan (offset, counters,
  # then the last result's text; spawn and resume reset it with the output), streamed through jq.
  # Every turn opens with system/init and closes with a result, which may answer several briefs
  # folded into it; a brief is taken when the worker replays it (--replay-user-messages); a peer's
  # SendMessage replays as isSynthetic and is no brief. A turn may end with a background task still
  # running (the last system/background_tasks_changed lists it): that is not the worker's answer.
  # When the task ends, its emptied task list and its system/task_notification come just before
  # the completion turn's init, so either, not yet followed by an init or a result, means a turn
  # is due. Key order varies and non-JSON lines are skipped. (Event shapes: Claude Code 2.1.288,
  # E-ROUTER3.W2 probes.)
  T=$(mktemp -d) || die "cannot create a scratch dir"
  trap 'rm -rf "$T"' EXIT
  S="$O/$name.scan"
  load() {
    off=0 inits=0 results=0 replays=0 bg=0 due=0 err=- ; : > "$T/last"
    [ -s "$S" ] || return 0
    read -r off inits results replays bg due err < "$S"
    tail -n +2 "$S" > "$T/last"
  }
  scan() {
    load
    tail -c +$((off + 1)) "$O/$name.out" 2>/dev/null > "$T/raw" || return 0
    n=$(( $(wc -l < "$T/raw") ))
    [ "$n" -gt 0 ] || return 0
    head -n "$n" "$T/raw" > "$T/chunk"
    off=$((off + $(wc -c < "$T/chunk") ))
    # one streamed pass: "<inits> <results> <replays> <background tasks> <turn due> <error>" (-1: not
    # in this chunk; error: the last result's subtype if it is an error, else -), then the last
    # result's text
    jq -n -R -r 'reduce (inputs | fromjson? | objects) as $x ({i: 0, r: 0, p: 0, bg: -1, due: -1, last: null};
        if $x.type == "result" then .r += 1 | .due = 0 | .last = $x
        elif $x.type == "system" and $x.subtype == "init" then .i += 1 | .due = 0
        elif $x.type == "system" and $x.subtype == "task_notification" then .due = 1
        elif $x.type == "system" and $x.subtype == "background_tasks_changed" then .bg = ($x.tasks | length) | if .bg == 0 then .due = 1 else . end
        elif $x.type == "user" and $x.isReplay == true and $x.isSynthetic != true then .p += 1
        else . end)
      | .last as $l
      | "\(.i) \(.r) \(.p) \(.bg) \(.due) \(if $l.is_error == true then $l.subtype // "error" else "-" end)",
        (if $l == null then empty elif $l.is_error == true then "error: \($l.subtype // "error")" + (($l.errors // []) | if length > 0 then ": " + join("; ") else "" end) else $l.result // "" end)' "$T/chunk" > "$T/scan"
    read -r i r p b d e < "$T/scan"
    inits=$((inits + i)) results=$((results + r)) replays=$((replays + p))
    [ "$b" -lt 0 ] || bg=$b
    [ "$d" -lt 0 ] || due=$d
    [ "$r" -eq 0 ] || { err=$e; tail -n +2 "$T/scan" > "$T/last"; }
    { echo "$off $inits $results $replays $bg $due $err"; cat "$T/last"; } > "$S.$$" && mv "$S.$$" "$S"
  }
  # settled: every opened turn has closed, no turn is due, and every brief in the inbox was taken
  settled() { [ "$results" -ge "$inits" ] && [ "$due" -eq 0 ] && [ "$replays" -ge $(( $(cat "$O/$name.in" 2>/dev/null | wc -l) )) ]; }
  # answered: settled with no background task left running
  answered() { settled && [ "$bg" -eq 0 ]; }
  # the report is the last result, and only for an answered inbox (judged once, by the caller):
  # never an earlier brief's answer
  report() { if [ "$1" = answered ] && [ -s "$T/last" ]; then cp "$T/last" "$O/$name.report"; else rm -f "$O/$name.report"; fi; }
  # quiet must hold on two consecutive polls: a command that just ended leaves the transcript
  # stale until its result is written, and the worker may be exiting
  quiet=0
  while live "$pid"; do
    scan
    # a stopped worker winding down (feed closed) is not idle: it ends in EXITED
    if answered && feed_open; then report answered
      echo "IDLE name=$name pid=$pid turn=$results report=$O/$name.report$([ "$err" = - ] || echo " error=$err")"; exit 0
    # settled but for a background task: its shell keeps the worker busy, so only the transcript's
    # quiet can tell a task that never ends, and the wait stalls instead of running forever
    elif ! { settled && [ "$bg" -gt 0 ]; } && busy; then quiet=0
    else
      at=$(seen)
      [ -n "$at" ] || at=$(mtime "$O/$name.pid")
      src=${at#* } idle=$(( $(date +%s) - ${at%% *} ))
      if [ "$idle" -lt $((stall * 60)) ]; then quiet=0
      elif [ "$quiet" -eq 1 ]; then echo "STALLED name=$name pid=$pid idle=$((idle / 60))m since=$src$([ "$bg" -eq 0 ] || echo " background=$bg")"; exit 0
      else quiet=1; fi
    fi
    sleep "${ROUTER_WAIT_POLL:-10}"
  done
  scan; if answered; then report answered; else report unanswered; fi
  echo "EXITED name=$name pid=$pid out=$O/$name.out"; exit 0
fi
if [ "${1:-}" = send ] || [ "${1:-}" = stop ]; then
  mode=$1; shift
  if [ "$mode" = send ]; then [ $# -eq 2 ] || die "send <name> <brief.md>"; else [ $# -eq 1 ] || die "stop <name>"; fi
  name=$1
  valid_name "$name"
  pid=$(cat "$O/$name.pid" 2>/dev/null) && live "$pid" || die "worker $name is not running"
  if [ "$mode" = stop ]; then
    feed_open || die "worker $name is already stopping (its input is closed)"
    kill "$(cat "$O/$name.feed")" 2>/dev/null || die "cannot close $name's input"
    echo "STOPPING name=$name pid=$pid (exits after its current turn; wait prints EXITED)"; exit 0
  fi
  feed_open || die "worker $name is stopping (its input is closed); resume it once it has exited"
  brief_file "$2"
  newest_run || die "no recorded run for $name"
  sid=$(cat "${run}session") wt=$(cat "${run}worktree")
  snapshot send
  turn "Continue your work. Read $f in full and do exactly what it says, then end with a final self-contained report."
  echo "SENT name=$name pid=$pid session=$sid snapshot=$snap brief=$f sha256=$sha"; exit 0
fi
C=${ROUTER_CLAUDE:-$HOME/.local/bin/claude}
[ -x "$C" ] || die "claude not found at $C (set ROUTER_CLAUDE)"
mode=${1:-}; [ $# -ge 1 ] && shift
case "$mode" in spawn) n=4 usage="spawn <name> <worktree> <brief.md> <model>";;
  resume) n=5 usage="resume <name> <uuid> <worktree> <note.md> <model>";; *) die "spawn|resume|send|stop|wait";; esac
usage="$usage [--permission-mode acceptEdits|plan] [--effort <level>]"
[ $# -eq $n ] || [ $# -eq $((n + 2)) ] || [ $# -eq $((n + 4)) ] || die "$usage"
name=$1; shift
valid_name "$name"
if [ "$mode" = resume ]; then sid=$1; shift
  expr "$sid" : '^[0-9a-f]\{8\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{12\}$' >/dev/null || die "bad session id"
else sid=$(uuidgen | tr 'A-F' 'a-f'); fi
wt=$1 file=$2 model=$3; shift 3
pm=acceptEdits pm_set= effort= effort_set=
while [ $# -gt 0 ]; do
  case "$1" in
    --permission-mode) [ -z "$pm_set" ] || die "$usage"; pm_set=1 pm=$2;;
    --effort) [ -z "$effort_set" ] || die "$usage"; effort_set=1 effort=$2;;
    *) die "$usage";;
  esac
  shift 2
done
# The worker policy (DEC-39) is [router] in .keel/config.toml as committed at this checkout's HEAD,
# so an uncommitted edit cannot widen it. The reader accepts one exact `[router]` header and in it only
# `key = "word"` and `key = ["word", ...]` (a word starts with a letter; optional trailing # comment);
# any other line in the section, a second `[router]` or any other header naming router refuses.
policy=$(git -C "$ROOT" show HEAD:.keel/config.toml 2>/dev/null | awk '
  /^[ \t]*\[/ {
    on = /^\[router\]([ \t]+#.*)?$/
    if (on && seen++) bad = 1
    if (!on && /^[ \t]*\[+[ \t]*"?router"?[ \t]*(\]|\.)/) bad = 1
    next
  }
  !on { next }
  { sub(/[ \t]*#.*$/, ""); sub(/[ \t]+$/, "") }
  /^$/ { next }
  /^worker_model = "[a-z][a-z0-9.-]*"$/ && m == "" { m = $3; next }
  /^worker_default_effort = "[a-z][a-z0-9.-]*"$/ && d == "" { d = $3; next }
  /^worker_efforts = \["[a-z][a-z0-9.-]*"(, "[a-z][a-z0-9.-]*")*\]$/ && e == "" { e = substr($0, 19, length($0) - 19); next }
  { bad = 1 }
  END {
    gsub(/"/, "", m); gsub(/"/, "", d); gsub(/[",]/, "", e)
    k = split(e, l, " ")
    for (i = 1; i <= k; i++) { if (l[i] !~ /^(low|medium|high|xhigh|max)$/) bad = 1; if (l[i] == d) has = 1 }
    if (bad || m == "" || !has) exit 1
    print m; print d; print e
  }') || die "no worker policy: [router] worker_model / worker_efforts / worker_default_effort in .keel/config.toml at HEAD of $ROOT"
{ read -r policy_model; read -r policy_default; read -r policy_efforts; } <<EOF
$policy
EOF
[ "$model" = "$policy_model" ] || die "model must be $policy_model ([router] worker_model in .keel/config.toml)"
[ -n "$effort_set" ] || effort=$policy_default
case "$effort" in ""|*[!a-z0-9.-]*) die "effort must be one of: $policy_efforts ([router] worker_efforts in .keel/config.toml)";; esac
case " $policy_efforts " in *" $effort "*) ;; *) die "effort must be one of: $policy_efforts ([router] worker_efforts in .keel/config.toml)";; esac
case "$pm" in acceptEdits|plan) ;; *) die "permission mode must be acceptEdits|plan";; esac
wt=$(cd "$wt" 2>/dev/null && pwd -P) || die "no such worktree"
git -C "$ROOT" worktree list --porcelain | grep -Fxq "worktree $wt" || die "not a registered worktree of this repo"
top=$(cd "$(git rev-parse --show-toplevel)" && pwd -P) || die "not in a git repo"
[ "$wt" != "$ROOT" ] && [ "$wt" != "$top" ] || die "worker may not run in the router's own checkout ($wt)"
main=$(git -C "$ROOT" worktree list --porcelain | sed -n '1s/^worktree //p')
main=$(cd "$main" 2>/dev/null && pwd -P) || die "cannot resolve main checkout"
[ "$wt" != "$main" ] || die "worker may not run in the repository's main checkout ($wt)"
brief_file "$file"
mkdir -p "$O"
# one live run per name: a second spawn/resume would interleave <name>.out and orphan the pid
p=$(cat "$O/$name.pid" 2>/dev/null) || p=
if [ -n "$p" ] && live "$p"; then
  die "worker $name is still running (pid $p); send it the brief, or stop it first"; fi
if [ "$mode" = resume ]; then
  ok=
  for d in "$ROOT"/.router/runs/*/; do
    [ -f "${d}session" ] && [ "$(cat "${d}session")" = "$sid" ] && [ "$(cat "${d}name")" = "$name" ] || continue
    [ "$(cat "${d}worktree")" = "$wt" ] && ok=1 || die "session $sid was spawned in $(cat "${d}worktree"), not $wt"
  done
  [ -n "$ok" ] || die "no recorded spawn for name=$name session=$sid"
fi
snapshot "$mode"
if [ "$mode" = spawn ]; then
  prompt="You are a fresh worker session. Read $f in full and do exactly what it says, in ONE non-stop run (status messages are not stopping points; stop only at your final self-contained report). First action: git branch --show-current, git log --oneline -6, git status --short."
  set -- --session-id "$sid"
else
  prompt="Continue your work. Read $f in full and do exactly what it says, then end with a final self-contained report."
  set -- --resume "$sid"
fi
# No `cargo run` (`cargo run -- mr` is keel mr) and no install/publish/add/update; no keel init or
# hooks install (they write the shared git dir). A deny also beats the owner's user-level allowlist.
set -- "$@" --allowedTools
for c in "cargo build" "cargo test" "cargo check" "cargo clippy" "cargo fmt" "keel preflight" \
  "keel verify-doc" "keel verify-dod" "keel notes write" "keel lint" "keel check"; do
  set -- "$@" "Bash($c)" "Bash($c *)"; done
set -- "$@" --disallowedTools "Bash(keel mr)" "Bash(keel mr *)" "Bash(cargo run)" "Bash(cargo run *)"
: > "$O/$name.in"; turn "$prompt"; rm -f "$O/$name.feed" "$O/$name.scan"
cd "$wt"
# the feed keeps stdin open while the worker waits for its next brief; killing it (stop) is EOF
sh -c 'tail -n +1 -f "$1" & echo $! > "$2"; wait' sh "$O/$name.in" "$O/$name.feed" 2>/dev/null |
  nohup "$C" -p --input-format stream-json --output-format stream-json --verbose --replay-user-messages "$@" -n "$name" --model "$model" --effort "$effort" --agent worker --permission-mode "$pm" --max-budget-usd 25 \
  > "$O/$name.out" 2>&1 &
pid=$!; echo "$pid" > "$O/$name.pid"
n=0; until [ -s "$O/$name.feed" ] || [ $n -ge 50 ]; do sleep 0.1; n=$((n+1)); done
# a worker that dies on its own leaves its feed blocked in tail -f: end that feed (this run's, by
# value: a later run of the name rewrites the feed file) with the worker
nohup sh -c 'while kill -0 "$1" 2>/dev/null; do sleep 1; done
  ps -o command= -p "$2" 2>/dev/null | grep -qxF -- "tail -n +1 -f $3" && kill "$2"' sh "$pid" "$(cat "$O/$name.feed")" "$O/$name.in" > /dev/null 2>&1 &
# survive a 2s startup window (poll 0.2s); an early exit reports its real exit code.
# alive() excludes zombies: an exited-but-unreaped child still answers `ps -p`.
alive() { st=$(ps -o stat= -p "$pid" 2>/dev/null) || return 1; case "$st" in Z*|"") return 1;; esac; }
n=0; while [ $n -lt 10 ] && alive; do sleep 0.2; n=$((n+1)); done
if alive; then echo "RUNNING name=$name pid=$pid session=$sid permission-mode=$pm effort=$effort out=$O/$name.out snapshot=$snap brief=$f sha256=$sha worktree=$wt"
else rc=0; wait "$pid" || rc=$?; echo "DEAD name=$name exit=$rc session=$sid snapshot=$snap brief=$f sha256=$sha"; tail -5 "$O/$name.out"; exit 1; fi
