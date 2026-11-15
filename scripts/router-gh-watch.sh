#!/bin/sh
# Router-only, read-only GitHub watch (E-GHFLOW.W3): the router hears its commanders through GitHub.
# The router runs it as a background Bash command (run_in_background, timeout 7200000), like
# scripts/router-watch.sh, so its exit wakes the router with what happened:
#   router-gh-watch.sh review  <pr> [since <id>|latest]   APPROVED | CHANGES-REQUESTED | COMMENTED | DISMISSED
#   router-gh-watch.sh comment <n>  [since <id>|latest]   COMMAND | MESSAGE | BADCOMMAND | EDITED (an issue or a pull request)
#   router-gh-watch.sh merged  <pr>                MERGED | CLOSED
#   router-gh-watch.sh checks  <pr>                CHECKS ... state=success|failure, once every check on the head is done
# It checks at once, then once per poll, until an event: one event line of k=v words, detail lines
# indented two spaces, then an `IGNORED from=other count=N` line when anyone else wrote meanwhile.
# Every event line carries at=<GitHub's time> (security review I2), so a replayed one shows its age;
# exit 0. TIMEOUT after ROUTER_WATCH_MAX s (default 6600, under the 2h background ceiling); UNREACHABLE
# after 5 failed polls in a row. Poll: ROUTER_WATCH_POLL s (default 60); both at most 9 digits. Bad
# arguments exit 2 before any poll; so does a refusal (not the bot's pull request, no commanders).
#
# Who is heard: a GitHub user id listed in [github] commanders (.keel/config.toml, committed; see
# scripts/router-gh.sh), account type User. The bot's own comments are skipped. Anyone else's comment
# or review is counted and never printed, and never ends the watch: a public repository gets spam,
# and every wake costs the router a turn. So is a comment an App posted for a commander
# (performed_via_github_app; security review N2; REST gives reviews no such field, so this covers
# comments only) - and a comment posted with a commander's own
# token is theirs to GitHub, so no such token is ever stored as a repository secret. Without `since`
# every comment or review counts; the watch reports the oldest and `pending=` the number after it, so
# the router re-arms with `since <id>` and reads each one once. `since latest` (a router that does not
# know where it stopped) skips everything present at the first poll. GitHub's newest 1000 comments or
# reviews are read (router-gh.sh; security review I1). When `since` falls before them, older items may
# be unread: the items read are still judged, and an event among them carries gap=yes oldest=<id>;
# with none, the watch prints OVERFLOW ... resume=<newest id read> and the router re-arms with
# `since <resume>` and tells the owner (round 2, F2). A comment deleted while the pages are read can
# shift a later one onto a page already read, so one poll may miss it; the next poll reads it (N-d). A comment whose updated_at differs from created_at prints EDITED and no text:
# an edit is never read as an order (REST names no editor), and one at or below `since` is never read
# again. A comment's first line is parsed by the closed grammar in scripts/router-gh-command.jq; the
# other lines are the commander's message. [github] free_text = false keeps only the commands: their
# further lines are dropped (text=dropped), and other comments count as IGNORED from=commander.
# A review's text is its author's message, never a command. REST shows no edit of a review's text
# (no updated_at; security review N4), so a review's text is read as it stands. DISMISSED names the
# review's author in from=, not who dismissed it (N3: REST does not say).
#
# Read-only, and it holds no secret: each poll runs `scripts/router-gh.sh watch-read <kind> <n>`
# (this checkout's), which mints the App's token, reads only an issue or pull request the bot opened,
# and prints one line of JSON. Nothing is written and nothing is kept between runs: no temp file, no
# state; the judgment is a pure function of GitHub's answer and the arguments. No git runs here.
# Detail lines are untrusted text: control characters (C0, DEL, C1, tab), line and paragraph
# separators, bidi marks, zero-width and invisible fillers, variation selectors, the BOM, interlinear
# annotation marks and tag characters (U+00AD, U+034F, U+061C, U+115F-1160, U+17B4-17B5, U+180E,
# U+200B-200F, U+2028-202E, U+2060-2069, U+3164, U+FE00-FE0F, U+FEFF, U+FFA0, U+FFF9-FFFB,
# U+E0000-E007F, U+E0100-E01EF) print as ?. A check's name and a status's
# context are third-party text: cut at 100 characters too (security review N1).
set -euf
for v in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
LC_ALL=C; export LC_ALL
die() { echo "router-gh-watch: $*" >&2; exit 2; }
shown() { printf '%s' "$1" | tr -d '\000-\037\177' | cut -c1-80; }
usage="usage: review <pr> [since <id>|latest] | comment <n> [since <id>|latest] | merged <pr> | checks <pr>"
DIR=$(cd "$(dirname "$0")" && pwd -P)
kind=${1:-}; [ $# -ge 1 ] && shift
case "$kind" in review|comment|merged|checks) ;; *) die "$usage";; esac
for a; do case "$a" in -*) die "an argument may not start with -: $(shown "$a")";; esac; done
since=0
case "$kind:$#" in
  *:1) ;;
  review:3|comment:3) [ "$2" = since ] || die "$usage"; since=$3
    [ "$since" = latest ] || expr "x$since" : 'x[1-9][0-9]\{0,14\}$' > /dev/null \
      || die "since must be latest or a comment or review id (a number, at most 15 digits): $(shown "$since")";;
  *) die "$usage";;
esac
n=$1
expr "x$n" : 'x[1-9][0-9]\{0,6\}$' > /dev/null || die "the issue or pull request must be a number: $(shown "$n")"
poll=${ROUTER_WATCH_POLL:-60}
max=${ROUTER_WATCH_MAX:-6600}
expr "$poll" : '[1-9][0-9]\{0,8\}$' > /dev/null || die "ROUTER_WATCH_POLL must be a positive integer, at most 9 digits: $poll"
expr "$max" : '[0-9]\{1,9\}$' > /dev/null || die "ROUTER_WATCH_MAX must be a non-negative integer, at most 9 digits: $max"

# The judgment: GitHub's answer (router-gh.sh watch-read) in, lines out. The first line is WAIT (no
# event yet) or the event line; detail lines follow, indented; IGNORED lines last.
judge='include "router-gh-command";
def clean: explode | map(if . < 32 or . == 127 or (. >= 128 and . < 160) or . == 173 or . == 847 or . == 1564
  or . == 4447 or . == 4448 or . == 6068 or . == 6069 or . == 6158 or (. >= 8203 and . <= 8207)
  or (. >= 8232 and . <= 8238) or (. >= 8288 and . <= 8297) or . == 12644 or (. >= 65024 and . <= 65039)
  or . == 65279 or . == 65440 or (. >= 65529 and . <= 65531) or (. >= 917504 and . <= 917631)
  or (. >= 917760 and . <= 917999) then 63 else . end) | implode;
def ts: if type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$") then . else "?" end;
def short: tostring | .[0:100];
def word: tostring | gsub("[^A-Za-z0-9._/-]"; "?");
def sha: if type == "string" and test("^[0-9a-f]{40}$") then . else "?" end;
def trimtail: if length > 0 and (.[-1] | test("^[ \t]*$")) then .[:-1] | trimtail else . end;
def heard($c): .user_type == "User" and (.user_id as $u | any($c[]; . == $u)) and (.self | not) and (.via_app | not);
# A gap: GitHub holds older pages than were read and `since` lies before the oldest item read. The
# items read are judged all the same: an event among them carries gap=yes oldest=<id>; with none,
# OVERFLOW names resume=<the newest id read>, everything readable having been read (round 2, F2).
def gapped($r; $what): ($r.truncated == true and (any($r.items[]; .id <= $since) | not)) as $g
  | ([$r.items[].id] | min // 0) as $old
  | if ($g | not) then .
    elif .ev == null then .ev = {line: "OVERFLOW \($what)=\($n) since=\($since) oldest=\($old) resume=\([$r.items[].id] | max // 0)", detail: []} | .ignored = []
    else .ev.line += " gap=yes oldest=\($old)" end;
def counted($k): if $k > 0 then "IGNORED from=other count=\($k)" else empty end;
. as $r
| if $r.kind == "comment" then
    ($r.items | map(select(.id > $since and (.self | not))) | sort_by(.id)) as $new
    | ($new | map(select(heard($r.commanders)))) as $mine
    | ($mine | map(. + {edited: (.updated_at != .created_at), cmd: (.body | keel_command)})
       | map(select(.edited or $r.free_text or .cmd.kind != "text"))) as $ev
    | (($mine | length) - ($ev | length)) as $muted
    | (if $r.pr then "pr" else "issue" end) as $what
    | {ignored: [counted(($new | length) - ($mine | length)),
        (if $muted > 0 then "IGNORED from=commander count=\($muted) why=free-text-off" else empty end)],
       ev: (if ($ev | length) == 0 then null else
         $ev[0] as $e
         | " pending=\(($ev | length) - 1)" as $p
         | "\($what)=\($n) comment=\($e.id) from=\($e.user_id) at=\($e.created_at | ts)" as $at
         | (if $r.free_text then "" else " text=dropped" end) as $td
         | if $e.edited then {line: "EDITED \($at)\($p)", detail: []}
           elif $e.cmd.kind == "command" then
             {line: ("COMMAND \($at) verb=\($e.cmd.verb)" + (if $e.cmd.verb == "answer" then " n=\($e.cmd.n) choice=\($e.cmd.choice)" else "" end) + $td + $p),
              detail: (if $r.free_text then $e.cmd.rest else [] end)}
           elif $e.cmd.kind == "bad" then
             {line: "BADCOMMAND \($at) why=\($e.cmd.why)\($td)\($p)",
              detail: ($e.body | keel_lines | if $r.free_text then . else .[:1] end)}
           else {line: "MESSAGE \($at)\($p)", detail: $e.cmd.rest} end end)} | gapped($r; $what)
  elif $r.kind == "review" then
    ($r.items | map(select(.id > $since and (.self | not) and .state != "PENDING")) | sort_by(.id)) as $new
    | ($new | map(select(heard($r.commanders)))) as $ev
    | {ignored: [counted(($new | length) - ($ev | length))],
       ev: (if ($ev | length) == 0 then null else
         $ev[0] as $e
         | ({APPROVED: "APPROVED", CHANGES_REQUESTED: "CHANGES-REQUESTED", COMMENTED: "COMMENTED", DISMISSED: "DISMISSED"}[$e.state] // "REVIEWED") as $w
         | {line: ("\($w) pr=\($n) sha=\($e.commit_id | sha) head=\($r.head | sha) review=\($e.id) from=\($e.user_id) at=\($e.submitted_at | ts)"
             + (if $r.free_text then "" else " text=dropped" end) + " pending=\(($ev | length) - 1)"),
            detail: (if $r.free_text then ($e.body | keel_lines) else [] end)} end)} | gapped($r; "pr")
  elif $r.kind == "merged" then
    {ignored: [],
     ev: (if $r.state != "closed" then null
       else (if $r.merged_by_id == null then "none"
             elif $r.merged_by_type == "User" and any($r.commanders[]; . == $r.merged_by_id) then $r.merged_by_id
             else "other" end) as $by
       | if $r.merged then {line: "MERGED pr=\($n) sha=\($r.merge_commit_sha | sha) head=\($r.head | sha) base=\($r.base | word) by=\($by) at=\($r.at | ts)", detail: []}
         else {line: "CLOSED pr=\($n) head=\($r.head | sha) base=\($r.base | word) at=\($r.at | ts)", detail: []} end end)}
  elif $r.kind == "checks" then
    {ignored: [],
     ev: (if ($r.runs | length) == 0 or any($r.runs[]; .status != "completed") or any($r.statuses[]; .state == "pending") then null
       else (if any($r.runs[]; .conclusion | IN("success", "neutral", "skipped") | not) or any($r.statuses[]; .state | IN("failure", "error"))
             then "failure" else "success" end) as $s
       | {line: "CHECKS pr=\($n) sha=\($r.head | sha) state=\($s) runs=\($r.runs | length) statuses=\($r.statuses | length) at=\([$r.runs[].completed_at | ts] | max | ts)",
          detail: ([$r.runs[] | "check \(.conclusion // "none" | word) \(.name // "" | short)"] + [$r.statuses[] | "status \(.state // "none" | word) \(.context // "" | short)"])} end)}
  else error("unknown kind") end
| if .ev == null then "WAIT" else .ev.line, (.ev.detail | trimtail | .[] | "  " + clean) end,
  (if (.ev.line // "" | startswith("OVERFLOW")) then empty else .ignored[] end)'

# a failed poll: count it, give up on the 5th in a row
fails=0 ign=
net_fail() {
  fails=$((fails + 1)); [ "$fails" -ge 5 ] || return 0
  echo "UNREACHABLE kind=$kind remote=github error=$(printf '%s\n' "$1" | sed -n '$p' | tr -c 'A-Za-z0-9 ._/:=@%,+^~\n-' ' ')"
  [ -z "$ign" ] || printf '%s\n' "$ign"
  exit 0
}

start=$(date +%s)
while :; do
  rc=0; out=$(sh "$DIR/router-gh.sh" watch-read "$kind" "$n" 2>&1) || rc=$?
  case "$rc" in
    0)
      # since latest: everything present at the first answer is skipped
      if [ "$since" = latest ]; then
        since=$(printf '%s\n' "$out" | jq '[.items[]?.id] | max // 0' 2> /dev/null) || { net_fail "GitHub's answer is not readable"; sleep "$poll"; continue; }
        expr "x$since" : 'x[0-9]\{1,16\}$' > /dev/null || { net_fail "GitHub's answer is not readable"; sleep "$poll"; continue; }
      fi
      if res=$(printf '%s\n' "$out" | jq -r -L "$DIR" --argjson since "$since" --arg n "$n" "$judge" 2>&1); then
        fails=0
        first=$(printf '%s\n' "$res" | sed -n 1p)
        if [ "$first" != WAIT ]; then printf '%s\n' "$res"; exit 0; fi
        ign=$(printf '%s\n' "$res" | sed 1d)
      else net_fail "GitHub's answer is not readable"; fi;;
    1) net_fail "$out";;
    *) printf '%s\n' "$out" >&2; exit 2;;
  esac
  waited=$(( $(date +%s) - start ))
  if [ "$waited" -ge "$max" ]; then
    echo "TIMEOUT kind=$kind waited=$((waited / 60))m"
    [ -z "$ign" ] || printf '%s\n' "$ign"
    exit 0
  fi
  sleep "$poll"
done
