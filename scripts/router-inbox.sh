#!/bin/sh
# Router report channel (E-RGUARD.W3): one message is one complete file under
# <main checkout>/.router/inbox. A worker posts, the router's armed watcher prints
# it, and consume moves the file to read/ as the durable ack:
#   init                          create inbox/ and inbox/read/
#   post <from> <kind> [<text>...] worker -> router; stdin is the body if no text
#   watch [<max-s>]               block until one message; print it, exit 0; TIMEOUT exit 1
#   consume <id>                  ack: move inbox/<id>.msg to inbox/read/
#   status                        counts; read-only
#
# Delivery is at-least-once: the watcher never writes, consume is the ack, and the
# id is what the router quotes back. A post writes a dotfile and renames it into
# place, so a crash leaves no partial message and the watcher never sees one. Order
# is the id's lexicographic order (epoch seconds first, then a random suffix: two
# messages in one second have no write order). Two writers cannot collide (mktemp
# names); two watchers can both print one file, which is a duplicate, never a loss.
# One predicate, id_ok, is the id grammar consume accepts and the name a message may
# carry; message_ok is exactly the set watch prints and status counts, so a hostile
# name cannot starve the queue and cannot be a message consume refuses.
# The reader caps the body by bytes (ROUTER_INBOX_MAX_BYTES) as well as by lines, and
# bytes= is the cleaned, capped body it printed, never the file's size.
# No delete verb and no redirect into a system path: the host's auto review denies a
# script carrying either, so cleanup of read/ is a manual move.
set -eu
for v in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
# bytes, not characters: a body may carry a byte that is not valid UTF-8
export LC_ALL=C
HERE=$(cd "$(dirname "$0")" && pwd -P)
die() { echo "router-inbox: $*" >&2; exit 2; }
verb=${1:-}
[ $# -ge 1 ] && shift
[ -n "$verb" ] || die "verb must be init|post|watch|consume|status"
if [ -n "${ROUTER_INBOX_DIR:-}" ]; then
  INBOX=$ROUTER_INBOX_DIR
else
  common=$(git -C "$HERE" rev-parse --path-format=absolute --git-common-dir) \
    || die "not inside a git checkout: $HERE"
  INBOX=$(dirname "$common")/.router/inbox
fi
READ=$INBOX/read
poll=${ROUTER_INBOX_POLL:-2}
maxbytes=${ROUTER_INBOX_MAX_BYTES:-4000}
expr "$poll" : '[1-9][0-9]\{0,6\}$' >/dev/null || die "ROUTER_INBOX_POLL must be a positive integer: $poll"
expr "$maxbytes" : '[1-9][0-9]\{0,6\}$' >/dev/null || die "ROUTER_INBOX_MAX_BYTES must be a positive integer: $maxbytes"

# Every field printed is untrusted: C0 and DEL, UTF-8 C1 (U+0080-009F), line and
# paragraph separators and bidi overrides (U+2028-202E, U+2066-2069) print as ?.
c1=$(printf '\302[\200-\237]') sep=$(printf '\342\200[\250-\256]') iso=$(printf '\342\201[\246-\251]')
clean() { sed "s/[[:cntrl:]]/?/g; s/$c1/?/g; s/$sep/?/g; s/$iso/?/g"; }

# The one id grammar: consume accepts it, and a message file may carry it as its name.
id_ok() { expr "$1" : '[A-Za-z0-9][A-Za-z0-9._-]*$' >/dev/null; }
kind_ok() {
  case "$1" in
    REPORT|QUESTION|BLOCKED|DONE|NOTE) return 0 ;;
  esac
  return 1
}
# Exactly what watch prints and status counts: a regular, never-followed file whose
# name is an id, whose id: line matches that name, and whose kind: is one of the five.
message_ok() {
  [ ! -L "$1" ] || return 1
  [ -f "$1" ] || return 1
  stem=${1##*/}
  stem=${stem%.msg}
  id_ok "$stem" || return 1
  [ "$(sed -n '1s/^id: //p' "$1")" = "$stem" ] || return 1
  kind_ok "$(sed -n '3s/^kind: //p' "$1")"
}

show() {
  p=$1
  file=${p##*/}
  id=${file%.msg}
  from=$(sed -n '2s/^from: //p' "$p" | clean | head -c 64)
  kind=$(sed -n '3s/^kind: //p' "$p")
  epoch=${id%%-*}
  case "$epoch" in ''|*[!0-9]*) epoch=0;; esac
  age=$(( $(date +%s) - epoch ))
  [ "$age" -ge 0 ] || age=0
  cut=
  [ "$(tail -n +6 "$p" | head -c $((maxbytes + 1)) | wc -c | tr -d ' ')" -le "$maxbytes" ] || cut=1
  more=
  [ -z "$(sed -n '86p' "$p")" ] || more=1
  # bytes= is what the watcher printed: cleaned, capped, and no larger than maxbytes
  bytes=$(tail -n +6 "$p" | head -c "$maxbytes" | sed -n '1,80p' | clean | wc -c | tr -d ' ')
  echo "MSG id=$id from=${from:-?} kind=${kind:-?} age=${age}s bytes=$bytes"
  tail -n +6 "$p" | head -c "$maxbytes" | sed -n '1,80p' | clean | sed 's/^/  /'
  if [ -n "$cut" ]; then
    # a byte cap can end mid-line: start the marker on its own line
    printf '\n  … truncated at %s bytes (read %s)\n' "$maxbytes" "$p"
  elif [ -n "$more" ]; then
    echo "  … more body lines (read $p)"
  fi
}

case "$verb" in
  init)
    [ $# -eq 0 ] || die "init takes no arguments"
    mkdir -p "$INBOX" "$READ" || die "cannot create $INBOX"
    echo "READY dir=$INBOX read=$READ"
    ;;
  post)
    [ $# -ge 2 ] || die "post <from> <kind> [<text>...]"
    from=$1
    kind=$2
    shift 2
    expr "$from" : '[A-Za-z0-9][A-Za-z0-9._-]*$' >/dev/null || die "from must be a worker name: $from"
    [ "${#from}" -le 64 ] || die "from is longer than 64 characters"
    kind_ok "$kind" || die "kind must be REPORT|QUESTION|BLOCKED|DONE|NOTE: $kind"
    for a; do
      case "$a" in -*) die "body may not start with -: $a" ;; esac
    done
    if [ $# -gt 0 ]; then body=$*; else body=$(cat); fi
    [ -n "$body" ] || die "empty body"
    bytes=$(printf '%s' "$body" | wc -c | tr -d ' ')
    [ "$bytes" -le "$maxbytes" ] || die "body is $bytes bytes; limit is $maxbytes"
    mkdir -p "$INBOX" "$READ" || die "cannot create $INBOX"
    tmp=$(mktemp "$INBOX/.new.XXXXXX") || die "cannot create a message file in $INBOX"
    id="$(date +%s)-$(basename "$tmp" | sed 's/^\.new\.//')"
    {
      printf 'id: %s\nfrom: %s\nkind: %s\nwhen: %s\n---\n' \
        "$id" "$from" "$kind" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
      printf '%s\n' "$body"
    } > "$tmp" || die "cannot write a message file in $INBOX"
    mv "$tmp" "$INBOX/$id.msg" || die "cannot place the message in $INBOX"
    echo "POSTED id=$id"
    ;;
  watch)
    [ $# -le 1 ] || die "watch [<max-s>]"
    max=${1:-${ROUTER_INBOX_MAX:-6600}}
    expr "$max" : '[0-9]\{1,7\}$' >/dev/null || die "max must be a non-negative integer: $max"
    [ -d "$INBOX" ] || die "NO-INBOX $INBOX (run: router-inbox.sh init)"
    start=$(date +%s)
    while :; do
      pick=
      for p in "$INBOX"/*.msg; do
        message_ok "$p" || continue
        pick=$p
        break
      done
      if [ -n "$pick" ]; then
        show "$pick"
        exit 0
      fi
      waited=$(( $(date +%s) - start ))
      if [ "$waited" -ge "$max" ]; then
        echo "TIMEOUT kind=inbox waited=$((waited / 60))m poll=${poll}s"
        exit 1
      fi
      sleep "$poll"
    done
    ;;
  consume)
    [ $# -eq 1 ] || die "consume <id>"
    id=$1
    id_ok "$id" || die "bad id: $id"
    if [ -f "$READ/$id.msg" ]; then
      echo "CONSUMED-ALREADY id=$id"
      exit 0
    fi
    if [ -L "$INBOX/$id.msg" ]; then die "MISSING id=$id"; fi
    [ -f "$INBOX/$id.msg" ] || die "MISSING id=$id"
    mkdir -p "$READ" || die "cannot create $READ"
    # a losing mv in a duplicate-consume race must not leak stderr before the ack
    if err=$(mv "$INBOX/$id.msg" "$READ/$id.msg" 2>&1); then
      echo "CONSUMED id=$id"
    elif [ -f "$READ/$id.msg" ]; then
      echo "CONSUMED-ALREADY id=$id"
    else
      die "MISSING id=$id (lost during move: $err)"
    fi
    ;;
  status)
    [ $# -eq 0 ] || die "status takes no arguments"
    [ -d "$INBOX" ] || die "NO-INBOX $INBOX"
    n=0
    oldest=
    for p in "$INBOX"/*.msg; do
      message_ok "$p" || continue
      n=$((n + 1))
      [ -n "$oldest" ] || oldest=${p##*/}
    done
    r=0
    for p in "$READ"/*.msg; do
      if [ -f "$p" ]; then r=$((r + 1)); fi
    done
    t=0
    for p in "$INBOX"/.new.*; do
      if [ -f "$p" ]; then t=$((t + 1)); fi
    done
    echo "INBOX dir=$INBOX pending=$n oldest=${oldest:-none} read=$r orphans=$t"
    ;;
  *) die "unknown verb: $verb" ;;
esac
