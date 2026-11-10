#!/bin/sh
# Router-only, read-only owner-action watch (E-SEED2.W12). The router runs it as a background Bash
# command (run_in_background, timeout 7200000), so its exit wakes the router with what landed:
#   router-watch.sh commit <branch> <old-tip> [<path>...]  COMMITTED | COMMITTED-PARTIAL | REWRITTEN | DELETED
#   router-watch.sh signed <tip>                           SIGNED + the mr note, indented
#   router-watch.sh merged <tip> <target-branch>           MERGED ... how=no-ff|ff|contained | DELETED target=<b>
#   router-watch.sh pushed <tip> <branch>                  PUSHED (git ls-remote + fetch origin, then ancestry)
#   router-watch.sh tagged <tag> [<sha>]                   TAGGED | TAGGED-MISMATCH (git ls-remote origin)
# It checks at once (the owner may already have acted), then once per poll until the state holds:
# one event line, detail lines indented two spaces, exit 0. TIMEOUT after ROUTER_WATCH_MAX s (default
# 6600, under the 2h background ceiling); UNREACHABLE after 5 failed network attempts in a row.
# Poll: ROUTER_WATCH_POLL s (default 15, 60 for pushed/tagged); both knobs at most 9 digits.
# Bad arguments exit 2 before any poll. A branch or tag missing on origin means "not yet", not a network failure.
# Only `git fetch` writes: refs/remotes/origin/<branch> and objects (no tags, no FETCH_HEAD, no
# maintenance). The remote is always origin. Detail lines are untrusted text, control characters as ?.
set -eu
# bytes, not characters: an invalid UTF-8 byte in a note or a git error must not stop sed or tr
export LC_ALL=C
die() { echo "router-watch: $*" >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
g() { git -C "$ROOT" "$@"; }
kind=${1:-}; [ $# -ge 1 ] && shift
case "$kind" in
  commit) [ $# -ge 2 ] || die "commit <branch> <old-tip> [<path>...]";;
  signed) [ $# -eq 1 ] || die "signed <tip>";;
  merged) [ $# -eq 2 ] || die "merged <tip> <target>";;
  pushed) [ $# -eq 2 ] || die "pushed <tip> <branch>";;
  tagged) [ $# -eq 1 ] || [ $# -eq 2 ] || die "tagged <tag> [<sha>]";;
  *) die "kind must be commit|signed|merged|pushed|tagged";;
esac
for a; do case "$a" in -*) die "argument may not start with -: $a";; esac; done
case "$kind" in pushed|tagged) poll=${ROUTER_WATCH_POLL:-60};; *) poll=${ROUTER_WATCH_POLL:-15};; esac
max=${ROUTER_WATCH_MAX:-6600}
expr "$poll" : '[1-9][0-9]\{0,8\}$' >/dev/null || die "ROUTER_WATCH_POLL must be a positive integer, at most 9 digits: $poll"
expr "$max" : '[0-9]\{1,9\}$' >/dev/null || die "ROUTER_WATCH_MAX must be a non-negative integer, at most 9 digits: $max"
sha() {
  expr "$1" : '[0-9a-f]\{7,40\}$' >/dev/null && g rev-parse --verify --quiet "$1^{commit}" 2>/dev/null || die "not a commit sha: $1"
}
branch() {
  g check-ref-format --branch "$1" >/dev/null 2>&1 && g show-ref --verify --quiet "refs/heads/$1" || die "no branch $1"
}
case "$kind" in
  commit) branch "$1"; b=$1; old=$(sha "$2"); shift 2
    for p; do case "$p" in /*|*..*) die "bad path: $p";; esac; done;;
  signed) t=$(sha "$1");;
  merged) t=$(sha "$1"); branch "$2"; b=$2;;
  pushed) t=$(sha "$1"); b=$2
    g check-ref-format --branch "$b" >/dev/null 2>&1 &&
      { g show-ref --verify --quiet "refs/heads/$b" || g show-ref --verify --quiet "refs/remotes/origin/$b"; } || die "no branch $b";;
  tagged) g check-ref-format "refs/tags/$1" || die "bad tag $1"; tag=$1; want=
    [ $# -eq 1 ] || want=$(sha "$2");;
esac

# detail lines carry commit and note text verbatim but for control characters: C0 and DEL, UTF-8 C1
# (U+0080-009F), line/paragraph separators and bidi overrides (U+2028-202E, U+2066-2069)
c1=$(printf '\302[\200-\237]') sep=$(printf '\342\200[\250-\256]') iso=$(printf '\342\201[\246-\251]')
clean() { sed "s/[[:cntrl:]]/?/g; s/$c1/?/g; s/$sep/?/g; s/$iso/?/g"; }

# ponytail: a stalled http transfer aborts after 120s; ssh has no bound short of the 2h background ceiling
net() { g -c http.lowSpeedLimit=1 -c http.lowSpeedTime=120 "$@"; }

# a failed network attempt: count it, give up on the 5th in a row
fails=0
net_fail() {
  fails=$((fails + 1)); [ "$fails" -ge 5 ] || return 0
  echo "UNREACHABLE kind=$kind remote=origin error=$(printf '%s\n' "$1" | sed -n '$p' | tr -c 'A-Za-z0-9 ._/:=@%,+^~\n-' ' ')"
  exit 0
}

check_commit() {
  new=$(g rev-parse --verify --quiet "refs/heads/$b") || { echo "DELETED branch=$b from=$old"; return 0; }
  [ "$new" != "$old" ] || return 1
  if ! g merge-base --is-ancestor "$old" "$new" 2>/dev/null; then echo "REWRITTEN branch=$b from=$old to=$new"; return 0; fi
  st=$(g diff --name-status --no-renames "$old" "$new")
  landed=$(printf '%s\n' "$st" | cut -f2-)
  missing= k=0
  for p; do
    if printf '%s\n' "$landed" | grep -Fxq -- "$p"; then k=$((k + 1)); else missing="$missing  missing $p
"; fi
  done
  unexpected=
  [ $# -eq 0 ] || unexpected=$(printf '%s\n' "$landed" | while IFS= read -r l; do
    [ -n "$l" ] || continue
    hit=; for p; do [ "$p" != "$l" ] || hit=1; done
    [ -n "$hit" ] || echo "  unexpected $l"
  done)
  word=COMMITTED; [ -z "$missing" ] || word=COMMITTED-PARTIAL
  echo "$word branch=$b from=$old to=$new commits=$(g rev-list --count "$old..$new") expected=$k/$#"
  g log --reverse --format='%H%x1f%(trailers:key=Reviewed-By,valueonly,unfold,separator=%x2C)%x1f%s' "$old..$new" |
    while IFS=$(printf '\037') read -r c r s; do echo "  commit $c reviewed-by=${r:-none} $s"; done | clean
  [ -z "$st" ] || printf '%s\n' "$st" | while IFS='	' read -r s p; do echo "  landed $s $p"; done | clean
  printf '%s' "$missing" | clean; [ -z "$unexpected" ] || printf '%s\n' "$unexpected" | clean
}

check_signed() {
  n=$(g log -1 --notes=mr --format=%N "$t")
  [ -n "$n" ] || return 1
  echo "SIGNED tip=$t"; printf '%s\n' "$n" | clean | sed 's/^/  /'
}

check_merged() {
  at=$(g rev-parse --verify --quiet "refs/heads/$b") || { echo "DELETED target=$b"; return 0; }
  g merge-base --is-ancestor "$t" "$at" || return 1
  if [ "$at" = "$t" ]; then how=ff
  elif [ "$(g rev-parse --verify --quiet "$at^2")" = "$t" ]; then how=no-ff
  else how=contained; fi
  echo "MERGED tip=$t target=$b at=$at how=$how"
}

check_pushed() {
  if ! out=$(net ls-remote --heads origin "refs/heads/$b" 2>&1); then net_fail "$out"; return 1; fi
  r=$(printf '%s\n' "$out" | awk -v r="refs/heads/$b" '$2 == r { print $1 }')
  [ -n "$r" ] || { fails=0; return 1; }
  if [ "$(g rev-parse --verify --quiet "refs/remotes/origin/$b")" != "$r" ] &&
    ! err=$(net -c remote.origin.followRemoteHEAD=never fetch -q --no-tags --no-write-fetch-head --no-auto-maintenance --no-recurse-submodules origin -- "+refs/heads/$b:refs/remotes/origin/$b" 2>&1); then net_fail "$err"; return 1; fi
  fails=0
  g merge-base --is-ancestor "$t" "refs/remotes/origin/$b" 2>/dev/null || return 1
  echo "PUSHED tip=$t branch=$b origin=$(g rev-parse "refs/remotes/origin/$b")"
}

check_tagged() {
  if ! out=$(net ls-remote --tags origin "refs/tags/$tag" "refs/tags/$tag^{}" 2>&1); then net_fail "$out"; return 1; fi
  fails=0
  obj=$(printf '%s\n' "$out" | awk -v r="refs/tags/$tag" '$2 == r { print $1 }')
  [ -n "$obj" ] || return 1
  peeled=$(printf '%s\n' "$out" | awk -v r="refs/tags/$tag^{}" '$2 == r { print $1 }')
  commit=${peeled:-$obj}
  if [ -n "$want" ] && [ "$commit" != "$want" ]; then
    echo "TAGGED-MISMATCH tag=$tag object=$obj commit=$commit expected=$want"
  else echo "TAGGED tag=$tag object=$obj commit=$commit"; fi
}

start=$(date +%s)
while :; do
  "check_$kind" "$@" && exit 0
  waited=$(( $(date +%s) - start ))
  [ "$waited" -lt "$max" ] || { echo "TIMEOUT kind=$kind waited=$((waited / 60))m"; exit 0; }
  sleep "$poll"
done
