#!/bin/sh
# Router-only launcher for worker sessions (E-ROUTER Q2/Q3 override, 2026-11-08). The router's Bash
# hook allows ONLY this script's shapes; every argument is validated here, and the prompt is fixed
# text, so no untrusted string ever reaches `claude`.
#   router-worker.sh spawn  <name> <worktree> <brief.md> <model> [--permission-mode acceptEdits|plan] [--effort <level>]
#   router-worker.sh resume <name> <session-uuid> <worktree> <note.md>
#   router-worker.sh stop   <name>
# A worker is a native background session (E-ROUTER3.W10, DEC-44): `claude --bg -n <name> --agent
# worker`, run in its worktree, listed by `claude agents --json` while it lives. It reports to the
# router by SendMessage and stays alive idle; a follow-up task is the router's own SendMessage, so
# the launcher has no mode for it. stop closes an idle worker (`claude stop`, then `claude rm`, which
# keeps its transcript and its worktree); a worker still busy after about 30s is refused, because
# `claude stop` cuts its turn.
# The two spawn options come in either order, once each. The model and effort must satisfy the worker
# policy, [router] in .keel/config.toml at this checkout's HEAD (DEC-39); no policy, no spawn. The mode
# defaults to acceptEdits and the effort to the policy's default. Every worker gets the fixed cargo/keel
# grant below and is denied `keel mr` and `cargo run`; the grant never reaches the router. `--agent
# worker` is always passed: this repo's settings make every other session a router.
# resume wakes a session that is no longer listed (stopped, crashed, rebooted) with its saved options:
# `claude --bg --resume` with any option starts a copy under a new id instead (Claude Code 2.1.288),
# so resume takes none, and the spawn's model, effort, mode and grant carry over. It must name the
# same worktree the session was spawned in (recorded in the spawn's run dir).
# brief/note must be a regular (non-symlink) .md file under <repo>/.router/briefs/; worktree must be a
# registered git worktree OTHER than the router's own checkout. The file is copied to a read-only
# snapshot .router/runs/<name>-<ts>-<pid>-<mode>/ (sha256 + path printed so the router can verify), and
# the worker reads a copy of it inside its own worktree (<worktree>/.router-brief.XXXXXX/brief.md,
# git-ignored). `--bg` assigns the session id, so the run dir records it once the session is listed.
# One live session per name, whatever its directory: SendMessage addresses by name.
set -eu
die() { echo "router-worker: $*" >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
B="$ROOT/.router/briefs"
C=${ROUTER_CLAUDE:-$HOME/.local/bin/claude}
# the one worker-name rule: spawn, resume and stop all check it here, and the guard does not repeat it
valid_name() { expr "$1" : '^[a-z0-9][a-z0-9-]\{0,40\}$' >/dev/null || die "bad name"; }
# the live sessions named $name, one JSON row per line; fails when the CLI gives no list (never "none")
rows() { list=$("$C" agents --json) || return 1; printf '%s\n' "$list" | jq -c --arg n "$name" '.[] | select(.name == $n)'; }
nolist() { die "cannot list sessions (claude agents --json)"; }
# a row's fields, US-separated (a whitespace IFS would merge empty fields); a blocked session (rate
# limited, or waiting on its user) is listed with a state and no status, so the state stands in for it
US=$(printf '\037')
fields() { printf '%s\n' "$r" | jq -r --arg i "${1-}" "select(\$i == \"\" or .id == \$i) | [.id, .sessionId, .cwd, (.status // .state), .kind] | map(. // \"\") | join(\"$US\")"; }
brief_file() {
  f=$(cd "$(dirname "$1")" 2>/dev/null && pwd -P)/$(basename "$1")
  case "$f" in "$B"/*.md) [ -f "$f" ] && [ ! -L "$f" ] || die "no such regular file (symlinks refused)";; *) die "file must be a .md under $B";; esac
}
# The brief $f is copied to a snapshot run dir R (kind $1) with its sha256. A session may only read
# its own worktree, so the worker gets a copy inside it, in a fresh random-named dir (mktemp -d: never
# an existing path or symlink) that ignores itself in git. Copy-in, not --add-dir "$R": the worker gets
# no tool access to the router's .router/runs/.
# ponytail: one .router-brief.* dir per run is left in the target until the worktree is removed
# (or `rm -rf .router-brief.*` there); W3 removes it at stop.
snapshot() {
  mkdir -p "$ROOT/.router/runs"
  R="$ROOT/.router/runs/$name-$(date +%Y%m%d%H%M%S)-$$-$1"
  mkdir "$R" || die "run dir exists"
  cp "$f" "$R/brief.md"
  printf '%s\n' "$name" > "$R/name"; printf '%s\n' "$wt" > "$R/worktree"
  [ -z "${pm:-}" ] || { printf '%s\n' "$pm" > "$R/permission-mode"; printf '%s\n' "$effort" > "$R/effort"; }
  sha=$(shasum -a 256 "$R/brief.md" | cut -d' ' -f1)
  echo "$sha" > "$R/sha256"
  in=$(mktemp -d "$wt/.router-brief.XXXXXX") || die "cannot create brief dir in $wt"
  echo '*' > "$in/.gitignore"; cp "$R/brief.md" "$in/brief.md"; chmod 444 "$in/brief.md"
  snap="$R/brief.md" f="$in/brief.md"
}
# the run dir becomes read-only once it records the session it started (none for a refused start)
seal() { [ -z "$sid" ] || printf '%s\n' "$sid" > "$R/session"; chmod 444 "$R"/*; chmod 555 "$R"; }
[ -x "$C" ] || die "claude not found at $C (set ROUTER_CLAUDE)"
mode=${1:-}; [ $# -ge 1 ] && shift
if [ "$mode" = stop ]; then
  [ $# -eq 1 ] || die "stop <name>"
  name=$1
  valid_name "$name"
  r=$(rows) || nolist
  [ -n "$r" ] || die "worker $name is not running"
  [ "$(printf '%s\n' "$r" | wc -l)" -eq 1 ] || die "more than one session is named $name"
  IFS=$US read -r id sid cwd status kind <<EOF
$(fields)
EOF
  [ "$kind" = background ] || die "$name is not a background session"
  git -C "$ROOT" worktree list --porcelain | grep -Fxq "worktree $cwd" || die "$name runs outside this repository's worktrees ($cwd)"
  # a worker reports by SendMessage before its turn has ended, so a stop right after its report may
  # find it busy: give the turn 60 more looks, 0.5s apart, to end, then refuse; the looks follow
  # this session's id, never another session that takes the name
  n=0
  while [ "$status" = busy ] && [ $n -lt 60 ]; do
    sleep 0.5; n=$((n+1))
    r=$(rows) || nolist
    row=$(fields "$id")
    [ -n "$row" ] || die "worker $name left the session list while busy; nothing to stop"
    status=$(printf '%s\n' "$row" | cut -d "$US" -f 4)
  done
  [ "$status" != busy ] || die "worker $name is still busy: stopping it would cut its turn; the owner decides (claude stop $id)"
  [ "$status" = idle ] || die "worker $name is ${status:-of unknown status}: the owner decides (claude stop $id)"
  "$C" stop "$id" > /dev/null || die "claude stop $id failed"
  "$C" rm "$id" > /dev/null || die "claude rm $id failed"
  echo "STOPPED name=$name id=$id session=$sid"; exit 0
fi
case "$mode" in spawn) n=4 usage="spawn <name> <worktree> <brief.md> <model> [--permission-mode acceptEdits|plan] [--effort <level>]";;
  resume) n=4 usage="resume <name> <uuid> <worktree> <note.md>";; *) die "spawn|resume|stop";; esac
if [ "$mode" = spawn ]; then [ $# -eq $n ] || [ $# -eq $((n + 2)) ] || [ $# -eq $((n + 4)) ] || die "$usage"
else [ $# -eq $n ] || die "$usage"; fi
name=$1; shift
valid_name "$name"
sid=
if [ "$mode" = resume ]; then sid=$1; shift
  expr "$sid" : '^[0-9a-f]\{8\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{12\}$' >/dev/null || die "bad session id"
  wt=$1 file=$2; pm=
else
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
fi
wt=$(cd "$wt" 2>/dev/null && pwd -P) || die "no such worktree"
git -C "$ROOT" worktree list --porcelain | grep -Fxq "worktree $wt" || die "not a registered worktree of this repo"
top=$(cd "$(git rev-parse --show-toplevel)" && pwd -P) || die "not in a git repo"
[ "$wt" != "$ROOT" ] && [ "$wt" != "$top" ] || die "worker may not run in the router's own checkout ($wt)"
main=$(git -C "$ROOT" worktree list --porcelain | sed -n '1s/^worktree //p')
main=$(cd "$main" 2>/dev/null && pwd -P) || die "cannot resolve main checkout"
[ "$wt" != "$main" ] || die "worker may not run in the repository's main checkout ($wt)"
brief_file "$file"
r=$(rows) || nolist
[ -z "$r" ] || die "worker $name is still running; message it, or stop it first"
if [ "$mode" = resume ]; then
  ok=
  for d in "$ROOT"/.router/runs/*/; do
    [ -f "${d}session" ] && [ "$(cat "${d}session")" = "$sid" ] && [ "$(cat "${d}name")" = "$name" ] || continue
    [ "$(cat "${d}worktree")" = "$wt" ] && ok=1 || die "session $sid was spawned in $(cat "${d}worktree"), not $wt"
  done
  [ -n "$ok" ] || die "no recorded spawn for name=$name session=$sid"
fi
snapshot "$mode"
done_line="When your work is finished, send one self-contained report by SendMessage to the router your brief names, then end your turn; you stay alive for follow-up tasks."
if [ "$mode" = spawn ]; then
  set -- --bg -n "$name" --model "$model" --effort "$effort" --agent worker --allowedTools
  # No `cargo run` (`cargo run -- mr` is keel mr) and no install/publish/add/update; no keel init or
  # hooks install (they write the shared git dir). A deny also beats the owner's user-level allowlist.
  for c in "cargo build" "cargo test" "cargo check" "cargo clippy" "cargo fmt" "keel preflight" \
    "keel verify-doc" "keel verify-dod" "keel notes write" "keel lint" "keel check"; do
    set -- "$@" "Bash($c)" "Bash($c *)"; done
  # the prompt follows a single-valued option: after a list option it would be read as a tool name
  set -- "$@" --disallowedTools "Bash(keel mr)" "Bash(keel mr *)" "Bash(cargo run)" "Bash(cargo run *)" --permission-mode "$pm" \
    "You are a fresh worker session. Read $f in full and do exactly what it says. $done_line First action: git branch --show-current, git log --oneline -6, git status --short."
else
  set -- --bg --resume "$sid" "Continue your work. Read $f in full and do exactly what it says. $done_line"
fi
cd "$wt"
rc=0; started=$("$C" "$@" 2>&1) || rc=$?
if [ "$rc" -eq 0 ]; then
  # listed within 50 looks, 0.2s apart, or the start failed
  n=0
  while r=$(rows) || {
      # the session may have started: report it with its snapshot, never as refused before a start
      sid=; seal
      echo "UNLISTED name=$name exit=0 snapshot=$snap brief=$f sha256=$sha"; printf '%s\n' "$started"
      die "cannot list sessions (claude agents --json): the session may be running; check ListAgents before any respawn"; }
    [ -z "$r" ] && [ $n -lt 50 ]; do sleep 0.2; n=$((n+1)); done
fi
if [ "$rc" -ne 0 ] || [ -z "$r" ]; then
  sid=; seal
  echo "DEAD name=$name exit=$rc snapshot=$snap brief=$f sha256=$sha"; printf '%s\n' "$started"; exit 1
fi
IFS=$US read -r id sid _ <<EOF
$(fields)
EOF
seal
opts=; [ -z "$pm" ] || opts="permission-mode=$pm effort=$effort "
echo "RUNNING name=$name id=$id session=$sid ${opts}snapshot=$snap brief=$f sha256=$sha worktree=$wt"
