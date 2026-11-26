#!/bin/sh
# Worker scratch helper (E-WSANDBOX.W4, owner q5 2026-11-26; DEC-46): the one fixed shape a
# worker uses to set up a throwaway repository under $TMPDIR - a fresh mktemp directory, a git
# repo in it, and `keel init` in it. It exists because the auto reviewer judged that harmless
# shape risky (36 classifier denials, the "scratch repo" class); the helper is on the worker's
# grant, so the reviewer no longer judges it. It is not a sandbox: it only ever works under
# $TMPDIR, which the sandbox already lets a worker write, and it never touches the worktree.
#   router-scratch.sh new          make one fresh dir (mktemp -d), initialise a git repo in it,
#                                  and print its path
#   router-scratch.sh init <dir>   run `keel init` in a dir `new` made, and nowhere else
# `init` refuses a path that is not a fresh helper dir: outside $TMPDIR, the temp folder itself,
# a symlink, a name mktemp did not make, a missing marker, or a dir holding anything but the
# marker and .git. It never runs before that check passes, so a refused path is not touched.
set -eu
die() { echo "router-scratch: $*" >&2; exit 2; }
usage() { die "usage: router-scratch.sh new | init <dir>"; }
marker=.keel-scratch
tmp=${TMPDIR:-/tmp}
[ $# -ge 1 ] || usage
case $1 in
  new)
    [ $# -eq 1 ] || usage
    d=$(mktemp -d "${tmp%/}/keel-scratch.XXXXXXXX") || die "cannot create a temp dir under $tmp"
    : > "$d/$marker"
    git -C "$d" init -q || die "git init failed in $d"
    printf '%s\n' "$d"
    ;;
  init)
    [ $# -eq 2 ] || usage
    [ -e "$2" ] || die "no such directory: $2"
    [ -d "$2" ] || die "not a directory: $2"
    [ ! -L "$2" ] || die "a symlink is not a fresh scratch dir: $2"
    real=$(cd -P "$2" 2>/dev/null && pwd -P) || die "cannot resolve: $2"
    base=$(cd -P "$tmp" 2>/dev/null && pwd -P) || die "cannot resolve the temp folder: $tmp"
    case $real in
      "$base"/keel-scratch.????????) ;;
      *) die "$2 is not a fresh helper scratch dir under $base" ;;
    esac
    [ "$real" != "$base" ] || die "$2 is the temp folder itself"
    [ -f "$real/$marker" ] && [ ! -L "$real/$marker" ] || die "$2 was not made by router-scratch.sh new"
    for e in "$real"/* "$real"/.[!.]* "$real"/..?*; do
      [ -e "$e" ] || [ -L "$e" ] || continue
      case ${e##*/} in
        "$marker" | .git) ;;
        *) die "$2 is not fresh: it holds ${e##*/}" ;;
      esac
    done
    cd "$real" || die "cannot enter $2"
    keel init
    ;;
  *) usage ;;
esac
