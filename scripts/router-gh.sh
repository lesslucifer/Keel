#!/bin/sh
# The router's one door to GitHub (E-GHFLOW.W2, DEC-46 amended by the owner 2026-11-15: the router bot
# pushes non-main branches). It acts as a GitHub App installed on this repository only, never as the
# owner: the owner cannot approve a pull request they authored. The router's Bash hook admits only
# these shapes, and every argument is validated here before any other step:
#   router-gh.sh push     <branch>                         wu/<E>/<slug> or epic/<E>, with every notes ref
#   router-gh.sh pr-open  <branch> [--base main|epic/<E>]  open a draft pull request, or refresh the bot's open one: an epic into
#                                                          main or --base; a WU into its epic; a task into its WU branch
#   router-gh.sh pr-ready <n>                              mark the bot's pull request ready for review; turn on
#                                                          auto-merge (a merge commit) when [github] merge_on_approve
#   router-gh.sh post     <n> <body-name>                  post <worktree of its head>/.router-gh/<body-name>.md
#   router-gh.sh sync     <epic/E|main|wu/E/slug>          fetch GitHub's branch; fast-forward it here unless checked out (never a task)
#   router-gh.sh issue    <branch>                         open or refresh the tracking issue of an epic, or a WU's sub-issue
# A task (E-GHFLOW2.W6) is wu/<E>/<wu>_<task>, three words without _: a piece of the WU wu/<E>/<wu>
# that waits on something, in its own pull request into that WU branch, which the owner merges. It
# gets no gate, notes, auto-merge, sync or issue of its own; nothing here merges a pull request.
#   (router-gh.sh credential <file> <op>: git's one-shot credential helper only; never a router shape)
#   (router-gh.sh watch-read <kind> <n>: scripts/router-gh-watch.sh's read of GitHub; never a router shape)
# Exit 2 = refused (arguments, setup, a precondition); exit 1 = GitHub or git refused the request.
# Output: one KEY line (PUSHED, PR, READY, POSTED, SYNCED, ISSUE) of k=v words; WARN lines; errors on
# stderr. watch-read prints one line of JSON instead (E-GHFLOW.W3).
#
# Who the router hears (E-GHFLOW.W3): [github] commanders, numeric GitHub user ids, and free_text, in
# .keel/config.toml. They are read from this checkout's file, which no worker can write, and only
# while its bytes are HEAD's: HEAD alone is a ref a worker can move in the shared git dir. pr-ready
# asks each commander for a review; watch-read hands the list to the watch, which judges. The same
# file's [github] merge_on_approve (true when absent; E-GHFLOW2.W4) makes pr-ready turn on GitHub's
# auto-merge as a merge commit, so the owner's approval lands a green pull request; the rulesets, not
# the bot, decide when it may merge.
#
# Secrets. The App's private key is read only here, from $HOME/.config/keel/router-gh/app.pem (a
# regular file, not a link, the user's, mode 0600 or 0400, outside every worktree of this repository
# and its git dir, with one link), in a directory of the user's that no one else may write, next to
# app.conf (app_id=, installation_id=, app_slug= lines). The bound on workers is the sandbox's denyRead
# of that directory (scripts/router-worker.sh; the Read rule and scripts/router-file-guard.sh cover the
# file tools); the launcher's Bash deny of this script states intent only (a prefix rule). Each call
# signs a 10-minute JWT with the key (openssl, the key's path on its argv, never its bytes) and trades
# it for an installation token restricted to this repository. Neither is ever printed or put on an
# argv or in the environment: curl reads its whole request, header included, as a config on stdin
# (-q: no ~/.curlrc); git reads the token through this script's credential mode, from a 0600 file in a
# private temp dir that the first `get` for https://github.com deletes, and that no other host gets.
# Git reads no global or system config here (GIT_CONFIG_GLOBAL=/dev/null, GIT_CONFIG_NOSYSTEM=1): no
# url.insteadOf, pushInsteadOf or push.followTags of the user's reroutes or widens a push.
# GIT_TERMINAL_PROMPT=0: a missing credential fails, it never prompts. The run's temp dir, which holds
# the token while it runs, lies under the key directory ($kdir/run/router-gh.XXXXXX), which workers
# cannot read; never in $TMPDIR, which they share (security review round 2, X2). It dies with the run.
#
# No pushed code runs here (security review, B1). The router can read the key, reach GitHub and write
# its checkout and the shared git dir; the pre-push hook's keel check would run the pushed branch's own
# [check] commands, build scripts and tests with those rights. So push runs with --no-verify and does
# only Keel's code-free checks itself: a wu/ tip must carry its test/v1 note (its worker ran keel check
# in its sandbox), and an epic/ push that GitHub already holds must pass the range walk (keel verify-dod
# range, git data only), as the hook would. The tests run in the worker's sandbox and in CI. The notes
# ride the same push: only Keel's kinds (test, doc, cr, mr, dod, report), after GitHub's sign-offs are
# taken in, add-only (see push). Git runs only in this checkout, never in another worktree (round 2, X1): sync fetches GitHub's branch into refs/remotes/origin/<b>
# and moves the local branch by update-ref when no worktree has it checked out; else that worktree's
# own session fast-forwards it (`git merge --ff-only origin/<b>`).
# These checks are policy, not a bound (round 2, N2, N4): workers can write the shared git dir (refs,
# notes, objects, replace refs, shallow), so a note or a branch read here may be theirs. A wu/ push and
# a first epic/ push get no range walk. CI's gate and the owner's sign-off are the authority; what no
# worker reaches is the key and the token. GIT_NO_REPLACE_OBJECTS=1: no replace ref steers a read.
# Which worktree holds a branch is policy too (round 3, I1). An entry is believed when it looks as git
# makes one, by scripts/router-worker.sh's rule: the first row's .git is our git dir (the main
# checkout), any other row's .git names $gd/worktrees/<id>, whose commondir is ../.. and whose gitdir
# names it back. That catches a rewritten link, not an entry a worker forges whole (it can write both
# halves: $gd/worktrees/<new id>/ and a .git file in a directory it owns). Such a forge can make sync
# report `fetched` for the worker's path and leave the epic unmoved, and make post read, post and
# delete <forged dir>/.router-gh/<name>.md, a comment it could also have written; the text is still
# made plain and inert. It runs no code here and reaches neither the key nor the token.
#
# Text. A pull request's title and body are built here from the branch, its tip, the docs the range
# changed (title, role, status, a link at the tip) and the notes on the tip; doc text is reduced to
# plain words first. Free text reaches GitHub only through post, from a file the head branch's worker
# wrote in its worktree (UC-ROUTE-3), at most 60000 bytes of UTF-8 without control characters but tab
# and newline, with @-mentions made inert; it is deleted once posted, so a stale text is never resent.
# The worker owns that directory, so the file is read once (security review, B2): from inside the
# directory, entered by its real path, opened without following a link (O_NOFOLLOW), and only a
# regular file with one link is copied to the temp dir; every check and the post read the copy.
#
# Seams for tests, never set by the router (its guard admits no environment prefix): ROUTER_GH_CURL
# (the curl to run), ROUTER_GH_API (https://api.github.com, or an https loopback URL), ROUTER_GH_GIT
# (https://github.com, or a file:/// directory holding <owner>/<name>.git). The repository is
# [github] repo in .keel/config.toml at HEAD of this checkout.
# One diagnostic, the owner's only (E-GHFLOW2.W1; the router cannot set it, its guard admits no
# environment prefix): ROUTER_GH_DIAG=mint makes the token request once (diag_mint), prints safe
# facts only on stderr (the status, the response's header names and top-level keys, the token's type,
# length, first four characters and shape) and exits 0 before any push or other request; never the
# token, the JWT, the key or a header's value.
set -euf
# byte-wise, whatever the caller's locale: [A-Za-z] means ASCII letters in every check below
LC_ALL=C; export LC_ALL
for v in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_NO_REPLACE_OBJECTS=1
export GIT_CONFIG_NOSYSTEM GIT_CONFIG_GLOBAL GIT_NO_REPLACE_OBJECTS
umask 077
die() { echo "router-gh: $*" >&2; exit 2; }
# Keel's notes kinds, the only ones pushed: the five, and the epic report (E-GHFLOW2.W3)
NOTE_KINDS="test doc cr mr dod report"
fail() { echo "router-gh: $*" >&2; exit 1; }
# an argument as printed in an error: no control character, at most 80 bytes
shown() { printf '%s' "$1" | tr -d '\000-\037\177' | cut -c1-80; }
# token_ok <t>: an installation token as GitHub mints it, opaque (E-GHFLOW2.W1): the legacy ghs_ + 36
# characters, or since 2026-04-27 the stateless ghs_<app id>_<JWT> of about 520, with . and -. Taken
# as 1 to 4096 bytes of printable ASCII with no space, quote or backslash: the set that stays inert
# in the curl config's quoted header line, git's password= line and the token file.
token_ok() {
  case "$1" in ''|*[!!-~]*|*\"*|*\'*|*\\*) return 1;; esac
  [ ${#1} -le 4096 ]
}
usage="usage: push <branch> | pr-open <branch> [--base <main|epic/E>] | pr-ready <n> | post <n> <body-name> | sync <epic/E|main|wu/E/slug> | issue <branch>"
# Keel's branch policy (.keel/config.toml [branches], as scripts/router-worker.sh holds it)
main_b=main epic_p=epic/ wu_p=wu/
seg='[A-Za-z0-9][A-Za-z0-9._-]*'
# a word of a task's name: no _, the task separator (as src/notes.rs task_of reads it)
word='[A-Za-z0-9][A-Za-z0-9.-]*'
# HOME, normalised: absolute, no trailing /, and only characters a curl config and a case pattern
# take as they are (the run dir lies under it)
home_ok() {
  : "${HOME:?router-gh: HOME is not set}"
  while [ "$HOME" != / ] && [ "${HOME%/}" != "$HOME" ]; do HOME=${HOME%/}; done
  case "$HOME" in /?*) ;; *) die "HOME must be an absolute directory other than /: $(shown "$HOME")";; esac
  case "$HOME" in *[!A-Za-z0-9/._-]*|*//*|*/./*|*/../*|*/.|*/..) die "HOME must hold only letters, digits and / . _ - (no // . or .. part): $(shown "$HOME")";; esac
}
nodash() { case "$1" in -*) die "an argument may not start with -: $(shown "$1")";; esac; }
refname() {
  case "$1" in *..*) die "not a valid branch name: $(shown "$1")";; esac
  git check-ref-format "refs/heads/$1" || die "not a valid branch name: $(shown "$1")"
}
# work_branch <b>: wu/<E>/<slug> or epic/<E>; sets base to the branch it merges into, and task to
# yes for a task wu/<E>/<wu>_<task>, whose base is its WU branch wu/<E>/<wu>
work_branch() {
  nodash "$1"; task=no
  if expr "x$1" : "x$epic_p$seg\$" > /dev/null; then base=$main_b
  elif expr "x$1" : "x$wu_p$word/${word}_$word\$" > /dev/null; then t=${1#"$wu_p"}; base=$wu_p${t%%_*}; task=yes
  elif expr "x$1" : "x$wu_p$seg/$seg\$" > /dev/null; then e=${1#"$wu_p"}; base=$epic_p${e%%/*}
  else die "branch must be $epic_p<E> or $wu_p<E>/<slug> in plain words: $(shown "$1")"; fi
  refname "$1"
}
number() { nodash "$1"; expr "x$1" : 'x[1-9][0-9]\{0,6\}$' > /dev/null || die "pull request must be a number: $(shown "$1")"; }

[ $# -ge 1 ] || die "$usage"
mode=$1; shift
case "$mode" in
  push)
    [ $# -eq 1 ] || die "usage: push <branch>"
    branch=$1; work_branch "$branch";;
  pr-open)
    [ $# -eq 1 ] || [ $# -eq 3 ] || die "usage: pr-open <branch> [--base <main|epic/E>]"
    branch=$1; work_branch "$branch"
    if [ $# -eq 3 ]; then
      [ "$2" = --base ] || die "usage: pr-open <branch> [--base <main|epic/E>]"
      nodash "$3"
      [ "$3" = "$main_b" ] || expr "x$3" : "x$epic_p$seg\$" > /dev/null || die "base must be $main_b or $epic_p<E>: $(shown "$3")"
      refname "$3"
      [ "$3" != "$branch" ] || die "a branch cannot be its own base: $branch"
      # a WU merges into its epic only, never into main (E-GHFLOW2.W4 review) nor another epic (W4
      # review, nit 5; E-GHFLOW2.W6): its base is derived from its name, a task's too
      case "$branch" in "$wu_p"*) die "$branch goes into $base, derived from its name: drop --base";; esac
      base=$3
    fi;;
  pr-ready)
    [ $# -eq 1 ] || die "usage: pr-ready <n>"
    n=$1; number "$n";;
  post)
    [ $# -eq 2 ] || die "usage: post <n> <body-name>"
    n=$1 name=$2; number "$n"; nodash "$name"
    expr "x$name" : 'x[A-Za-z0-9][A-Za-z0-9_-]\{0,63\}$' > /dev/null || die "body name must be plain words (letters, digits, _ and -): $(shown "$name")";;
  sync)
    [ $# -eq 1 ] || die "usage: sync <epic/E|main|wu/E/slug>"
    branch=$1; nodash "$branch"
    # a WU branch too (E-GHFLOW2.W6): the owner merges a task into it on GitHub
    [ "$branch" = "$main_b" ] || expr "x$branch" : "x$epic_p$seg\$" > /dev/null || expr "x$branch" : "x$wu_p$seg/$seg\$" > /dev/null \
      || die "sync takes $main_b, an $epic_p<E> or a $wu_p<E>/<slug> branch only: $(shown "$branch")"
    refname "$branch"
    case "$branch" in "$wu_p"*) work_branch "$branch"; [ "$task" = no ] || die "a task is not synced: sync its WU branch, $base";; esac;;
  issue)
    [ $# -eq 1 ] || die "usage: issue <wu/E/slug|epic/E>"
    branch=$1; work_branch "$branch"
    [ "$task" = no ] || die "a task has no issue of its own: the sub-issue of its WU, $base, is the record";;
  watch-read)
    # scripts/router-gh-watch.sh's read, one per poll; read-only, never a router shape (the guard refuses it)
    [ $# -eq 2 ] || die "usage: watch-read <review|comment|merged|checks|conflict> <n>"
    wkind=$1 n=$2
    case "$wkind" in review|comment|merged|checks|conflict) ;; *) die "usage: watch-read <review|comment|merged|checks|conflict> <n>";; esac
    number "$n";;
  credential)
    # git's credential helper (`!'<this script>' credential '<file>'` + the op): get, asked for
    # protocol=https host=github.com, prints the token once and the file is gone; any other host gets
    # nothing; store and erase do nothing. Only a token file this script made.
    [ $# -eq 2 ] || die "usage: credential <file> <op>"
    f=$1
    home_ok
    case "$f" in "$HOME"/.config/keel/router-gh/run/router-gh.[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]/token) ;; *) die "credential: not a router-gh token file";; esac
    case "$f" in *..*|*/./*|*//*) die "credential: not a router-gh token file";; esac
    [ "$2" = get ] || exit 0
    # only for GitHub over https: a rerouted URL's host never gets the token
    proto= host=
    while IFS= read -r l; do
      [ -n "$l" ] || break
      case "$l" in protocol=*) proto=${l#protocol=};; host=*) host=${l#host=};; esac
    done
    [ "$proto" = https ] && [ "$host" = github.com ] || exit 0
    [ -f "$f" ] && [ ! -L "$f" ] || exit 0
    # one reader only: the rename succeeds once
    mv "$f" "$f.used" 2> /dev/null || exit 0
    t=$(cat "$f.used"); rm -f "$f.used"
    token_ok "$t" || exit 0
    printf 'username=x-access-token\npassword=%s\n' "$t"
    exit 0;;
  *) die "$usage";;
esac

# --- setup: the key, the App's ids, the repository; nothing has touched the network yet ---
SELF=$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
home_ok
kdir=$HOME/.config/keel/router-gh
key=$kdir/app.pem conf=$kdir/app.conf
[ -e "$key" ] || [ -L "$key" ] || die "no App key at $key (the owner's App setup puts it there)"
[ -f "$key" ] && [ ! -L "$key" ] || die "the App key $key must be a regular file, not a link"
set -- $(ls -lnd "$kdir")
case "$1" in d????[!w]??[!w]*) ;; *) die "the App key's directory $kdir must be a directory no one else may write (chmod 700 $kdir)";; esac
[ "$3" = "$(id -u)" ] || die "the App key's directory $kdir must be owned by you"
set -- $(ls -ln "$key")
case "$1" in -rw-------|-rw-------[@.]|-r--------|-r--------[@.]) ;; *) die "the App key $key must have mode 0600 or 0400 (chmod 600 $key)";; esac
[ "$3" = "$(id -u)" ] || die "the App key $key must be owned by you"
# one link: a hard link elsewhere (a worker's worktree) would be a second name for the key
[ "$2" = 1 ] || die "the App key $key has $2 links: remove the others"
kreal=$(cd "$kdir" && pwd -P) || die "cannot resolve $kdir"
# This checkout's own git dir, pinned for every git below (security review P1, backlog R33): never
# one a commondir file names, since workers can write the shared git dir. A main checkout's .git has
# no commondir file; a linked checkout's admin dir holds ../.. and names this checkout back.
# GIT_DIR and GIT_COMMON_DIR then make any commondir moot, and every git here runs with no hooks and
# no fsmonitor (GIT_CONFIG_COUNT) and speaks only https to GitHub (GIT_ALLOW_PROTOCOL, below), over
# whatever the repository's config says.
if [ -d "$ROOT/.git" ] && [ ! -L "$ROOT/.git" ]; then
  gdir=$(cd -P "$ROOT/.git" && pwd -P) || die "cannot resolve $ROOT/.git"
  gd=$gdir
elif [ -f "$ROOT/.git" ] && [ ! -L "$ROOT/.git" ]; then
  gl=$(cat "$ROOT/.git") || die "cannot read $ROOT/.git"
  ga=${gl#gitdir: }
  [ "$ga" != "$gl" ] || die "$ROOT/.git is not a gitdir link"
  gdir=$(cd -P "$ga" 2> /dev/null && pwd -P) || die "cannot resolve $ROOT/.git's admin dir"
  vg=$(cat "$gdir/gitdir" 2> /dev/null) || vg=
  [ "$(basename "$(dirname "$gdir")")" = worktrees ] && [ "$(cat "$gdir/commondir" 2> /dev/null)" = ../.. ] \
    && [ "$(basename "$vg")" = .git ] && [ "$(cd -P "$(dirname "$vg")" 2> /dev/null && pwd -P)" = "$ROOT" ] \
    || die "$gdir is not this checkout's admin dir as git makes one (commondir ../.., gitdir naming $ROOT/.git): the owner decides"
  gd=$(cd -P "$gdir/../.." && pwd -P) || die "cannot resolve this repository's git dir"
else
  die "$ROOT has no .git"
fi
[ ! -e "$gd/commondir" ] && [ ! -L "$gd/commondir" ] || die "$gd/commondir exists: a main git dir has none, so a worker may have written it; the owner removes it"
# nor a config.worktree, in the git dir or this checkout's admin dir (round 2): git reads one even
# under the pins, and a url.insteadOf or core.askPass there reroutes git or runs code
for f in "$gd/config.worktree" "$gdir/config.worktree"; do
  [ ! -e "$f" ] && [ ! -L "$f" ] || die "$f exists: this repository uses none, so a worker may have written it; the owner removes it"
done
GIT_DIR=$gdir GIT_COMMON_DIR=$gd GIT_WORK_TREE=$ROOT
GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/dev/null GIT_CONFIG_KEY_1=core.fsmonitor GIT_CONFIG_VALUE_1=false
export GIT_DIR GIT_COMMON_DIR GIT_WORK_TREE GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0 GIT_CONFIG_KEY_1 GIT_CONFIG_VALUE_1
# valid_wt <path> <row>: prints the resolved path when the entry looks as git makes one (see the
# header): row 1, the main checkout, whose .git is our git dir; any later row, a linked worktree whose
# .git names $gd/worktrees/<id>, whose commondir is ../.. and whose gitdir names <path>/.git back
valid_wt() {
  vp=$(cd -P "$1" 2> /dev/null && pwd -P) || return 1
  if [ "$2" = 1 ]; then
    [ -d "$vp/.git" ] && [ ! -L "$vp/.git" ] || return 1
    [ "$(cd -P "$vp/.git" 2> /dev/null && pwd -P)" = "$gd" ] || return 1
  else
    [ -f "$vp/.git" ] && [ ! -L "$vp/.git" ] || return 1
    vl=$(cat "$vp/.git") || return 1
    vn=${vl#"gitdir: $gd/worktrees/"}
    [ "$vn" != "$vl" ] && [ "$vn" != . ] && [ "$vn" != .. ] && expr "x$vn" : 'x[A-Za-z0-9._-]\{1,\}$' > /dev/null || return 1
    va=$gd/worktrees/$vn
    [ -d "$va" ] && [ ! -L "$va" ] && [ "$(cat "$va/commondir" 2> /dev/null)" = ../.. ] || return 1
    vg=$(cat "$va/gitdir" 2> /dev/null) && [ "$(basename "$vg")" = .git ] || return 1
    [ "$(cd -P "$(dirname "$vg")" 2> /dev/null && pwd -P)" = "$vp" ] || return 1
  fi
  printf '%s\n' "$vp"
}
# worktree_rows: "<row><TAB><path><TAB><branch ref or ->" per entry git lists (believed or not)
worktree_rows() { git -C "$ROOT" worktree list --porcelain | awk '
  /^worktree / { if (w != "") print n "\t" w "\t" (b == "" ? "-" : b); n++; w = substr($0, 10); b = "" }
  /^branch / { b = substr($0, 8) }
  END { if (w != "") print n "\t" w "\t" (b == "" ? "-" : b) }'; }
# never inside a worktree of this repository or its git dir, where a worker could reach it
inside=$(worktree_rows | while IFS='	' read -r wn w _; do
  wr=$(valid_wt "$w" "$wn") || continue
  case "$kreal/" in ("$wr"/*) printf '%s\n' "$wr"; break;; esac
done || :)
[ -z "$inside" ] || die "the App key's directory $kdir lies inside $inside, which workers can reach: move it out"
case "$kreal/" in "$gd"/*) die "the App key's directory $kdir lies inside the git dir $gd, which workers can reach: move it out";; esac
[ -f "$conf" ] && [ ! -L "$conf" ] || die "no App settings at $conf (app_id=, installation_id=, app_slug= lines)"
app_id= inst= slug=
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in ''|'#'*) continue;; esac
  k=${line%%=*} v=${line#*=}
  case "$k" in
    app_id) [ -z "$app_id" ] && expr "x$v" : 'x[1-9][0-9]\{0,11\}$' > /dev/null || die "bad app_id line in $conf"; app_id=$v;;
    installation_id) [ -z "$inst" ] && expr "x$v" : 'x[1-9][0-9]\{0,11\}$' > /dev/null || die "bad installation_id line in $conf"; inst=$v;;
    app_slug) [ -z "$slug" ] && expr "x$v" : 'x[a-z0-9][a-z0-9-]\{0,62\}$' > /dev/null || die "bad app_slug line in $conf"; slug=$v;;
    *) die "unknown line in $conf (only app_id=, installation_id=, app_slug=)";;
  esac
done < "$conf"
[ -n "$app_id" ] && [ -n "$inst" ] && [ -n "$slug" ] || die "$conf must set app_id, installation_id and app_slug"
bot="$slug[bot]"
# [github] repo and [docs] root, from the committed policy at this checkout's HEAD
cfg=$(git -C "$ROOT" show HEAD:.keel/config.toml 2> /dev/null) || die "no .keel/config.toml at HEAD of $ROOT"
toml() { printf '%s\n' "$cfg" | awk -v s="[$1]" -v k="$2" '
  /^[ \t]*\[/ { t = $0; sub(/[ \t]*#.*$/, "", t); on = (t == s); next }
  on { l = $0; sub(/[ \t]*#.*$/, "", l); if (l ~ "^" k " = \"[^\"]*\"[ \t]*$") { sub(/^[^"]*"/, "", l); sub(/"[ \t]*$/, "", l); print l; n++ } }
  END { if (n != 1) exit 1 }'; }
repo=$(toml github repo) || die "no [github] repo = \"<owner>/<name>\" in .keel/config.toml at HEAD of $ROOT"
expr "x$repo" : 'x[A-Za-z0-9][A-Za-z0-9-]\{0,38\}/[A-Za-z0-9_.-]\{1,100\}$' > /dev/null || die "[github] repo must be <owner>/<name>: $(shown "$repo")"
case "$repo" in */.*|*..*) die "[github] repo must be <owner>/<name>: $(shown "$repo")";; esac
owner=${repo%%/*} rname=${repo#*/}
# the repository's Pages site (E-GHFLOW2.W8): GitHub serves a project site at the owner's host, in lower case
pages=https://$(printf '%s' "$owner" | tr 'A-Z' 'a-z').github.io/$rname
docs=$(toml docs root) || docs=docs/
expr "x$docs" : 'x[A-Za-z0-9_][A-Za-z0-9_./-]*/$' > /dev/null || die "[docs] root must be a relative directory ending in /"
# gh_policy: [github] commanders (space-separated ids, maybe none) and free_text (true|false, default
# true) from this checkout's .keel/config.toml file, refused unless its bytes are HEAD's (see the header)
gh_policy() {
  pf=$ROOT/.keel/config.toml
  [ -f "$pf" ] && [ ! -L "$pf" ] || die "no .keel/config.toml file in $ROOT"
  hb=$(git -C "$ROOT" rev-parse -q --verify "HEAD:.keel/config.toml") || die "no .keel/config.toml at HEAD of $ROOT"
  wb=$(git -C "$ROOT" hash-object --no-filters -- "$pf") || die "cannot hash $pf"
  [ "$hb" = "$wb" ] || die "$pf differs from HEAD's: who the router hears is read only from a committed file"
  pol=$(awk '
    /^[ \t]*\[/ { t = $0; sub(/[ \t]*#.*$/, "", t); on = (t == "[github]"); next }
    on { l = $0; sub(/[ \t]*#.*$/, "", l); sub(/[ \t]+$/, "", l)
      if (l ~ /^commanders[ \t]*=/) { c++; sub(/^commanders[ \t]*=[ \t]*/, "", l); print "c" l }
      else if (l ~ /^free_text[ \t]*=/) { f++; sub(/^free_text[ \t]*=[ \t]*/, "", l); print "f" l }
      else if (l ~ /^merge_on_approve[ \t]*=/) { m++; sub(/^merge_on_approve[ \t]*=[ \t]*/, "", l); print "m" l } }
    END { if (c > 1 || f > 1 || m > 1) exit 1 }' "$pf") || die "[github] in $pf sets commanders, free_text or merge_on_approve twice"
  commanders= free_text=true merge_on_approve=true
  while IFS= read -r pl; do
    case "$pl" in
      c*) expr "x${pl#c}" : 'x\(\[ *[1-9][0-9]\{0,11\}\( *, *[1-9][0-9]\{0,11\}\)* *\]\)$' > /dev/null \
            || die "[github] commanders must be a list of GitHub user ids, as commanders = [5499546]"
          commanders=$(printf '%s' "${pl#c}" | tr -c '0-9' ' ' | tr -s ' ' | sed 's/^ //; s/ $//');;
      f*) case "${pl#f}" in true|false) free_text=${pl#f};; *) die "[github] free_text must be true or false";; esac;;
      m*) case "${pl#m}" in true|false) merge_on_approve=${pl#m};; *) die "[github] merge_on_approve must be true or false";; esac;;
    esac
  done <<EOF
$pol
EOF
}
API=${ROUTER_GH_API:-https://api.github.com}
case "$API" in
  https://api.github.com) ;;
  https://127.0.0.1:*|https://localhost:*) expr "x${API##*:}" : 'x[1-9][0-9]\{0,4\}$' > /dev/null || die "ROUTER_GH_API: bad port";;
  *) die "ROUTER_GH_API must be https://api.github.com or an https loopback URL";;
esac
GITBASE=${ROUTER_GH_GIT:-https://github.com}
case "$GITBASE" in https://github.com|file:///*) ;; *) die "ROUTER_GH_GIT must be https://github.com or a file:/// directory";; esac
case "$GITBASE" in *[!A-Za-z0-9/:._-]*|*..*) die "ROUTER_GH_GIT: plain path characters only";; esac
# only the transport the URL needs, over any protocol.*.allow or url.*.insteadOf in the repository's
# config (git's GIT_ALLOW_PROTOCOL overrides both): https to GitHub, file only for the test seam
case "$GITBASE" in https:*) GIT_ALLOW_PROTOCOL=https;; *) GIT_ALLOW_PROTOCOL=file;; esac
export GIT_ALLOW_PROTOCOL
url=$GITBASE/$repo.git
CURL=${ROUTER_GH_CURL:-curl}
# the run dir: under the key dir, which workers cannot read (X2), never the shared $TMPDIR
run=$kdir/run
[ -d "$run" ] || [ -L "$run" ] || mkdir "$run" || die "cannot make $run"
[ -d "$run" ] && [ ! -L "$run" ] || die "$run must be a directory, not a link"
tmp=
# GitHub's refs/notes/mr while push reads it: a ref of this run's own (F7), gone with the run; and
# refs/notes/mr as this checkout last pushed it (push's snapshot, kept between runs)
ghtmp=refs/notes/keel-gh/mr-$$ ghsnap=refs/notes/keel-gh/pushed-mr
trap '[ -z "$tmp" ] || rm -rf "$tmp"; git -C "$ROOT" update-ref -d "$ghtmp" 2> /dev/null || :' EXIT
trap 'exit 1' HUP INT TERM
tmp=$(mktemp -d "$run/router-gh.XXXXXX") || die "cannot make a temp dir in $run"
case "$tmp" in *[!A-Za-z0-9/._-]*) die "the temp dir $tmp has characters a curl config cannot hold";; esac

local_tip() { git -C "$ROOT" rev-parse --verify -q "refs/heads/$1^{commit}"; }
# the registered worktree that has branch $1 checked out, if any
checked_out_at() {
  worktree_rows | while IFS='	' read -r cn cp cb; do
    [ "$cb" = "refs/heads/$1" ] || continue
    valid_wt "$cp" "$cn" && break
  done || :
}
sq() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }

# --- GitHub ---
# api <method> <path> [<json file>] [jwt]: the response body lands in $tmp/resp, its status in $code.
# The request, Authorization header included, reaches curl only on its stdin.
api() {
  am=$1 ap=$2 ab=${3:-} aa=${4:-}
  if [ "$aa" = jwt ]; then ah=$jwt; else ah=$tok; fi
  rm -f "$tmp/resp" "$tmp/hdr"
  rc=0
  code=$({
    printf 'url = "%s%s"\nrequest = "%s"\n' "$API" "$ap" "$am"
    printf 'header = "Authorization: Bearer %s"\n' "$ah"
    printf 'header = "Accept: application/vnd.github+json"\nheader = "X-GitHub-Api-Version: 2022-11-28"\nheader = "User-Agent: keel-router-gh"\n'
    [ -z "$ab" ] || printf 'header = "Content-Type: application/json"\ndata-binary = "@%s"\n' "$ab"
    printf 'output = "%s"\ndump-header = "%s"\nwrite-out = "%%{http_code}"\nsilent\nshow-error\nproto = "=https"\nconnect-timeout = 15\nmax-time = 60\n' "$tmp/resp" "$tmp/hdr"
  } | "$CURL" -q --config -) || rc=$?
  [ "$rc" -eq 0 ] || fail "GitHub unreachable: $am $ap (curl exit $rc)"
  expr "x$code" : 'x[1-5][0-9][0-9]$' > /dev/null || fail "GitHub gave no status: $am $ap"
  [ -f "$tmp/resp" ] || : > "$tmp/resp"
}
# api_ok: as api, and anything but a 2xx is exit 1 with GitHub's message
api_ok() {
  api "$@"
  case "$code" in 2??) return 0;; esac
  msg=$(jq -r '.message // empty' "$tmp/resp" 2> /dev/null | tr -d '\000-\037\177' | cut -c1-200) || msg=
  fail "GitHub refused $1 $2: HTTP $code${msg:+: $msg}"
}
# pages <path> <query prefix> <jq path to the page's list>: every page of a list, 100 a page, at most
# 10 (more is refused), joined into $tmp/all
pages() {
  pg=1
  printf '[]' > "$tmp/all"
  while :; do
    api_ok GET "$1?$2per_page=100&page=$pg"
    jq -c "$3 | arrays" "$tmp/resp" > "$tmp/page" 2> /dev/null && [ -s "$tmp/page" ] || fail "GitHub's answer for $1 is not a list"
    jq -c -s '.[0] + .[1]' "$tmp/all" "$tmp/page" > "$tmp/all.new" && mv "$tmp/all.new" "$tmp/all" || fail "cannot join GitHub's pages for $1"
    [ "$(jq length "$tmp/page")" -ge 100 ] || break
    pg=$((pg + 1))
    [ "$pg" -le 10 ] || die "more than 1000 items at $1: refused"
  done
}
# pages_newest <path>: the newest pages of a list GitHub orders oldest first, at most 10 of 100,
# found by the Link header's rel="last" (security review I1: a thread longer than 1000 items is read
# from its end, never refused). Into $tmp/all; $truncated is true when older pages were skipped.
pages_newest() {
  api_ok GET "$1?per_page=100&page=1"
  jq -c '. | arrays' "$tmp/resp" > "$tmp/page" 2> /dev/null && [ -s "$tmp/page" ] || fail "GitHub's answer for $1 is not a list"
  last=$(grep -i '^link:' "$tmp/hdr" 2> /dev/null | tr ',' '\n' | sed -n 's/.*[?&]page=\([1-9][0-9]\{0,6\}\)[^>]*> *; *rel="last".*/\1/p' | head -n 1) || last=
  if [ -z "$last" ]; then
    # a full first page with no Link to the last one: the newest items cannot be found (round 2, N-c)
    [ "$(jq length "$tmp/page")" -lt 100 ] || fail "GitHub's answer for $1 holds 100 items and no Link to its last page"
    last=1
  fi
  truncated=false first=2
  if [ "$last" -gt 10 ]; then truncated=true first=$((last - 9)); printf '[]' > "$tmp/all"; else cp "$tmp/page" "$tmp/all"; fi
  pg=$first
  while [ "$pg" -le "$last" ]; do
    api_ok GET "$1?per_page=100&page=$pg"
    jq -c '. | arrays' "$tmp/resp" > "$tmp/page" 2> /dev/null && [ -s "$tmp/page" ] || fail "GitHub's answer for $1 is not a list"
    jq -c -s '.[0] + .[1]' "$tmp/all" "$tmp/page" > "$tmp/all.new" && mv "$tmp/all.new" "$tmp/all" || fail "cannot join GitHub's pages for $1"
    pg=$((pg + 1))
  done
}
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
# diag_mint (ROUTER_GH_DIAG=mint, the owner's diagnostic, E-GHFLOW2.W1): the token request once,
# then only facts that are no secret, on stderr, and exit 0 before any other step. It prints the
# status, the response's header names and top-level keys, and the token's type, length, first four
# characters and shape; never the token, the JWT, the key or a header's value.
diag_mint() {
  api POST "/app/installations/$inst/access_tokens" "$tmp/tokreq" jwt
  jwt=
  dt=$(jq -r '.token | strings' "$tmp/resp" 2> /dev/null) || dt=
  {
    echo "DIAG status=$code"
    echo "DIAG status-line=$(head -n 1 "$tmp/hdr" 2> /dev/null | tr -cd 'A-Za-z0-9/. ')"
    echo "DIAG header-names=$(sed -n 's/^\([A-Za-z0-9-]\{1,64\}\):.*/\1/p' "$tmp/hdr" 2> /dev/null | tr '\n' ' ')"
    echo "DIAG body-bytes=$(wc -c < "$tmp/resp" | tr -d ' ')"
    echo "DIAG keys=$(jq -r 'if type == "object" then keys_unsorted | join(" ") else "(a JSON " + type + ")" end' "$tmp/resp" 2> /dev/null || echo '(not JSON)')"
    echo "DIAG token-type=$(jq -r '.token | type' "$tmp/resp" 2> /dev/null || echo '(not JSON)')"
    echo "DIAG token-length=${#dt}"
    echo "DIAG token-first4=$(printf '%s' "$dt" | cut -c1-4 | tr -cd 'A-Za-z0-9_')"
    echo "DIAG token-dots=$(printf '%s' "$dt" | tr -cd . | wc -c | tr -d ' ') token-dashes=$(printf '%s' "$dt" | tr -cd - | wc -c | tr -d ' ') token-other-bytes=$(printf '%s' "$dt" | tr -d 'A-Za-z0-9_.-' | wc -c | tr -d ' ')"
    case "$dt" in "ghs_${app_id}_"?*) echo "DIAG token-shape=ghs_<app_id>_<more>";; ghs_*) echo "DIAG token-shape=ghs_<other>";; '') echo "DIAG token-shape=empty";; *) echo "DIAG token-shape=other";; esac
    case "$code" in 2??) ;; *) echo "DIAG message=$(jq -r '.message // empty' "$tmp/resp" 2> /dev/null | tr -d '\000-\037\177' | cut -c1-200)";; esac
  } >&2
  dt=
  rm -f "$tmp/resp" "$tmp/hdr"
  exit 0
}
# the installation token, restricted to this repository; the JWT that bought it lives 10 minutes
mint() {
  now=$(date +%s)
  h=$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)
  p=$(printf '{"iat":%s,"exp":%s,"iss":"%s"}' "$((now - 60))" "$((now + 540))" "$app_id" | b64url)
  s=$(printf '%s.%s' "$h" "$p" | openssl dgst -sha256 -sign "$key" -binary 2> /dev/null | b64url) || s=
  [ ${#s} -ge 300 ] || die "cannot sign with the App key $key (an RSA private key in PEM form)"
  jwt=$h.$p.$s tok=
  jq -n --arg r "$rname" '{repositories: [$r]}' > "$tmp/tokreq"
  [ "${ROUTER_GH_DIAG:-}" != mint ] || diag_mint
  api_ok POST "/app/installations/$inst/access_tokens" "$tmp/tokreq" jwt
  tok=$(jq -r '.token | strings' "$tmp/resp" 2> /dev/null) || tok=
  rm -f "$tmp/resp"
  jwt=
  # the error names safe facts only: the status and the token's length, never a byte of it
  token_ok "$tok" || fail "GitHub returned no usable installation token (HTTP $code, a token of ${#tok} bytes)"
}
# git to GitHub: over https the token comes from the one-shot credential helper, never a prompt
gh_git() {
  if [ "${GITBASE#https:}" != "$GITBASE" ]; then
    printf '%s' "$tok" > "$tmp/token"
    grc=0; GIT_TERMINAL_PROMPT=0 git -C "$ROOT" -c maintenance.auto=false -c credential.helper= -c "credential.helper=!$(sq "$SELF") credential $(sq "$tmp/token")" "$@" || grc=$?
    rm -f "$tmp/token"
    return "$grc"
  else
    GIT_TERMINAL_PROMPT=0 git -C "$ROOT" -c maintenance.auto=false "$@"
  fi
}
# bot_pr <n>: the pull request must be open, the bot's, from and into this repository, head wu/ or
# epic/, base main or epic/ (a task's: its WU branch); sets head_ref, head_sha, pr_base, draft, node, task
# automerge: with [github] merge_on_approve, turn on GitHub's auto-merge for pull request $node as a
# merge commit (E-GHFLOW2.W4). The pull request is ready already, so a refusal is a WARN line, never a
# failure. Sets automerge_state: on, off (the policy says no), task (never for a task) or refused.
automerge() {
  automerge_state=off
  # a task's pull request is the owner's to merge (E-GHFLOW2.W6): no ruleset covers its WU branch,
  # so GitHub would merge it at once or refuse
  if [ "$task" = yes ]; then automerge_state=task; return 0; fi
  [ "$merge_on_approve" = true ] || return 0
  jq -n --arg id "$node" '{query: "mutation($id: ID!) { enablePullRequestAutoMerge(input: {pullRequestId: $id, mergeMethod: MERGE}) { pullRequest { autoMergeRequest { mergeMethod } } } }", variables: {id: $id}}' > "$tmp/req"
  api POST /graphql "$tmp/req"
  case "$code" in 2??) ;; *) echo "WARN auto-merge not turned on: HTTP $code"; automerge_state=refused; return 0;; esac
  if jq -e '(.errors | not) and .data.enablePullRequestAutoMerge.pullRequest.autoMergeRequest.mergeMethod == "MERGE"' "$tmp/resp" > /dev/null 2>&1; then
    automerge_state=on
  else
    msg=$(jq -r '.errors[0].message // empty' "$tmp/resp" 2> /dev/null | tr -d '\000-\037\177' | cut -c1-200) || msg=
    echo "WARN auto-merge not turned on${msg:+: $msg}"
    automerge_state=refused
  fi
}
bot_pr() {
  api_ok GET "/repos/$repo/pulls/$1"
  facts=$(jq -r --arg bot "$bot" --arg repo "$repo" '
    if (.user.login == $bot and .user.type == "Bot" and .head.repo.full_name == $repo and .base.repo.full_name == $repo)
    then [.state, .head.ref, .head.sha, .base.ref, (.draft | tostring), .node_id] | map(tostring) | join(" ")
    else "foreign" end' "$tmp/resp" 2> /dev/null) || fail "GitHub's answer for pull request $1 is not readable"
  [ "$facts" != foreign ] || die "pull request $1 is not one the bot opened from $repo: refused"
  set -- $facts
  [ $# -eq 6 ] || fail "GitHub's answer for pull request $n is not readable"
  [ "$1" = open ] || die "pull request $n is $(shown "$1"), not open"
  head_ref=$2 head_sha=$3 pr_base=$4 draft=$5 node=$6
  work_branch "$head_ref"
  # a WU's or a task's base is the one its name gives (W6 security review); an epic's, main or an epic
  case "$head_ref" in
    "$wu_p"*) [ "$pr_base" = "$base" ] || die "pull request $n merges into $(shown "$pr_base"), not $base, which its head's name gives";;
    *) [ "$pr_base" = "$main_b" ] || expr "x$pr_base" : "x$epic_p$seg\$" > /dev/null || die "pull request $n merges into $(shown "$pr_base"), not $main_b or an epic";;
  esac
  expr "x$head_sha" : 'x[0-9a-f]\{40\}$' > /dev/null || fail "GitHub's head sha for pull request $n is not a sha"
  expr "x$node" : 'x[A-Za-z0-9_=-]\{1,100\}$' > /dev/null || fail "GitHub's node id for pull request $n is not readable"
}
# doc text made plain: no markup, link, mention, table cell or control character survives
plain() { LC_ALL=C tr -d '\000-\037\177`*_[]<>@#|\\~!$^{}"' | LC_ALL=C cut -c1-200; }
# page_ok <branch> <path>: scripts/pages-build.sh publishes this file, by its --folder and --path
# rules (E-GHFLOW2.W8, security review F4), so no page link is a dead one: main, epic/<E> or
# wu/<E>/<x>; a path under docs/, whatever [docs] root says, as the builder reads; parts in
# [A-Za-z0-9._-], not starting with a dot, at most 200 bytes; at most 400 bytes in all; nothing under
# docs/_report/; a doc's file type. tests/suite/router_gh.rs checks it against the builder's answers.
page_ok() {
  for pg_x in "$1" "$2"; do
    case "$pg_x" in ''|*[!A-Za-z0-9._/-]*|*..*) return 1;; esac
    [ ${#pg_x} -le 400 ] || return 1
  done
  case "$2" in docs/_report|docs/_report/*) return 1;; esac
  case "$2" in *.html|*.css|*.png|*.jpg|*.jpeg|*.gif|*.webp|*.svg|*.drawio) ;; *) return 1;; esac
  pg_part='[A-Za-z0-9_-][A-Za-z0-9._-]{0,199}'
  printf '%s\n' "$1" | grep -Eqx "main|epic/$pg_part|wu/$pg_part/$pg_part" && printf '%s\n' "$2" | grep -Eqx "docs(/$pg_part)+"
}
# data_keys: a data file on stdin (DEC-69) -> four lines, its top-level artifact, status, id and title,
# each empty when absent or not a plain value. artifact as text, [a, b] or a block list of "- a"
# lines; the others as a one-line plain or quoted scalar (a block scalar, an anchor, a flow value is
# empty). The first of a repeated key wins. Every value is still filtered by its caller.
data_keys() {
  LC_ALL=C awk '
    function scalar(v,  q) {
      q = substr(v, 1, 1)
      if (q == "\"" || q == "'\''") return (length(v) >= 2 && substr(v, length(v), 1) == q) ? substr(v, 2, length(v) - 2) : ""
      if (v ~ /^[>|&*!{[%@`]/) return ""
      sub(/[ \t]+#.*$/, "", v)
      return v
    }
    /^[^ \t#-][^:]*:([ \t]|$)/ {
      blk = 0
      k = $0; sub(/:.*/, "", k)
      v = $0; sub(/^[^:]*:[ \t]*/, "", v); sub(/[ \t\r]+$/, "", v)
      if (k in seen) next
      seen[k] = 1
      if (k == "artifact") {
        if (v == "") { blk = 1; next }
        if (v ~ /^\[.*\]$/) { v = substr(v, 2, length(v) - 2); gsub(/,/, " ", v) }
        role = scalar(v)
      } else if (k == "status") status = scalar(v)
      else if (k == "id") id = scalar(v)
      else if (k == "title") title = scalar(v)
      next
    }
    blk && /^[ \t]+-[ \t]/ { x = $0; sub(/^[ \t]+-[ \t]*/, "", x); sub(/[ \t\r]+$/, "", x); role = role " " scalar(x); next }
    /^[^ \t]/ { blk = 0 }
    END { print role; print status; print id; print title }
  '
}
# doc_rows <merge-base> <tip> <plan role>: sets title and rows. rows: a markdown row per doc the
# range changed - a page, or the page of a changed data file (<stem>.yaml beside <stem>.html), each
# once, when the page is at the tip - linked at the tip, and to its page on the Pages site of $branch,
# which scripts/pages-build.sh writes at <site>/<branch>/<path> once the push's build is deployed
# (E-GHFLOW2.W8). Role, status and "id · title" come from the data file at the tip (DEC-69,
# E-DOCSPLIT.W3); a page with none shows a dash for both and its <title>. title: the row text of the
# plan of $3's kind the range committed last (a WU branch with no WU plan takes an epic plan), all
# made plain.
doc_rows() {
  title= rows= dr_seen= dr_plans= dr_epics=
  for f in $(git -C "$ROOT" diff --name-only --diff-filter=AMR "$1" "$2" -- "$docs"); do
    case "$f" in *.html) ;; *.yaml) f=${f%.yaml}.html;; *) continue;; esac
    expr "x$f" : 'x[A-Za-z0-9_][A-Za-z0-9_./-]*$' > /dev/null || continue
    case "$f" in *..*) continue;; esac
    case "
$dr_seen
" in *"
$f
"*) continue;; esac
    dr_seen="$dr_seen
$f"
    git -C "$ROOT" cat-file -e "$2:$f" 2> /dev/null || continue
    role= status= id= dt=
    if git -C "$ROOT" cat-file -e "$2:${f%.html}.yaml" 2> /dev/null; then
      keys=$(git -C "$ROOT" show "$2:${f%.html}.yaml" | head -c 65536 | data_keys)
      role=$(printf '%s\n' "$keys" | sed -n 1p | LC_ALL=C sed -n 's/^ *\([a-z -]*[a-z]\) *$/\1/p' | tr -s ' ' | cut -c1-40)
      status=$(printf '%s\n' "$keys" | sed -n 2p | LC_ALL=C sed -n '/^[a-z-]*$/p' | cut -c1-20)
      id=$(printf '%s\n' "$keys" | sed -n 3p | plain)
      dt=$(printf '%s\n' "$keys" | sed -n 4p | plain)
    fi
    if [ -n "$dt" ]; then
      t=$dt
      [ -z "$id" ] || t="$id · $dt"
    else
      top=$(git -C "$ROOT" show "$2:$f" | head -c 8192 | LC_ALL=C tr '\n\r\t' '   ')
      t=$(printf '%s' "$top" | LC_ALL=C sed -n 's/.*<title>\([^<]*\)<\/title>.*/\1/p' | plain)
    fi
    [ -n "$t" ] || t=${f##*/}
    case " $role " in
      *" $3 "*) dr_plans="$dr_plans
$f	$t";;
      *" epic-plan "*) dr_epics="$dr_epics
$f	$t";;
    esac
    page=—
    page_ok "$branch" "$f" && page="[page]($pages/$branch/$f)"
    rows="$rows| [$t](https://github.com/$repo/blob/$2/$f) | ${role:-—} | ${status:-—} | $page |
"
  done
  # the plan the range committed last: commits newest first (plumbing, so no display config adds a
  # line), the first that touches a plan's page or data file
  [ -n "$dr_plans" ] || dr_plans=$dr_epics
  [ -n "$dr_plans" ] || return 0
  for g in $(git -C "$ROOT" rev-list "$1..$2" | git -C "$ROOT" diff-tree --stdin -r --no-commit-id --name-only -- "$docs"); do
    case "$g" in *.yaml) g=${g%.yaml}.html;; esac
    title=$(printf '%s\n' "$dr_plans" | awk -F '	' -v f="$g" '$1 == f { print $2; exit }')
    [ -z "$title" ] || return 0
  done
  title=$(printf '%s\n' "$dr_plans" | sed -n 2p | cut -f2)
}
# docs_md: the docs table of doc_rows, or a line saying there is none
docs_md() {
  if [ -n "$rows" ]; then
    echo "**Docs this range changed**, at the tip:"
    echo
    echo "| Doc | Role | Status | Page |"
    echo "|---|---|---|---|"
    printf '%s' "$rows"
  else
    echo "No doc changed in this range."
  fi
}
# note_state <kind> <commit>: yes, —, or the cr verdict and round
note_state() {
  if v=$(git -C "$ROOT" notes --ref="$1" show "$2" 2> /dev/null); then
    if [ "$1" = cr ]; then
      verdict=$(printf '%s' "$v" | jq -r '.verdict // empty' 2> /dev/null) || verdict=
      round=$(printf '%s' "$v" | jq -r '.round // empty' 2> /dev/null) || round=
      expr "x$verdict" : 'x[A-Z]\{1,12\}$' > /dev/null || verdict=unreadable
      expr "x$round" : 'x[0-9]\{1,3\}$' > /dev/null && echo "$verdict, round $round" || echo "$verdict"
    else echo yes; fi
  else echo —; fi
}
# notes_row <commit>: the five notes as markdown cells
notes_row() { echo "$(note_state test "$1") | $(note_state doc "$1") | $(note_state cr "$1") | $(note_state mr "$1") | $(note_state dod "$1")"; }

case "$mode" in
push)
  tip=$(local_tip "$branch") || die "no local branch $branch"
  # Keel's code-free checks only; the pre-push hook, which runs the pushed code, is skipped (B1). A
  # task carries no notes: its WU's last commit does (E-GHFLOW2.W6, security review 3a)
  if [ "$base" != "$main_b" ] && [ "$task" = no ]; then
    git -C "$ROOT" notes --ref=test show "$tip" 2> /dev/null | jq -e --arg t "$tip" '.schema == "test/v1" and .tip == $t' > /dev/null 2>&1 \
      || die "the tip $tip of $branch carries no test/v1 note: its worker runs keel check and writes the note first"
  fi
  mint
  if [ "$base" = "$main_b" ]; then
    rsha=$(gh_git ls-remote "$url" "refs/heads/$branch") || fail "cannot read $branch on GitHub"
    rsha=$(printf '%s' "$rsha" | awk 'NR == 1 { print $1 }')
    if [ -n "$rsha" ] && [ "$rsha" != "$tip" ]; then
      expr "x$rsha" : 'x[0-9a-f]\{40\}$' > /dev/null || fail "GitHub's $branch is not a sha"
      git -C "$ROOT" cat-file -e "$rsha^{commit}" 2> /dev/null || die "GitHub's $branch is at $rsha, which this checkout lacks: sync it first"
      command -v keel > /dev/null || die "keel is not on PATH: the range walk needs it"
      (cd "$ROOT" && keel verify-dod range "$rsha" "$branch") >&2 || die "the range walk refused $rsha..$branch (above): nothing pushed"
    fi
  fi
  # GitHub's sign-offs first (E-GHFLOW2.W3, DEC-61's open consequence; security review I2 and round
  # 2 F1, F2, F6). The sign-off writer puts github-tier mr/v1 notes on GitHub's refs/notes/mr; that
  # ref alone is read, add-only, and only what GitHub added since this checkout's last push of it
  # ($ghsnap, the ref as pushed): a note removed here stays removed. Such a note is taken when this
  # checkout holds no mr note on its commit and it reads as one the writer makes (mr/v1, tier github,
  # its tip and its review's commit that commit). Nothing is merged or fast-forwarded, so no edit or
  # deletion made on GitHub reaches a note here, and no other kind is read. A writer-shaped note on a
  # commit this checkout lacks (the owner pressed "Update branch") has its commit fetched by sha
  # first, so the push carries the note on and never erases that sign-off; a commit GitHub does not
  # serve leaves its note untaken, never a hold on every later push (security review round 3,
  # I3-1). A note taken proves no more than a username line until check 8 re-reads the
  # review (backlog github-flow G7): anyone who can write GitHub's notes can make one. Our
  # refs/notes/mr then replaces GitHub's, leased on the ref as fetched.
  lease=
  ghmr=$(gh_git ls-remote "$url" refs/notes/mr) || fail "cannot read GitHub's refs/notes/mr: nothing pushed"
  ghmr=$(printf '%s' "$ghmr" | awk 'NR == 1 { print $1 }')
  if [ -n "$ghmr" ]; then
    expr "x$ghmr" : 'x[0-9a-f]\{40\}$' > /dev/null || fail "GitHub's refs/notes/mr is not a sha"
    gh_git fetch -q --no-tags "$url" "+refs/notes/mr:$ghtmp" || fail "cannot fetch GitHub's refs/notes/mr: nothing pushed"
    lease=$(git -C "$ROOT" rev-parse --verify -q "$ghtmp") || fail "cannot read the fetched refs/notes/mr"
    git -C "$ROOT" notes --ref="$ghtmp" list > "$tmp/ghmr" || fail "cannot list GitHub's mr notes"
    : > "$tmp/snap"
    if git -C "$ROOT" show-ref --verify -q "$ghsnap"; then
      git -C "$ROOT" notes --ref="$ghsnap" list > "$tmp/snap" || fail "cannot list the mr notes last pushed"
    fi
    while read -r nb no; do
      grep -Fqx "$nb $no" "$tmp/snap" && continue
      git -C "$ROOT" notes --ref=mr list "$no" > /dev/null 2>&1 && continue
      if ! git -C "$ROOT" cat-file blob "$nb" | jq -e --arg o "$no" \
        '.schema == "mr/v1" and .tier == "github" and .tip == $o and .review.commit == $o and (.review.id | type) == "number"' > /dev/null 2>&1; then
        echo "WARN GitHub's mr note on $no not taken: not a sign-off by approval"
      elif ! git -C "$ROOT" cat-file -e "$no^{commit}" 2> /dev/null \
        && ! { expr "x$no" : 'x[0-9a-f]\{40\}$' > /dev/null && gh_git fetch -q --no-tags "$url" "$no" 2> /dev/null \
          && git -C "$ROOT" cat-file -e "$no^{commit}" 2> /dev/null; }; then
        echo "WARN GitHub's mr note on $no not taken: GitHub serves no such commit"
      else
        GIT_AUTHOR_NAME=keel-router GIT_AUTHOR_EMAIL=keel-router@localhost GIT_COMMITTER_NAME=keel-router GIT_COMMITTER_EMAIL=keel-router@localhost \
          git -C "$ROOT" notes --ref=mr add -C "$nb" "$no" || fail "cannot take GitHub's mr note on $no"
      fi
    done < "$tmp/ghmr"
    git -C "$ROOT" update-ref -d "$ghtmp"
  fi
  set -- "refs/heads/$branch:refs/heads/$branch"
  # refs/notes/mr is pushed as its value read now, so the snapshot is exactly what was pushed (I3-2)
  m=
  for k in $NOTE_KINDS; do
    git -C "$ROOT" show-ref --verify -q "refs/notes/$k" || continue
    if [ "$k" = mr ]; then
      m=$(git -C "$ROOT" rev-parse --verify -q refs/notes/mr) || fail "cannot read refs/notes/mr"
      set -- "$@" "$m:refs/notes/mr"
    else
      set -- "$@" "refs/notes/$k:refs/notes/$k"
    fi
  done
  rc=0; out=$(gh_git push --porcelain --no-verify --no-follow-tags "--force-with-lease=refs/notes/mr:$lease" "$url" "$@") || rc=$?
  # the snapshot of refs/notes/mr as GitHub now holds it from us (F2)
  if [ -n "$m" ] && printf '%s\n' "$out" | awk -F '\t' '$2 ~ /:refs\/notes\/mr$/ && substr($0, 1, 1) != "!" { f = 1 } END { exit !f }'; then
    git -C "$ROOT" update-ref "$ghsnap" "$m"
  fi
  line=$(printf '%s\n' "$out" | awk -F '\t' -v r="refs/heads/$branch:refs/heads/$branch" '$2 == r { print; exit }')
  [ -n "$line" ] || fail "push of $branch failed before any ref moved (git exit $rc; see above)"
  case "$(printf '%s' "$line" | cut -c1)" in
    '*') state=new;; ' ') state=updated;; '=') state=unchanged;;
    *) why=$(printf '%s' "$line" | awk -F '\t' '{ print $3 }' | tr -d '\000-\037\177' | cut -c1-120)
       fail "push of $branch rejected: $why (never forced: merge GitHub's branch in, or ask the owner)";;
  esac
  bad=$(printf '%s\n' "$out" | awk -F '\t' '$2 ~ /:refs\/notes\// && substr($0, 1, 1) == "!" { print $2 "\t" $3 }')
  printf '%s\n' "$bad" | while IFS='	' read -r ref why; do
    [ -z "$ref" ] || echo "WARN notes ref ${ref#*:} not pushed: $(printf '%s' "$why" | tr -d '\000-\037\177' | cut -c1-120)"
  done
  [ "$rc" -eq 0 ] || [ -n "$bad" ] || fail "git push of $branch exited $rc"
  echo "PUSHED branch=$branch sha=$tip state=$state";;

pr-open)
  tip=$(local_tip "$branch") || die "no local branch $branch"
  local_tip "$base" > /dev/null || die "no local base branch $base"
  case "$branch" in "$epic_p"*) kind_plan=epic-plan;; *) kind_plan=wu-plan;; esac
  mb=$(git -C "$ROOT" merge-base "refs/heads/$base" "$tip") || die "$branch shares no history with $base"
  mint
  api GET "/repos/$repo/git/ref/heads/$branch"
  case "$code" in
    200) rsha=$(jq -r '.object.sha // empty' "$tmp/resp" 2> /dev/null) || rsha=;;
    404) die "push first: $branch is not on GitHub";;
    *) api_ok GET "/repos/$repo/git/ref/heads/$branch";;
  esac
  [ "$rsha" = "$tip" ] || die "push first: GitHub has $branch at $(shown "$rsha"), the local tip is $tip"
  if [ "$task" = yes ]; then
    api GET "/repos/$repo/git/ref/heads/$base"
    case "$code" in 200) ;; 404) die "push first: $branch's WU branch $base is not on GitHub";; *) api_ok GET "/repos/$repo/git/ref/heads/$base";; esac
  fi
  # the docs this range changed, as they are at the tip
  doc_rows "$mb" "$tip" "$kind_plan"
  [ -n "$title" ] || title=$branch
  {
    echo "<!-- keel:router-gh pr-body v1 -->"
    echo "Opened by the Keel router bot for \`$branch\` into \`$base\`."
    if [ "$task" = yes ]; then
      echo
      echo "A task of \`$base\`: no gate runs on it and no review of its own. The owner merges it; the WU is reviewed and signed as a whole."
    fi
    echo
    echo "**Tip:** \`$tip\`"
    echo
    docs_md
    echo
    echo "**Notes on the tip** when the router last opened or refreshed this page; the gate check on this pull request judges them:"
    echo
    echo "| test | doc | cr | mr | dod |"
    echo "|---|---|---|---|---|"
    echo "| $(notes_row "$tip") |"
  } > "$tmp/body"
  api_ok GET "/repos/$repo/pulls?state=open&head=$owner:$branch&per_page=100"
  open=$(jq -r --arg bot "$bot" --arg repo "$repo" --arg b "$branch" '
    [.[] | select(.head.ref == $b and .head.repo.full_name == $repo)] | map("\(.number) \(.user.login == $bot) \(.base.ref)") | first // empty' "$tmp/resp" 2> /dev/null) || fail "GitHub's list of pull requests is not readable"
  if [ -n "$open" ]; then
    set -- $open
    expr "x$1" : 'x[1-9][0-9]*$' > /dev/null || fail "GitHub's list of pull requests is not readable"
    [ "$2" = true ] || die "pull request $1 for $branch is open and the bot did not open it: refused"
    [ "$3" = "$base" ] || die "pull request $1 for $branch merges into $(shown "$3"), not $base"
    jq -n --arg t "$title" --rawfile b "$tmp/body" '{title: $t, body: $b}' > "$tmp/req"
    api_ok PATCH "/repos/$repo/pulls/$1" "$tmp/req"
    state=refreshed
  else
    jq -n --arg t "$title" --arg h "$branch" --arg base "$base" --rawfile b "$tmp/body" '{title: $t, head: $h, base: $base, body: $b, draft: true}' > "$tmp/req"
    api_ok POST "/repos/$repo/pulls" "$tmp/req"
    state=opened
  fi
  set -- $(jq -r '"\(.number) \(.html_url)"' "$tmp/resp" 2> /dev/null) || fail "GitHub's answer is not readable"
  expr "x${1:-}" : 'x[1-9][0-9]*$' > /dev/null || fail "GitHub's answer names no pull request"
  case "${2:-}" in https://github.com/*) ;; *) set -- "$1" -;; esac
  echo "PR number=$1 url=$2 state=$state head=$branch base=$base sha=$tip";;

pr-ready)
  gh_policy
  mint
  bot_pr "$n"
  [ "$(local_tip "$head_ref" || :)" = "$head_sha" ] || die "pull request $n's head $head_ref is $head_sha on GitHub, not the local tip: push first"
  if [ "$draft" = false ]; then automerge; echo "READY pr=$n head=$head_ref sha=$head_sha state=unchanged automerge=$automerge_state"; exit 0; fi
  jq -n --arg id "$node" '{query: "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { pullRequest { isDraft } } }", variables: {id: $id}}' > "$tmp/req"
  api_ok POST /graphql "$tmp/req"
  if ! jq -e '(.errors | not) and .data.markPullRequestReadyForReview.pullRequest.isDraft == false' "$tmp/resp" > /dev/null 2>&1; then
    msg=$(jq -r '.errors[0].message // empty' "$tmp/resp" 2> /dev/null | tr -d '\000-\037\177' | cut -c1-200) || msg=
    fail "GitHub did not mark pull request $n ready${msg:+: $msg}"
  fi
  # GitHub asks each commander for a review (W2's follow-up, W3): their logins from their ids. The
  # pull request is ready already, so a refusal here is a WARN line, never a failure.
  revs=
  for cid in $commanders; do
    api GET "/user/$cid"
    l=$(jq -r 'if .type == "User" then .login // empty else empty end' "$tmp/resp" 2> /dev/null) || l=
    case "$code" in 2??) ;; *) l=;; esac
    if expr "x$l" : 'x[A-Za-z0-9][A-Za-z0-9-]\{0,38\}$' > /dev/null; then revs=$revs${revs:+,}$l
    else echo "WARN no review request for commander $cid: GitHub names no user login (HTTP $code)"; fi
  done
  if [ -n "$revs" ]; then
    jq -n --arg r "$revs" '{reviewers: ($r | split(","))}' > "$tmp/req"
    api POST "/repos/$repo/pulls/$n/requested_reviewers" "$tmp/req"
    case "$code" in 2??) ;; *) echo "WARN review request for $revs refused: HTTP $code"; revs=;; esac
  fi
  automerge
  echo "READY pr=$n head=$head_ref sha=$head_sha state=ready reviewers=${revs:-none} automerge=$automerge_state";;

post)
  mint
  bot_pr "$n"
  wt=$(checked_out_at "$head_ref")
  [ -n "$wt" ] || die "no worktree of this repository has $head_ref checked out: the comment file lives in its worktree"
  # read once, safely (B2): enter the directory by its real path, then open the name without
  # following a link and copy a regular, singly linked file; the worker may swap names meanwhile
  wtr=$wt
  d=$wtr/.router-gh f=$d/$name.md
  cd -P "$d" 2> /dev/null || die "no comment file $f"
  [ "$(pwd -P)" = "$d" ] || die "$d is a link or was moved: refused"
  rc=0; perl -e '
    use Fcntl qw(O_RDONLY O_NOFOLLOW O_NONBLOCK);
    my ($src, $dst) = @ARGV;
    sysopen(my $in, $src, O_RDONLY | O_NOFOLLOW | O_NONBLOCK) or exit($!{ENOENT} ? 3 : ($!{ELOOP} || $!{EMLINK}) ? 4 : 7);
    my @st = stat($in) or exit 7;
    -f _ or exit 4;
    $st[3] == 1 or exit 5;
    ($st[7] >= 1 && $st[7] <= 60000) or exit 6;
    my ($buf, $n) = ("");
    while ($n = sysread($in, $buf, 65536, length $buf)) { exit 6 if length($buf) > 60000 }
    defined $n or exit 7;
    length($buf) >= 1 or exit 6;
    open(my $out, ">", $dst) or exit 7;
    print {$out} $buf or exit 7;
    close($out) or exit 7;' "$name.md" "$tmp/comment" || rc=$?
  case "$rc" in
    0) ;;
    3) die "no comment file $f";;
    4) die "the comment file $f must be a regular file, not a link";;
    5) die "the comment file $f has more than one link: refused";;
    6) die "the comment file $f must hold 1 to 60000 bytes";;
    *) die "cannot read the comment file $f";;
  esac
  size=$(wc -c < "$tmp/comment" | tr -d ' ')
  [ "$(LC_ALL=C tr -d '\000-\010\013-\037\177' < "$tmp/comment" | wc -c | tr -d ' ')" = "$size" ] || die "the comment file $f holds a control character (only tab and newline may)"
  iconv -f UTF-8 -t UTF-8 < "$tmp/comment" > /dev/null 2>&1 || die "the comment file $f is not UTF-8"
  tip=$(local_tip "$head_ref") || tip=$head_sha
  wj=$(printf '\342\201\240')
  {
    printf '**From the worker on `%s`** (`%s.md`, tip `%s`):\n\n' "$head_ref" "$name" "$(printf '%s' "$tip" | cut -c1-12)"
    LC_ALL=C sed "s/@/@$wj/g" "$tmp/comment"
  } > "$tmp/text"
  jq -n --rawfile b "$tmp/text" '{body: $b}' > "$tmp/req"
  api_ok POST "/repos/$repo/issues/$n/comments" "$tmp/req"
  id=$(jq -r '.id // empty' "$tmp/resp" 2> /dev/null) || id=
  expr "x$id" : 'x[1-9][0-9]*$' > /dev/null || fail "GitHub's answer names no comment"
  # the name in the directory entered above: a link put there meanwhile is removed, never followed
  rm -f "./$name.md"
  echo "POSTED pr=$n comment=$id file=$f";;

sync)
  old=$(local_tip "$branch") || die "no local branch $branch"
  mint
  # GitHub's branch lands in its remote-tracking ref; no git runs in any worktree (X1)
  track=refs/remotes/origin/$branch
  gh_git fetch --no-tags --quiet "$url" "+refs/heads/$branch:$track" || fail "fetch of $branch from GitHub failed"
  new=$(git -C "$ROOT" rev-parse --verify -q "$track^{commit}") || fail "fetch of $branch gave no commit"
  if [ "$new" = "$old" ]; then echo "SYNCED branch=$branch sha=$new state=unchanged"; exit 0; fi
  at=$(checked_out_at "$branch")
  if ! git -C "$ROOT" merge-base --is-ancestor "$old" "$new"; then
    # a WU branch with commits of its own after the owner merged a task on GitHub: its worker merges
    # the fetched ref in (E-GHFLOW2.W6); nothing moves here
    case "$branch" in "$wu_p"*) echo "SYNCED branch=$branch from=$old sha=$new state=diverged ref=$track${at:+ worktree=$at}"; exit 0;; esac
    die "not a fast-forward: the local $branch ($old) has commits GitHub's ($new, in $track) lacks; the owner decides"
  fi
  if [ -n "$at" ]; then
    echo "SYNCED branch=$branch from=$old sha=$new state=fetched ref=$track worktree=$at"
  else
    git -C "$ROOT" update-ref "refs/heads/$branch" "$new" "$old" || fail "fast-forward of $branch failed"
    echo "SYNCED branch=$branch from=$old sha=$new state=fast-forward"
  fi;;

issue)
  tip=$(local_tip "$branch") || die "no local branch $branch"
  btip=$(local_tip "$base") || die "no local base branch $base"
  mb=$(git -C "$ROOT" merge-base "refs/heads/$base" "$tip") || die "$branch shares no history with $base"
  merged=no
  if [ "$tip" != "$btip" ] && git -C "$ROOT" merge-base --is-ancestor "$tip" "$btip"; then merged=yes; fi
  case "$branch" in
    "$epic_p"*) kind_plan=epic-plan epic=$branch;;
    *) kind_plan=wu-plan e=${branch#"$wu_p"}; epic=$epic_p${e%%/*};;
  esac
  doc_rows "$mb" "$tip" "$kind_plan"
  [ -n "$title" ] || title=$branch
  # the issue is found again by this first line, among the bot's issues
  marker="<!-- keel:router-gh issue v1 $branch -->" label=keel-tracking
  {
    echo "$marker"
    if [ "$branch" = "$epic" ]; then
      echo "Tracking issue of \`$branch\` into \`$base\`, kept by the Keel router bot from git. Each work unit is a sub-issue."
    else
      echo "Work unit \`$branch\` into \`$base\`, kept by the Keel router bot from git."
    fi
    echo
    echo "**Tip:** \`$tip\` · **merged into \`$base\`:** $merged"
    echo
    docs_md
    echo
    echo "**Notes on the tip:**"
    echo
    echo "| test | doc | cr | mr | dod |"
    echo "|---|---|---|---|---|"
    echo "| $(notes_row "$tip") |"
    if [ "$branch" = "$epic" ]; then
      wus=$wu_p${epic#"$epic_p"}
      echo
      echo "**Work units**, the local \`$wus/*\` branches:"
      echo
      wrows=
      for w in $(git -C "$ROOT" for-each-ref --format='%(refname:short)' "refs/heads/$wus"); do
        expr "x$w" : "x$wus/$seg\$" > /dev/null || continue
        wt=$(local_tip "$w") || continue
        win=no
        if [ "$wt" != "$tip" ] && git -C "$ROOT" merge-base --is-ancestor "$wt" "$tip"; then win=yes; fi
        wrows="$wrows| \`$w\` | \`$(printf '%s' "$wt" | cut -c1-12)\` | $win | $(notes_row "$wt") |
"
      done
      if [ -n "$wrows" ]; then
        echo "| Branch | Tip | In the epic | test | doc | cr | mr | dod |"
        echo "|---|---|---|---|---|---|---|---|"
        printf '%s' "$wrows"
      else
        echo "No work unit branch here."
      fi
    fi
  } > "$tmp/body"
  mint
  # only issues carrying the bot's label (security review I1): the list stays short whatever the
  # repository holds; only users with triage rights can label, and the author is checked too
  pages "/repos/$repo/issues" "state=all&labels=$label&" .
  jq -r --arg bot "$bot" '.[] | select(.pull_request == null and .user.login == $bot and .user.type == "Bot")
    | ((.body // "") | split("\n")[0] | rtrimstr("\r")) as $m
    | select($m | startswith("<!-- keel:router-gh issue v1 ") and endswith(" -->"))
    | "\($m | ltrimstr("<!-- keel:router-gh issue v1 ") | rtrimstr(" -->"))\t\(.number)\t\(.id)"' "$tmp/all" > "$tmp/ours" 2> /dev/null \
    || fail "GitHub's list of issues is not readable"
  # our_issue <branch>: "<number> <id>" of the bot's issue for it, the newest first
  our_issue() {
    fr=$(awk -F '\t' -v b="$1" '$1 == b { print $2 " " $3; exit }' "$tmp/ours")
    [ -z "$fr" ] || expr "x$fr" : 'x[1-9][0-9]* [1-9][0-9]*$' > /dev/null || fail "GitHub's list of issues is not readable"
    printf '%s' "$fr"
  }
  parent=-
  if [ "$branch" != "$epic" ]; then
    p=$(our_issue "$epic")
    [ -n "$p" ] || die "no tracking issue for $epic yet: run router-gh.sh issue $epic first"
    parent=${p%% *}
  fi
  want=open; [ "$merged" = no ] || want=closed
  mine=$(our_issue "$branch")
  if [ -n "$mine" ]; then
    num=${mine%% *} iid=${mine#* }
    jq -n --arg t "$title" --rawfile b "$tmp/body" --arg s "$want" '{title: $t, body: $b, state: $s}' > "$tmp/req"
    api_ok PATCH "/repos/$repo/issues/$num" "$tmp/req"
    state=refreshed
  else
    # the label exists before an issue carries it (GitHub may drop an unknown one)
    api GET "/repos/$repo/labels/$label"
    case "$code" in
      200) ;;
      404) jq -n --arg n "$label" '{name: $n, color: "0e8a16", description: "Kept by the Keel router bot: an epic tracking issue or a WU sub-issue"}' > "$tmp/req"
           api_ok POST "/repos/$repo/labels" "$tmp/req";;
      *) api_ok GET "/repos/$repo/labels/$label";;
    esac
    jq -n --arg t "$title" --rawfile b "$tmp/body" --arg l "$label" '{title: $t, body: $b, labels: [$l]}' > "$tmp/req"
    api_ok POST "/repos/$repo/issues" "$tmp/req"
    set -- $(jq -r '"\(.number) \(.id)"' "$tmp/resp" 2> /dev/null) || fail "GitHub's answer is not readable"
    expr "x${1:-} ${2:-}" : 'x[1-9][0-9]* [1-9][0-9]*$' > /dev/null || fail "GitHub's answer names no issue"
    num=$1 iid=$2
    if [ "$want" = closed ]; then
      jq -n '{state: "closed"}' > "$tmp/req"
      api_ok PATCH "/repos/$repo/issues/$num" "$tmp/req"
    fi
    state=opened
  fi
  url=$(jq -r '.html_url // empty' "$tmp/resp" 2> /dev/null) || url=
  case "$url" in https://github.com/*) expr "x$url" : 'x[A-Za-z0-9:/._-]*$' > /dev/null || url=-;; *) url=-;; esac
  if [ "$parent" != - ]; then
    # a sub-issue of the epic's, linked once (sub_issue_id is the issue's id, not its number)
    pages "/repos/$repo/issues/$parent/sub_issues" "" .
    if ! jq -e --argjson i "$iid" 'any(.[]; .id == $i)' "$tmp/all" > /dev/null 2>&1; then
      jq -n --argjson i "$iid" '{sub_issue_id: $i}' > "$tmp/req"
      api_ok POST "/repos/$repo/issues/$parent/sub_issues" "$tmp/req"
    fi
  fi
  [ "$want" = open ] && o=yes || o=no
  echo "ISSUE number=$num url=$url state=$state open=$o branch=$branch parent=$parent";;

watch-read)
  gh_policy
  [ -n "$commanders" ] || die "no [github] commanders = [<user id>] in .keel/config.toml: the watch would hear nobody"
  cids=$(printf '%s' "$commanders" | tr ' ' ,)
  mint
  # only an issue or pull request the bot opened, from this repository
  if [ "$wkind" = comment ]; then
    api GET "/repos/$repo/issues/$n"
    case "$code" in 200) ;; 404|410) die "no issue or pull request $n in $repo";; *) api_ok GET "/repos/$repo/issues/$n";; esac
    ispr=$(jq -r --arg bot "$bot" 'if (.user.login == $bot and .user.type == "Bot") then (.pull_request != null | tostring) else "foreign" end' "$tmp/resp" 2> /dev/null) || ispr=
    case "$ispr" in true|false) ;; foreign) die "issue $n is not one the bot opened: refused";; *) fail "GitHub's answer for issue $n is not readable";; esac
    pages_newest "/repos/$repo/issues/$n/comments"
    jq -c --argjson c "[$cids]" --argjson ft "$free_text" --arg bot "$bot" --argjson pr "$ispr" --argjson tr "$truncated" '{kind: "comment", pr: $pr, commanders: $c, free_text: $ft, truncated: $tr,
      items: map({id, user_id: .user.id, user_type: .user.type, self: (.user.login == $bot and .user.type == "Bot"),
        via_app: (.performed_via_github_app != null), created_at, updated_at, body: (.body // "")})}' "$tmp/all" \
      || fail "GitHub's comments on $n are not readable"
    exit 0
  fi
  api GET "/repos/$repo/pulls/$n"
  case "$code" in 200) ;; 404|410) die "no pull request $n in $repo";; *) api_ok GET "/repos/$repo/pulls/$n";; esac
  head_sha=$(jq -r --arg bot "$bot" --arg repo "$repo" '
    if (.user.login == $bot and .user.type == "Bot" and .head.repo.full_name == $repo and .base.repo.full_name == $repo)
    then .head.sha else "foreign" end' "$tmp/resp" 2> /dev/null) || head_sha=
  [ "$head_sha" != foreign ] || die "pull request $n is not one the bot opened from $repo: refused"
  expr "x$head_sha" : 'x[0-9a-f]\{40\}$' > /dev/null || fail "GitHub's head sha for pull request $n is not a sha"
  case "$wkind" in
    review)
      pages_newest "/repos/$repo/pulls/$n/reviews"
      jq -c --argjson c "[$cids]" --argjson ft "$free_text" --arg bot "$bot" --arg h "$head_sha" --argjson tr "$truncated" '{kind: "review", head: $h, commanders: $c, free_text: $ft, truncated: $tr,
        items: map({id, user_id: .user.id, user_type: .user.type, self: (.user.login == $bot and .user.type == "Bot"),
          via_app: (.performed_via_github_app != null), state, commit_id, submitted_at, body: (.body // "")})}' "$tmp/all" \
        || fail "GitHub's reviews of $n are not readable";;
    merged)
      jq -c --argjson c "[$cids]" '{kind: "merged", commanders: $c, head: .head.sha, base: .base.ref, state, merged: (.merged == true),
        merge_commit_sha, merged_by_id: (.merged_by.id // null), merged_by_type: (.merged_by.type // null), at: (.merged_at // .closed_at)}' "$tmp/resp" \
        || fail "GitHub's answer for pull request $n is not readable";;
    conflict)
      # E-GHFLOW2.W6: whether GitHub can merge it; the watch judges mergeable as a JSON boolean and
      # mergeable_state as an exact word
      jq -c '{kind: "conflict", head: .head.sha, base: .base.ref, state, merged: (.merged == true), mergeable, mergeable_state,
        at: (.merged_at // .closed_at // .updated_at)}' "$tmp/resp" || fail "GitHub's answer for pull request $n is not readable";;
    checks)
      pages "/repos/$repo/commits/$head_sha/check-runs" "" .check_runs
      mv "$tmp/all" "$tmp/runs"
      api_ok GET "/repos/$repo/commits/$head_sha/status?per_page=100"
      jq -c -n --arg h "$head_sha" --slurpfile r "$tmp/runs" --slurpfile s "$tmp/resp" '{kind: "checks", head: $h,
        runs: ($r[0] | map({name, status, conclusion, completed_at})), statuses: (($s[0].statuses // []) | map({context, state}))}' \
        || fail "GitHub's checks on $head_sha are not readable";;
  esac;;
esac
