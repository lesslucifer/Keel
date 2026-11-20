#!/bin/sh
# The router's update log (E-APPLIST.W1, K14): the one writer of <main checkout>/.router/app/
# log.jsonl and state.json, the two files the local app reads (docs/local-app.html "Two sources").
#   start <router-name>                    wipe both files and begin a new run
#   stage <wu-id> <stage-word> [<tip>]     the router's shape of set wu/<wu-id>/stage
#   set <key> <json>                       one key's value (any JSON but null, at most 512 bytes)
#   del <key>                              remove one key (an absent key still writes its line)
#   alert <json object>                    headline, lines, optional wu and link; the state keeps 20
# The router runs start, stage and del in plain words (the router guard admits exactly those
# shapes); set and alert carry free JSON, so only programs with a fixed argv run them (the
# watchdog, the sender), as the inbox's post.
#
# Each change is one numbered JSON line, keys in the contract's order: run, n, at, op, then key
# (set, del), then value (set, alert). state.json is every value as it stands, the alerts kept,
# and the number of the last line folded in. Order: the words are checked and the change built
# with no lock and nothing written (a refusal exits 2); then, under one lock, n is read from the
# state, the new state is renamed into place, the line is appended to the log (past 2,000 lines a
# file holding only that line is renamed over the log), and the lock is released. The state goes
# first, so a reader that meets line n and reads the state finds n in it; a writer killed between
# the two leaves a gap the app reads the state for, never a repeated number.
#
# The lock is mkdir of .router/app/.lock, holding a directory named for the holder's pid; it is
# released by a trap on exit. A writer waits up to 5 s, then exits 1 naming it; a stale lock is
# never broken here (two breakers can each remove the other's live lock): the owner removes it.
# .router, .router/app and both files must not be symlinks (refused, exit 2): the script writes
# only inside the main checkout's .router/app. No environment override of the place, and no rm:
# a file is only ever replaced by a rename (the lock, a directory, is released with rmdir).
# Exits: 0 OK run=<run> n=<n>; 2 refused, nothing written; 1 lock or file error.
set -eu
for v in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
export LC_ALL=C
umask 077
HERE=$(cd "$(dirname "$0")" && pwd -P)
nl='
'
die() { echo "router-app: $*" >&2; exit 2; }
fail() { echo "router-app: $*" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || fail "jq is not on PATH"

# The words, checked by jq alone (no shell pattern reads a newline as an end): prints the change
# as one compact JSON object {op, key?, value?}, or "ERR <why>".
check='
def wu: "E-[A-Z0-9]{1,16}\\.W[0-9]{1,3}";
def id: "[a-z0-9][a-z0-9-]{0,40}";
def whole($re): type == "string" and test("\\A(?:" + $re + ")\\z");
def key_ok: whole("router|facts|wu/" + wu + "/(?:stage|conflict)|epic/E-[A-Z0-9]{1,16}/paused|question/" + id + "|worker/" + id + "(?:/report)?");
def parsed: try [fromjson] catch [];
def str($lo; $hi): type == "string" and length >= $lo and length <= $hi;
def alert_ok:
  type == "object"
  and ((keys - ["headline", "lines", "wu", "link"]) | length) == 0
  and (.headline | str(1; 80))
  and (.lines | type == "array" and length >= 1 and length <= 2 and all(.[]; str(1; 200)))
  and (if has("wu") then .wu | whole(wu) else true end)
  and (if has("link") then .link | whole("keel://[A-Za-z0-9._/-]{1,200}") else true end);
$ARGS.positional as $a
| ($a | length) as $n
| ($a[0] // "") as $verb
| if $verb == "start" then
    if $n != 2 then "ERR start <router-name>"
    elif ($a[1] | whole("[a-z0-9][a-z0-9-]{0,31}")) then {op: "start", name: $a[1]}
    else "ERR the router name must be [a-z0-9][a-z0-9-]{0,31}" end
  elif $verb == "stage" then
    if $n < 3 or $n > 4 then "ERR stage <wu-id> <plan-in-work|plan-to-read|plan-approved|building|in-review> [<tip>]"
    elif ($a[1] | whole(wu) | not) then "ERR the work unit must be E-<CODE>.W<m>"
    elif ($a[2] | whole("plan-in-work|plan-to-read|plan-approved|building|in-review") | not) then "ERR the stage must be plan-in-work|plan-to-read|plan-approved|building|in-review"
    elif $n == 4 and ($a[3] | whole("[0-9a-f]{7,40}") | not) then "ERR the tip must be 7 to 40 lowercase hex digits"
    else {op: "set", key: "wu/\($a[1])/stage", value: ({stage: ($a[2] | gsub("-"; " "))} + (if $n == 4 then {tip: $a[3]} else {} end))} end
  elif $verb == "set" then
    if $n != 3 then "ERR set <key> <json>"
    elif ($a[1] | key_ok | not) then "ERR unknown key"
    else ($a[2] | parsed) as $v
      | if ($v | length) != 1 then "ERR the value is not one JSON value"
        elif ($v[0] | tojson) == "null" then "ERR the value is null; del removes a key"
        elif ($v[0] | tojson | utf8bytelength) > 512 then "ERR the value is over 512 bytes"
        else {op: "set", key: $a[1], value: $v[0]} end
    end
  elif $verb == "del" then
    if $n != 2 then "ERR del <key>"
    elif ($a[1] | key_ok) then {op: "del", key: $a[1]}
    else "ERR unknown key" end
  elif $verb == "alert" then
    if $n != 2 then "ERR alert <json object>"
    else ($a[1] | parsed) as $v
      | if ($v | length) == 1 and ($v[0] | alert_ok) then {op: "alert", value: $v[0]}
        else "ERR an alert is {headline: 1-80 chars, lines: 1 or 2 strings of 1-200 chars, wu?: E-<CODE>.W<m>, link?: keel://<path>}" end
    end
  else "ERR the verb must be start|stage|set|del|alert" end
| if type == "string" then . else tojson end
'
change=$(jq -nr "$check" --args -- "$@") || fail "jq could not check the words"
case "$change" in
  ERR\ *) die "${change#ERR }" ;;
  "{"*) ;;
  *) fail "jq printed no change" ;;
esac
case "$change" in *"$nl"*) fail "jq printed more than one line" ;; esac

# The longest line this change can make: the longest run id (32 + 1 + 15 + 1 + 10), a 12-digit n
# and the 25-byte date. Over 1,024 bytes it is refused here, before the lock.
bytes=$(jq -nr --argjson c "$change" \
  '{run: ("x" * 59), n: 999999999999, at: ("x" * 25)} + ($c | del(.name)) | tojson | utf8bytelength') \
  || fail "jq could not measure the line"
[ "$bytes" -le 1024 ] || die "the line would be $bytes bytes; the cap is 1024"

common=$(git -C "$HERE" rev-parse --path-format=absolute --git-common-dir) \
  || fail "not inside a git checkout: $HERE"
ROOT=$(dirname "$common")
APP=$ROOT/.router/app
LOG=$APP/log.jsonl
STATE=$APP/state.json
LOCK=$APP/.lock

no_links() {
  for p in "$ROOT/.router" "$APP" "$LOG" "$STATE"; do
    [ ! -L "$p" ] || die "$p is a symlink; refused"
  done
  [ -d "$APP" ] || fail "$APP is not a directory"
  for p in "$LOG" "$STATE"; do
    [ ! -e "$p" ] || [ -f "$p" ] || fail "$p is not a regular file"
  done
}
for p in "$ROOT/.router" "$APP"; do
  [ ! -L "$p" ] || die "$p is a symlink; refused"
done
mkdir -p "$APP" || fail "cannot create $APP"
no_links

# Take the lock: up to 100 tries 0.05 s apart.
tries=0
until mkdir "$LOCK" 2>/dev/null; do
  tries=$((tries + 1))
  [ "$tries" -lt 100 ] || fail "LOCKED $LOCK: remove it if no router-app.sh runs"
  sleep 0.05
done
mkdir "$LOCK/$$" || { rmdir "$LOCK" 2>/dev/null; fail "cannot mark $LOCK"; }
# Only a lock still holding this pid's mark is released; one the owner removed is left alone.
trap 'rmdir "$LOCK/$$" 2>/dev/null && rmdir "$LOCK" 2>/dev/null; :' EXIT
trap 'exit 1' INT TERM HUP
no_links

at=$(date +%Y-%m-%dT%H:%M:%S%z | sed 's/\(..\)$/:\1/')
# A temp file is made only once nothing can refuse the change; one a failed write leaves is a
# dotfile the app never reads (no rm here).

case "$change" in
  '{"op":"start",'*)
    name=$(printf '%s' "$change" | jq -r .name) || fail "jq could not read the name"
    run=$name.$(date +%Y%m%d-%H%M%S).$$
    n=1
    new_state=$(mktemp "$APP/.state.XXXXXX") || fail "cannot create a file in $APP"
    new_log=$(mktemp "$APP/.log.XXXXXX") || fail "cannot create a file in $APP"
    jq -nc --arg run "$run" --arg at "$at" '{run: $run, n: 1, at: $at, values: {}, alerts: []}' > "$new_state" \
      || fail "cannot write $new_state"
    jq -nc --arg run "$run" --arg at "$at" '{run: $run, n: 1, at: $at, op: "start"}' > "$new_log" \
      || fail "cannot write $new_log"
    mv -f "$new_state" "$STATE" || fail "cannot place $STATE"
    mv -f "$new_log" "$LOG" || fail "cannot place $LOG"
    ;;
  *)
    [ -f "$STATE" ] || die "no run; start first"
    out=$(jq -r --argjson c "$change" --arg at "$at" '
      if type == "object" and (.run | type) == "string" and (.n | type) == "number"
        and (.values | type) == "object" and (.alerts | type) == "array"
      then (.n + 1) as $n
        | ({run, n: $n, at: $at} + $c) as $line
        | ($line | tojson),
          ({run, n: $n, at: $at,
            values: (if $c.op == "set" then .values + {($c.key): $c.value}
                     elif $c.op == "del" then .values | del(.[$c.key])
                     else .values end),
            alerts: (if $c.op == "alert" then (.alerts + [{n: $n, at: $at} + $c.value]) | .[-20:]
                     else .alerts end)} | tojson),
          "\(.run) \($n)"
      else error("not a state") end' "$STATE") || fail "cannot read $STATE (start again to begin a new run)"
    line=${out%%"$nl"*}
    rest=${out#*"$nl"}
    state=${rest%%"$nl"*}
    tail=${rest#*"$nl"}
    run=${tail% *}
    n=${tail##* }
    new_state=$(mktemp "$APP/.state.XXXXXX") || fail "cannot create a file in $APP"
    printf '%s\n' "$state" > "$new_state" || fail "cannot write $new_state"
    lines=0
    [ ! -f "$LOG" ] || lines=$(wc -l < "$LOG" | tr -d ' ')
    mv -f "$new_state" "$STATE" || fail "cannot place $STATE"
    if [ "$lines" -ge 2000 ]; then
      new_log=$(mktemp "$APP/.log.XXXXXX") || fail "cannot create a file in $APP"
      printf '%s\n' "$line" > "$new_log" || fail "cannot write $new_log"
      mv -f "$new_log" "$LOG" || fail "cannot place $LOG"
    else
      printf '%s\n' "$line" >> "$LOG" || fail "cannot append to $LOG"
    fi
    ;;
esac
echo "OK run=$run n=$n"
