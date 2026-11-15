#!/bin/sh
# The router's one door to GitHub (E-GHFLOW.W2, DEC-46 amended by the owner 2026-11-15: the router bot
# pushes non-main branches). It acts as a GitHub App installed on this repository only, never as the
# owner: the owner cannot approve a pull request they authored. The router's Bash hook admits only
# these shapes, and every argument is validated here before any other step:
#   router-gh.sh push     <branch>                         wu/<E>/<slug> or epic/<E>, with every notes ref
#   router-gh.sh pr-open  <branch> [--base main|epic/<E>]  open a draft pull request, or refresh the bot's open one
#   router-gh.sh pr-ready <n>                              mark the bot's pull request ready for review
#   router-gh.sh post     <n> <body-name>                  post <worktree of its head>/.router-gh/<body-name>.md
#   router-gh.sh sync     <epic/E>                         fetch GitHub's epic; fast-forward it here unless checked out
#   (router-gh.sh credential <file> <op>: git's one-shot credential helper only; never a router shape)
# Exit 2 = refused (arguments, setup, a precondition); exit 1 = GitHub or git refused the request.
# Output: one KEY line (PUSHED, PR, READY, POSTED, SYNCED) of k=v words; WARN lines; errors on stderr.
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
# ride the same push: only Keel's five kinds (test, doc, cr, mr, dod). Git runs only in this checkout,
# never in another worktree (round 2, X1): sync fetches GitHub's branch into refs/remotes/origin/<b>
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
set -euf
# byte-wise, whatever the caller's locale: [A-Za-z] means ASCII letters in every check below
LC_ALL=C; export LC_ALL
for v in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_NO_REPLACE_OBJECTS=1
export GIT_CONFIG_NOSYSTEM GIT_CONFIG_GLOBAL GIT_NO_REPLACE_OBJECTS
umask 077
die() { echo "router-gh: $*" >&2; exit 2; }
fail() { echo "router-gh: $*" >&2; exit 1; }
# an argument as printed in an error: no control character, at most 80 bytes
shown() { printf '%s' "$1" | tr -d '\000-\037\177' | cut -c1-80; }
usage="usage: push <branch> | pr-open <branch> [--base <main|epic/E>] | pr-ready <n> | post <n> <body-name> | sync <epic/E>"
# Keel's branch policy (.keel/config.toml [branches], as scripts/router-worker.sh holds it)
main_b=main epic_p=epic/ wu_p=wu/
seg='[A-Za-z0-9][A-Za-z0-9._-]*'
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
# work_branch <b>: wu/<E>/<slug> or epic/<E>; sets base to the branch it merges into
work_branch() {
  nodash "$1"
  if expr "x$1" : "x$epic_p$seg\$" > /dev/null; then base=$main_b
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
    [ $# -eq 1 ] || die "usage: sync <epic/E>"
    branch=$1; nodash "$branch"
    expr "x$branch" : "x$epic_p$seg\$" > /dev/null || die "sync takes an $epic_p<E> branch only: $(shown "$branch")"
    refname "$branch";;
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
    case "$t" in ''|*[!A-Za-z0-9_]*) exit 0;; esac
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
gd=$(cd "$ROOT" && cd "$(git rev-parse --git-common-dir)" && pwd -P) || die "cannot resolve this repository's git dir"
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
docs=$(toml docs root) || docs=docs/
expr "x$docs" : 'x[A-Za-z0-9_][A-Za-z0-9_./-]*/$' > /dev/null || die "[docs] root must be a relative directory ending in /"
API=${ROUTER_GH_API:-https://api.github.com}
case "$API" in
  https://api.github.com) ;;
  https://127.0.0.1:*|https://localhost:*) expr "x${API##*:}" : 'x[1-9][0-9]\{0,4\}$' > /dev/null || die "ROUTER_GH_API: bad port";;
  *) die "ROUTER_GH_API must be https://api.github.com or an https loopback URL";;
esac
GITBASE=${ROUTER_GH_GIT:-https://github.com}
case "$GITBASE" in https://github.com|file:///*) ;; *) die "ROUTER_GH_GIT must be https://github.com or a file:/// directory";; esac
case "$GITBASE" in *[!A-Za-z0-9/:._-]*|*..*) die "ROUTER_GH_GIT: plain path characters only";; esac
url=$GITBASE/$repo.git
CURL=${ROUTER_GH_CURL:-curl}
# the run dir: under the key dir, which workers cannot read (X2), never the shared $TMPDIR
run=$kdir/run
[ -d "$run" ] || [ -L "$run" ] || mkdir "$run" || die "cannot make $run"
[ -d "$run" ] && [ ! -L "$run" ] || die "$run must be a directory, not a link"
tmp=
trap '[ -z "$tmp" ] || rm -rf "$tmp"' EXIT
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
  rm -f "$tmp/resp"
  rc=0
  code=$({
    printf 'url = "%s%s"\nrequest = "%s"\n' "$API" "$ap" "$am"
    printf 'header = "Authorization: Bearer %s"\n' "$ah"
    printf 'header = "Accept: application/vnd.github+json"\nheader = "X-GitHub-Api-Version: 2022-11-28"\nheader = "User-Agent: keel-router-gh"\n'
    [ -z "$ab" ] || printf 'header = "Content-Type: application/json"\ndata-binary = "@%s"\n' "$ab"
    printf 'output = "%s"\nwrite-out = "%%{http_code}"\nsilent\nshow-error\nproto = "=https"\nconnect-timeout = 15\nmax-time = 60\n' "$tmp/resp"
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
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
# the installation token, restricted to this repository; the JWT that bought it lives 10 minutes
mint() {
  now=$(date +%s)
  h=$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)
  p=$(printf '{"iat":%s,"exp":%s,"iss":"%s"}' "$((now - 60))" "$((now + 540))" "$app_id" | b64url)
  s=$(printf '%s.%s' "$h" "$p" | openssl dgst -sha256 -sign "$key" -binary 2> /dev/null | b64url) || s=
  [ ${#s} -ge 300 ] || die "cannot sign with the App key $key (an RSA private key in PEM form)"
  jwt=$h.$p.$s tok=
  jq -n --arg r "$rname" '{repositories: [$r]}' > "$tmp/tokreq"
  api_ok POST "/app/installations/$inst/access_tokens" "$tmp/tokreq" jwt
  tok=$(jq -r '.token // empty' "$tmp/resp" 2> /dev/null) || tok=
  rm -f "$tmp/resp"
  jwt=
  case "$tok" in ''|*[!A-Za-z0-9_]*) fail "GitHub returned no usable installation token";; esac
  [ ${#tok} -ge 20 ] && [ ${#tok} -le 255 ] || fail "GitHub returned no usable installation token"
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
# epic/, base main or epic/; sets head_ref, head_sha, pr_base, draft, node
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
  [ "$pr_base" = "$main_b" ] || expr "x$pr_base" : "x$epic_p$seg\$" > /dev/null || die "pull request $n merges into $(shown "$pr_base"), not $main_b or an epic"
  expr "x$head_sha" : 'x[0-9a-f]\{40\}$' > /dev/null || fail "GitHub's head sha for pull request $n is not a sha"
  expr "x$node" : 'x[A-Za-z0-9_=-]\{1,100\}$' > /dev/null || fail "GitHub's node id for pull request $n is not readable"
}
# doc text made plain: no markup, link, mention, table cell or control character survives
plain() { LC_ALL=C tr -d '\000-\037\177`*_[]<>@#|\\~!$^{}"' | LC_ALL=C cut -c1-200; }

case "$mode" in
push)
  tip=$(local_tip "$branch") || die "no local branch $branch"
  # Keel's code-free checks only; the pre-push hook, which runs the pushed code, is skipped (B1)
  if [ "$base" != "$main_b" ]; then
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
  set -- "refs/heads/$branch:refs/heads/$branch"
  for k in test doc cr mr dod; do
    git -C "$ROOT" show-ref --verify -q "refs/notes/$k" && set -- "$@" "refs/notes/$k:refs/notes/$k"
  done
  rc=0; out=$(gh_git push --porcelain --no-verify --no-follow-tags "$url" "$@") || rc=$?
  line=$(printf '%s\n' "$out" | awk -F '\t' -v r="refs/heads/$branch:refs/heads/$branch" '$2 == r { print; exit }')
  [ -n "$line" ] || fail "push of $branch failed before any ref moved (git exit $rc; see above)"
  case "$(printf '%s' "$line" | cut -c1)" in
    '*') state=new;; ' ') state=updated;; '=') state=unchanged;;
    *) why=$(printf '%s' "$line" | awk -F '\t' '{ print $3 }' | tr -d '\000-\037\177' | cut -c1-120)
       fail "push of $branch rejected: $why (never forced: merge GitHub's branch in, or ask the owner)";;
  esac
  bad=$(printf '%s\n' "$out" | awk -F '\t' '$2 ~ /^refs\/notes\// && substr($0, 1, 1) == "!" { print $2 "\t" $3 }')
  printf '%s\n' "$bad" | while IFS='	' read -r ref why; do
    [ -z "$ref" ] || echo "WARN notes ref ${ref%%:*} not pushed: $(printf '%s' "$why" | tr -d '\000-\037\177' | cut -c1-120)"
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
  # the docs this range changed, as they are at the tip
  title= rows=
  for f in $(git -C "$ROOT" diff --name-only --diff-filter=AMR "$mb" "$tip" -- "$docs"); do
    case "$f" in *.html) ;; *) continue;; esac
    expr "x$f" : 'x[A-Za-z0-9_][A-Za-z0-9_./-]*$' > /dev/null || continue
    case "$f" in *..*) continue;; esac
    top=$(git -C "$ROOT" show "$tip:$f" | head -c 8192 | LC_ALL=C tr '\n\r\t' '   ')
    role=$(printf '%s' "$top" | LC_ALL=C sed -n 's/.*<html[^>]* data-artifact="\([a-z -]*\)".*/\1/p' | cut -c1-40)
    status=$(printf '%s' "$top" | LC_ALL=C sed -n 's/.*<html[^>]* data-status="\([a-z-]*\)".*/\1/p' | cut -c1-20)
    t=$(printf '%s' "$top" | LC_ALL=C sed -n 's/.*<title>\([^<]*\)<\/title>.*/\1/p' | plain)
    [ -n "$t" ] || t=${f##*/}
    case " $role " in *" $kind_plan "*) [ -n "$title" ] || title=$t;; esac
    rows="$rows| [$t](https://github.com/$repo/blob/$tip/$f) | ${role:-—} | ${status:-—} |
"
  done
  note_state() {
    if v=$(git -C "$ROOT" notes --ref="$1" show "$tip" 2> /dev/null); then
      if [ "$1" = cr ]; then
        verdict=$(printf '%s' "$v" | jq -r '.verdict // empty' 2> /dev/null) || verdict=
        round=$(printf '%s' "$v" | jq -r '.round // empty' 2> /dev/null) || round=
        expr "x$verdict" : 'x[A-Z]\{1,12\}$' > /dev/null || verdict=unreadable
        expr "x$round" : 'x[0-9]\{1,3\}$' > /dev/null && echo "$verdict, round $round" || echo "$verdict"
      else echo yes; fi
    else echo —; fi
  }
  [ -n "$title" ] || title=$branch
  {
    echo "<!-- keel:router-gh pr-body v1 -->"
    echo "Opened by the Keel router bot for \`$branch\` into \`$base\`."
    echo
    echo "**Tip:** \`$tip\`"
    echo
    if [ -n "$rows" ]; then
      echo "**Docs this range changed**, at the tip:"
      echo
      echo "| Doc | Role | Status |"
      echo "|---|---|---|"
      printf '%s' "$rows"
    else
      echo "No doc changed in this range."
    fi
    echo
    echo "**Notes on the tip** when the router last opened or refreshed this page; the gate check on this pull request judges them:"
    echo
    echo "| test | doc | cr | mr | dod |"
    echo "|---|---|---|---|---|"
    echo "| $(note_state test) | $(note_state doc) | $(note_state cr) | $(note_state mr) | $(note_state dod) |"
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
  mint
  bot_pr "$n"
  [ "$(local_tip "$head_ref" || :)" = "$head_sha" ] || die "pull request $n's head $head_ref is $head_sha on GitHub, not the local tip: push first"
  if [ "$draft" = false ]; then echo "READY pr=$n head=$head_ref sha=$head_sha state=unchanged"; exit 0; fi
  jq -n --arg id "$node" '{query: "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { pullRequest { isDraft } } }", variables: {id: $id}}' > "$tmp/req"
  api_ok POST /graphql "$tmp/req"
  if ! jq -e '(.errors | not) and .data.markPullRequestReadyForReview.pullRequest.isDraft == false' "$tmp/resp" > /dev/null 2>&1; then
    msg=$(jq -r '.errors[0].message // empty' "$tmp/resp" 2> /dev/null | tr -d '\000-\037\177' | cut -c1-200) || msg=
    fail "GitHub did not mark pull request $n ready${msg:+: $msg}"
  fi
  echo "READY pr=$n head=$head_ref sha=$head_sha state=ready";;

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
  git -C "$ROOT" merge-base --is-ancestor "$old" "$new" || die "not a fast-forward: the local $branch ($old) has commits GitHub's ($new, in $track) lacks; the owner decides"
  at=$(checked_out_at "$branch")
  if [ -n "$at" ]; then
    echo "SYNCED branch=$branch from=$old sha=$new state=fetched ref=$track worktree=$at"
  else
    git -C "$ROOT" update-ref "refs/heads/$branch" "$new" "$old" || fail "fast-forward of $branch failed"
    echo "SYNCED branch=$branch from=$old sha=$new state=fast-forward"
  fi;;
esac
