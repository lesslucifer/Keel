#!/bin/sh
# Router report channel (E-RGUARD.W3): one message is one complete file under
# <main checkout>/.router/inbox. A worker posts, the router's armed watcher prints
# it, and consume moves the file to read/ as the durable ack:
#   init                          create inbox/ and inbox/read/
#   post <from> <kind> [<text>...] worker -> router; stdin is the body if no text
#   watch [<max-s>]               block until one message; print it, exit 0; TIMEOUT exit 1
#   consume <id>                  ack: move inbox/<id>.msg to inbox/read/
#   status                        counts, junk included; read-only
#
# Delivery is at-least-once: the watcher never writes, consume is the ack, and the
# id is what the router quotes back. A post writes a dotfile and renames it into
# place, so a crash leaves no partial message and the watcher never sees one. Order
# is the id's lexicographic order (epoch seconds first, then a random suffix: two
# messages in one second have no write order). Two writers cannot collide (mktemp
# names); two watchers can both print one file, which is a duplicate, never a loss.
# One predicate, id_ok, is the id grammar consume accepts and the name a message may
# carry (the router guard holds the same grammar; tests/router.rs ties the two);
# message_ok is exactly what post can write, so it is what watch prints and status
# counts: a hostile file cannot starve the queue or forge a header field. A file it
# refuses is junk: one watch judges it once, and status counts it for the owner.
# Only a header's first 512 bytes and a body's first ROUTER_INBOX_MAX_BYTES + 2 are
# read, so a large file costs no more than a small one. The body shown is at most
# that many bytes and 80 lines, and bytes= is the cleaned body it printed.
# No delete verb and no redirect into a system path: the host's auto review denies a
# script carrying either, so cleanup of read/ is a manual move.
set -eu
for v in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
# bytes, not characters: a body may carry a byte that is not valid UTF-8
export LC_ALL=C
HERE=$(cd "$(dirname "$0")" && pwd -P)
nl='
'
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

# Every field printed is untrusted: C0 and DEL, UTF-8 C1 (U+0080-009F), zero-width and
# direction marks (U+200B-200F, U+FEFF), line and paragraph separators and bidi
# overrides (U+2028-202E, U+2066-2069) print as ?.
c1=$(printf '\302[\200-\237]') zw=$(printf '\342\200[\213-\217]') bom=$(printf '\357\273\277')
sep=$(printf '\342\200[\250-\256]') iso=$(printf '\342\201[\246-\251]')
clean() { sed "s/[[:cntrl:]]/?/g; s/$c1/?/g; s/$zw/?/g; s/$bom/?/g; s/$sep/?/g; s/$iso/?/g"; }

# The one id grammar: consume accepts it, and a message file may carry it as its name.
# No "..": the router guard refuses any scripts/ command that holds one.
id_ok() {
  case "$1" in *..*) return 1 ;; esac
  expr "$1" : '[A-Za-z0-9][A-Za-z0-9._-]*$' >/dev/null
}
# The sender's grammar, the one post writes.
name_ok() { expr "$1" : '[A-Za-z0-9][A-Za-z0-9._-]\{0,63\}$' >/dev/null; }
kind_ok() {
  case "$1" in
    REPORT|QUESTION|BLOCKED|DONE|NOTE) return 0 ;;
  esac
  return 1
}
# Exactly what watch prints and status counts: a regular, never-followed file whose
# name is an id, whose id: line matches that name, whose from: is a name post takes,
# and whose kind: is one of the five. It sets m_id, m_from and m_kind for show.
message_ok() {
  [ ! -L "$1" ] || return 1
  [ -f "$1" ] || return 1
  m_id=${1##*/}
  m_id=${m_id%.msg}
  id_ok "$m_id" || return 1
  hdr=$(head -c 512 "$1" | sed -n '1s/^id: //p;2s/^from: //p;3s/^kind: //p')
  case "$hdr" in *"$nl"*"$nl"*"$nl"*) return 1 ;; *"$nl"*"$nl"*) ;; *) return 1 ;; esac
  [ "${hdr%%"$nl"*}" = "$m_id" ] || return 1
  m_kind=${hdr##*"$nl"}
  m_from=${hdr#*"$nl"}
  m_from=${m_from%"$nl"*}
  name_ok "$m_from" && kind_ok "$m_kind"
}

# Prints the message message_ok just accepted. The body is everything after the five
# header lines less the one newline post adds, read once, at most maxbytes + 2 bytes.
show() {
  p=$1
  # epoch seconds, decimal: a leading zero is not octal, and anything else is age 0
  epoch=${m_id%%-*}
  expr "$epoch" : '[1-9][0-9]\{0,11\}$' >/dev/null || epoch=
  age=0
  [ -z "$epoch" ] || age=$(( $(date +%s) - epoch ))
  [ "$age" -ge 0 ] || age=0
  raw=$(tail -n +6 "$p" | head -c $((maxbytes + 2)) | tr -d '\000'; echo .)
  raw=${raw%.}
  body=${raw%"$nl"}
  cut=
  if [ "${#body}" -gt "$maxbytes" ]; then
    cut=1
    body=$(printf '%s' "$body" | head -c "$maxbytes"; echo .)
    body=${body%.}
  fi
  more=
  [ "$(printf '%s\n' "$body" | sed -n '$=')" -le 80 ] || more=1
  shown=$(printf '%s\n' "$body" | sed -n '1,80p' | clean)
  echo "MSG id=$m_id from=$m_from kind=$m_kind age=${age}s bytes=${#shown}"
  printf '%s\n' "$shown" | sed 's/^/  /'
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
    [ "${#from}" -le 64 ] || die "from is longer than 64 characters"
    name_ok "$from" || die "from must be a worker name: $from"
    kind_ok "$kind" || die "kind must be REPORT|QUESTION|BLOCKED|DONE|NOTE: $kind"
    for a; do
      case "$a" in -*) die "body may not start with -: $a" ;; esac
    done
    if [ $# -gt 0 ]; then
      body=$*
    else
      # read no more than the cap needs: maxbytes + 2 is over it even with a final newline
      body=$(head -c $((maxbytes + 2)); echo .)
      body=${body%.}
      [ "${#body}" -le $((maxbytes + 1)) ] || die "body is over $maxbytes bytes; limit is $maxbytes"
      while :; do case "$body" in *"$nl") body=${body%"$nl"} ;; *) break ;; esac; done
    fi
    [ -n "$body" ] || die "empty body"
    bytes=${#body}
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
    # names judged junk in this watch, /-separated (a file name holds no /): each is
    # read once, not on every poll
    junk=/
    while :; do
      pick=
      for p in "$INBOX"/*.msg; do
        f=${p##*/}
        case "$junk" in *"/$f/"*) continue ;; esac
        if message_ok "$p"; then
          pick=$p
          break
        fi
        [ ! -e "$p" ] && [ ! -L "$p" ] || junk="$junk$f/"
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
    if [ -L "$INBOX/$id.msg" ]; then die "MISSING id=$id"; fi
    # a pending file is moved even when read/ already holds that id (a copy put back),
    # so it cannot be re-delivered ahead of every later message
    if [ ! -f "$INBOX/$id.msg" ]; then
      [ -f "$READ/$id.msg" ] || die "MISSING id=$id"
      echo "CONSUMED-ALREADY id=$id"
      exit 0
    fi
    mkdir -p "$READ" || die "cannot create $READ"
    # a losing mv in a duplicate-consume race must not leak stderr before the ack
    if err=$(mv -f "$INBOX/$id.msg" "$READ/$id.msg" 2>&1); then
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
    j=0
    oldest=
    for p in "$INBOX"/*.msg; do
      if message_ok "$p"; then
        n=$((n + 1))
        [ -n "$oldest" ] || oldest=$m_id
      elif [ -e "$p" ] || [ -L "$p" ]; then
        j=$((j + 1))
      fi
    done
    r=0
    for p in "$READ"/*.msg; do
      if [ -f "$p" ]; then r=$((r + 1)); fi
    done
    t=0
    for p in "$INBOX"/.new.*; do
      if [ -f "$p" ]; then t=$((t + 1)); fi
    done
    echo "INBOX dir=$INBOX pending=$n oldest=${oldest:-none} read=$r orphans=$t junk=$j"
    ;;
  *) die "unknown verb: $verb" ;;
esac
