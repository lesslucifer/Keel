#!/bin/sh
# The sign-off writer (E-GHFLOW2.W3): an approval of a pull request becomes its mr/v1 note.
#   scripts/ci-sign-off.sh
# Run by .github/workflows/keel-sign-off.yml on workflow_run, so this file, the workflow and the
# checkout are main's, never the pull request's. It trusts nothing from the run that started it: it
# takes the pull request's number from GitHub's own workflow_run payload (or, when that is empty, the
# one open pull request whose head is the run's commit), then re-reads the pull request and every
# review from the API, and lets keel pick the approval by the base's committed roster and policy and
# write the note (keel ci sign-off). The job holds a write token, so it runs no code a worker can
# reach (security review I4): keel is built from this checkout, main, which only the owner moves
# (DEC-61); the base (an epic, which the router bot pushes) and the head are git data only - read
# by keel from a base worktree, never built or run. The note goes to origin's refs/notes/mr, and the
# keel/sign-off status says what happened on the head commit, and what the approval signed over
# (E-GHFLOW2.W7: a review that did not pass, a roster change); a run that stops after it knows the
# head says so too (status error).
# Input, from the workflow's env (never from a run: line):
#   GH_TOKEN      the workflow's token (contents and statuses write, pull-requests read)
#   REPO          owner/name of this repository
#   PR_NUMBER     optional: workflow_run.pull_requests[0].number
#   RUN_HEAD_SHA  workflow_run.head_sha, used only to find the pull request when PR_NUMBER is empty
#   RUN_EVENT     workflow_run.event; anything but pull_request_review signs nothing
#   KEEL          optional: a keel to use; else one is built from this checkout (main)
#   API, CURL     test seams (https://api.github.com and curl by default)
# Exit 0 = done (signed, or nothing to sign, said in the status); 1 = GitHub, git or keel failed;
# 2 = bad input. The token reaches curl only on its stdin and git only through a one-shot
# credential helper reading a 0600 file in a private temp dir; no child but those two gets it.
set -euf
LC_ALL=C
export LC_ALL
# the writer's own identity, for keel's notes (their committer) and every git commit it makes: a
# GitHub runner has no [user] and Linux git guesses none, so without these keel stops (E-GHFLOW2.W10)
GIT_AUTHOR_NAME=keel-sign-off
GIT_AUTHOR_EMAIL=keel-sign-off@users.noreply.github.com
GIT_COMMITTER_NAME=$GIT_AUTHOR_NAME
GIT_COMMITTER_EMAIL=$GIT_AUTHOR_EMAIL
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

die() {
	printf 'ci-sign-off: %s\n' "$*" >&2
	exit 2
}
fail() {
	printf 'ci-sign-off: %s\n' "$*" >&2
	exit 1
}

branch_ok() {
	case $1 in
	'' | -* | *..* | *' '* | *'	'* | *@\{* | *\\*) return 1 ;;
	esac
	git check-ref-format --branch "$1" >/dev/null 2>&1
}
sha_ok() {
	printf '%s\n' "$1" | grep -Eqx '[0-9a-f]{40}|[0-9a-f]{64}'
}
# token_ok <t>: as scripts/router-gh.sh takes an installation token: 1 to 4096 bytes of printable
# ASCII with no space, quote or backslash, inert in the curl config's quoted header line and git's
# password= line
token_ok() {
	case "$1" in '' | *[!!-~]* | *\"* | *\'* | *\\*) return 1 ;; esac
	[ ${#1} -le 4096 ]
}

[ $# -eq 0 ] || die "usage: scripts/ci-sign-off.sh   (env GH_TOKEN, REPO, RUN_EVENT; PR_NUMBER or RUN_HEAD_SHA)"
tok=${GH_TOKEN:-}
unset GH_TOKEN
repo=${REPO:-}
pr=${PR_NUMBER:-}
run_sha=${RUN_HEAD_SHA:-}
API=${API:-https://api.github.com}
CURL=${CURL:-curl}
token_ok "$tok" || die "GH_TOKEN is not a token GitHub mints (1 to 4096 printable bytes, no space, quote or backslash)"
printf '%s\n' "$repo" | grep -Eqx '[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}' || die "REPO is not owner/name: '$repo'"
if [ "${RUN_EVENT:-}" != pull_request_review ]; then
	echo "ci-sign-off: the run was started by '${RUN_EVENT:-}', not a review: nothing to sign" >&2
	exit 0
fi
[ -z "$pr" ] || printf '%s\n' "$pr" | grep -Eqx '[1-9][0-9]{0,9}' || die "PR_NUMBER is not a number: '$pr'"
[ -z "$run_sha" ] || sha_ok "$run_sha" || die "RUN_HEAD_SHA is not a full commit id: '$run_sha'"
[ -n "$pr" ] || [ -n "$run_sha" ] || die "neither PR_NUMBER nor RUN_HEAD_SHA"

# main as the workflow checked it out, read before any ref here moves (security review round 2, F9)
main_sha=$(git rev-parse -q --verify 'HEAD^{commit}') || die "no commit at HEAD"
scratch=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/keel-sign-off.XXXXXX") || die "cannot make a scratch directory"
[ -n "$scratch" ] && [ -d "$scratch" ] || die "cannot make a scratch directory"
chmod 700 "$scratch"
repo_dir=$(pwd)
head_sha=
posted=
cleanup() {
	rc=$?
	# a run that stops once it knows the head leaves a status on it (security review N4)
	if [ "$rc" -ne 0 ] && [ -n "$head_sha" ] && [ -z "$posted" ]; then
		(status error "The sign-off writer stopped (exit $rc); its log says why") || true
	fi
	cd "$repo_dir" || return
	git worktree remove --force "$scratch/base" >/dev/null 2>&1 || true
	rm -rf "$scratch"
}
trap cleanup EXIT

# api <method> <path> [<json file>]: the body lands in $scratch/resp, the status in $code; the
# request, its Authorization header included, reaches curl only on its stdin
api() {
	rm -f "$scratch/resp"
	rc=0
	code=$({
		printf 'url = "%s%s"\nrequest = "%s"\n' "$API" "$2" "$1"
		printf 'header = "Authorization: Bearer %s"\n' "$tok"
		printf 'header = "Accept: application/vnd.github+json"\nheader = "X-GitHub-Api-Version: 2022-11-28"\nheader = "User-Agent: keel-sign-off"\n'
		[ -z "${3:-}" ] || printf 'header = "Content-Type: application/json"\ndata-binary = "@%s"\n' "$3"
		printf 'output = "%s"\nwrite-out = "%%{http_code}"\nsilent\nshow-error\nproto = "=https"\nconnect-timeout = 15\nmax-time = 60\n' "$scratch/resp"
	} | "$CURL" -q --config -) || rc=$?
	[ "$rc" -eq 0 ] || fail "GitHub unreachable: $1 $2 (curl exit $rc)"
	printf '%s\n' "$code" | grep -Eqx '[1-5][0-9][0-9]' || fail "GitHub gave no status: $1 $2"
	[ -f "$scratch/resp" ] || : >"$scratch/resp"
	case $code in 2??) ;; *) fail "GitHub refused $1 $2: HTTP $code" ;; esac
}

# status <state> <description>: keel/sign-off on the head commit
status() {
	jq -n --arg s "$1" --arg d "$(printf '%s' "$2" | tr -d '\000-\037\177' | cut -c1-140)" \
		'{state: $s, context: "keel/sign-off", description: $d}' >"$scratch/status.json"
	api POST "/repos/$repo/statuses/$head_sha" "$scratch/status.json"
	posted=1
	echo "ci-sign-off: keel/sign-off $1 on $head_sha: $2" >&2
}

if [ -z "$pr" ]; then
	api GET "/repos/$repo/commits/$run_sha/pulls"
	pr=$(jq -r --arg r "$repo" --arg s "$run_sha" \
		'[.[]? | select(.state == "open" and .head.sha == $s and .head.repo.full_name == $r and .base.repo.full_name == $r) | .number] | if length == 1 then .[0] else empty end' \
		"$scratch/resp" 2>/dev/null) || pr=
	if [ -z "$pr" ]; then
		echo "ci-sign-off: no single open pull request of $repo has the head $run_sha: nothing to sign" >&2
		exit 0
	fi
	printf '%s\n' "$pr" | grep -Eqx '[1-9][0-9]{0,9}' || fail "GitHub's pull request number is not a number"
fi

api GET "/repos/$repo/pulls/$pr"
facts=$(jq -r --arg r "$repo" '
	if (.state == "open" and .head.repo.full_name == $r and .base.repo.full_name == $r) then
		[.head.ref, .head.sha, .base.ref] | map(tostring) | join(" ")
	else "no" end' "$scratch/resp" 2>/dev/null) || fail "GitHub's pull request $pr is not JSON"
if [ "$facts" = no ]; then
	echo "ci-sign-off: pull request $pr is closed or crosses repositories: nothing to sign" >&2
	exit 0
fi
set -- $facts
[ $# -eq 3 ] || fail "GitHub's pull request $pr lacks a head or a base"
head_ref=$1 head_sha=$2 base_ref=$3
branch_ok "$head_ref" && branch_ok "$base_ref" || fail "pull request $pr's branches are not branch names"
sha_ok "$head_sha" || {
	head_sha=
	fail "pull request $pr's head is not a full commit id"
}
if [ -n "$run_sha" ] && [ "$run_sha" != "$head_sha" ]; then
	echo "ci-sign-off: the run's commit $run_sha is no longer the head $head_sha of $pr: the newer head's review run signs it" >&2
	exit 0
fi
case $head_ref in
wu/*/*)
	epic=${head_ref#wu/}
	level=wu
	want=epic/${epic%%/*}
	;;
epic/?*)
	level=epic
	want=main
	;;
*) want= ;;
esac
if [ -z "$want" ] || [ "$base_ref" != "$want" ]; then
	echo "ci-sign-off: $head_ref into $base_ref is not a Keel WU or epic pull request: nothing to sign" >&2
	exit 0
fi

# every review, 100 a page, at most 10 pages
printf '[]' >"$scratch/reviews.json"
page=1
while :; do
	api GET "/repos/$repo/pulls/$pr/reviews?per_page=100&page=$page"
	jq -e -c 'arrays' "$scratch/resp" >"$scratch/page" 2>/dev/null || fail "GitHub's reviews of $pr are not a list"
	jq -c -s '.[0] + .[1]' "$scratch/reviews.json" "$scratch/page" >"$scratch/all" && mv "$scratch/all" "$scratch/reviews.json"
	[ "$(jq length "$scratch/page")" -ge 100 ] || break
	page=$((page + 1))
	[ "$page" -le 10 ] || fail "more than 1000 reviews on $pr"
done

# git data only: the base and head branches as origin holds them, and every notes ref
git fetch -q --no-tags origin "+refs/heads/$base_ref:refs/remotes/origin/$base_ref" "+refs/heads/$head_ref:refs/remotes/origin/$head_ref" '+refs/notes/*:refs/notes/*' ||
	fail "cannot fetch $base_ref, $head_ref and the notes from origin"
if [ "$(git rev-parse -q --verify "refs/remotes/origin/$head_ref^{commit}")" != "$head_sha" ]; then
	echo "ci-sign-off: $head_ref moved past $head_sha on origin: the next review's run signs its new head" >&2
	exit 0
fi
base_sha=$(git rev-parse -q --verify "refs/remotes/origin/$base_ref^{commit}") || fail "no $base_ref on origin"
git update-ref "refs/heads/$head_ref" "$head_sha"
git update-ref "refs/heads/$base_ref" "$base_sha"

git worktree add -q --detach "$scratch/base" "$base_sha"
if [ -z "${KEEL:-}" ]; then
	echo "ci-sign-off: building keel from main at $main_sha" >&2
	(cd "$repo_dir" && CARGO_TARGET_DIR=$scratch/target cargo build --locked --quiet) >&2 || fail "cannot build keel from main"
	KEEL=$scratch/target/debug/keel
fi
case $KEEL in /*) ;; *) die "KEEL must be an absolute path: '$KEEL'" ;; esac
[ -x "$KEEL" ] || die "no keel at $KEEL"
cd "$scratch/base"
case $("$KEEL" ci 2>&1 || true) in
*"keel ci sign-off"*) ;;
*)
	status error "The keel of main predates sign-off by approval; sign with keel mr confirm"
	exit 0
	;;
esac
"$KEEL" hooks install >&2
# an approval attests only a report committed in the range, which GitHub showed (security review I6)
set -- ci sign-off "$level" "$head_ref" --target "$base_ref" --reviews "$scratch/reviews.json"
rc=0
"$KEEL" "$@" >"$scratch/keel.out" 2>&1 || rc=$?
cat "$scratch/keel.out" >&2
short=$(printf '%s' "$head_sha" | cut -c1-10)
case $rc in
0) ;;
4)
	# already signed: a note check 8 accepts is on the head; nothing was written. Say who signed, as
	# the note says - "by approval" only for a github-tier note
	note=$(git notes --ref=mr show "$head_sha") || fail "keel says $short is already signed, but its note cannot be read"
	tier=$(printf '%s' "$note" | jq -r '.tier // empty')
	signer=$(printf '%s' "$note" | jq -r '.signer // empty')
	case $tier in
	github) status success "Already signed by approval (github $signer, review $(printf '%s' "$note" | jq -r '.review.id // empty'))" ;;
	email | username | key) status success "Already signed by $signer ($tier)" ;;
	*) status error "Already signed, but the note on $short names no known tier" ;;
	esac
	exit 0
	;;
3)
	status pending "Waiting for an approval of $short by a signer the roster lists"
	exit 0
	;;
1)
	# a refusal or a red check names itself; any other keel error is named by its own line, never
	# taken for a red check
	why=$(grep -m 1 -E 'refused|FAIL' "$scratch/keel.out" || grep -m 1 '^keel: ' "$scratch/keel.out" || echo "keel stopped (exit 1); the run's log says why")
	status failure "Approval not signed: $why"
	exit 0
	;;
*)
	status error "The sign-off writer failed (keel exit $rc)"
	exit 1
	;;
esac

# the note to origin; a notes ref that moved meanwhile is merged in, keeping ours, then pushed again
cd "$repo_dir"
helper="!f() { test \"\$1\" = get || exit 0; echo username=x-access-token; printf 'password=%s\\n' \"\$(cat '$scratch/token')\"; }; f"
pushed=
for try in 1 2 3; do
	printf '%s' "$tok" >"$scratch/token"
	prc=0
	GIT_TERMINAL_PROMPT=0 git -c credential.helper= -c "credential.helper=$helper" push -q --no-verify origin refs/notes/mr:refs/notes/mr || prc=$?
	rm -f "$scratch/token"
	if [ "$prc" -eq 0 ]; then
		pushed=1
		break
	fi
	git fetch -q --no-tags origin "+refs/notes/mr:refs/notes/keel-origin-mr" || fail "cannot fetch origin's refs/notes/mr"
	git notes --ref=mr merge -q -s ours refs/notes/keel-origin-mr || fail "cannot merge origin's refs/notes/mr"
	git update-ref -d refs/notes/keel-origin-mr
	echo "ci-sign-off: origin's refs/notes/mr moved; merged, push $try failed" >&2
done
[ -n "$pushed" ] || {
	status error "Signed, but the note could not be pushed"
	exit 1
}
note=$(git notes --ref=mr show "$head_sha")
id=$(printf '%s' "$note" | jq -r '.review.id')
# what the approval signed over (E-GHFLOW2.W7), in fixed words: the review's state only when it is
# one keel writes, and whether the roster changed
over=
case $(printf '%s' "$note" | jq -r '.covers.review // ""') in
'') ;;
missing | unreadable | FAIL | stale | WAIVED) over=", over a review that did not pass ($(printf '%s' "$note" | jq -r '.covers.review'))" ;;
*) over=", over a review in an unknown state" ;;
esac
if [ "$(printf '%s' "$note" | jq -r '(.covers.roster // []) | length > 0')" = true ]; then
	over="$over, with a roster change"
fi
status success "Signed by approval (review $id)$over"
