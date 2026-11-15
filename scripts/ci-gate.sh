#!/bin/sh
# The Keel gate's CI jobs (E-GHFLOW.W1), one per argument, run the same on a runner and a laptop.
#   scripts/ci-gate.sh ci|gate|docs
# Input, from the workflow's env (never from a run: line):
#   HEAD_BRANCH  the pushed branch, or the pull request's head branch
#   HEAD_SHA     its commit
#   BASE_BRANCH  optional: the pull request's base; else derived (wu/<e>/<x> -> epic/<e>, epic/<x> -> main)
#   BASE_SHA     optional: the pull request's base commit; else origin/<BASE_BRANCH>
#   KEEL         optional: a keel to use; else one is built from BASE_SHA, never from the head
#   GITHUB_STEP_SUMMARY  optional: where the evidence page is appended (default: stdout)
# The checkout is the repository the job runs in; its remote "origin" holds the branches and notes.
set -eu

die() {
	printf 'ci-gate: %s\n' "$*" >&2
	exit 2
}

usage() {
	die "usage: scripts/ci-gate.sh ci|gate|docs   (env HEAD_BRANCH, HEAD_SHA; optional BASE_BRANCH, BASE_SHA, KEEL)"
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

job=${1:-}
[ $# -eq 1 ] || usage
case $job in ci | gate | docs) ;; *) usage ;; esac

head_branch=${HEAD_BRANCH:-}
head_sha=${HEAD_SHA:-}
base_branch=${BASE_BRANCH:-}
base_sha=${BASE_SHA:-}
branch_ok "$head_branch" || die "HEAD_BRANCH is not a branch name: '$head_branch'"
sha_ok "$head_sha" || die "HEAD_SHA is not a full commit id: '$head_sha'"

case $head_branch in
wu/*/*)
	level=wu
	epic=${head_branch#wu/}
	derived=epic/${epic%%/*}
	;;
epic/?*)
	level=epic
	derived=main
	;;
main)
	level=
	derived=main
	;;
*)
	[ "$job" = ci ] && [ -n "$base_branch" ] && [ -n "$base_sha" ] ||
		die "HEAD_BRANCH '$head_branch' is neither wu/<epic>/<name>, epic/<name> nor main"
	level=
	derived=$base_branch
	;;
esac
[ -n "$base_branch" ] || base_branch=$derived
branch_ok "$base_branch" || die "BASE_BRANCH is not a branch name: '$base_branch'"
if [ "$head_branch" = main ]; then
	[ "$job" = ci ] || die "a push to main has no head and base to gate; only the ci job runs"
	base_sha=$head_sha
fi
if [ -z "$base_sha" ]; then
	base_sha=$(git rev-parse -q --verify "refs/remotes/origin/$base_branch^{commit}") ||
		die "the base branch $base_branch is not on origin - push it first"
fi
sha_ok "$base_sha" || die "BASE_SHA is not a full commit id: '$base_sha'"
git cat-file -e "$head_sha^{commit}" 2>/dev/null || die "HEAD_SHA $head_sha is not in this clone"
git cat-file -e "$base_sha^{commit}" 2>/dev/null || die "BASE_SHA $base_sha is not in this clone"

summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
scratch=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/keel-ci.XXXXXX") || die "cannot make a scratch directory"
[ -n "$scratch" ] && [ -d "$scratch" ] || die "cannot make a scratch directory"
repo=$(pwd)
cleanup() {
	cd "$repo" || return
	git worktree remove --force "$scratch/base" >/dev/null 2>&1 || true
	git worktree remove --force "$scratch/head" >/dev/null 2>&1 || true
	rm -rf "$scratch"
}
trap cleanup EXIT

git worktree add -q --detach "$scratch/base" "$base_sha"
if [ -z "${KEEL:-}" ]; then
	echo "ci-gate: building keel from the base $base_branch at $base_sha" >&2
	CARGO_TARGET_DIR=$scratch/target cargo build --locked --quiet --manifest-path "$scratch/base/Cargo.toml" >&2
	KEEL=$scratch/target/debug/keel
fi
case $KEEL in /*) ;; *) die "KEEL must be an absolute path: '$KEEL'" ;; esac
[ -x "$KEEL" ] || die "no keel at $KEEL"

case $job in
ci)
	echo "ci-gate: keel check on $(git rev-parse HEAD), keel from $base_sha" >&2
	if "$KEEL" check; then
		printf '## Keel check: green\n\nTree of `%s`, keel built from base `%s`.\n' "$(git rev-parse HEAD)" "$base_sha" >>"$summary"
	else
		printf '## Keel check: red\n\nTree of `%s`, keel built from base `%s`. The log names the failing lint or test.\n' "$(git rev-parse HEAD)" "$base_sha" >>"$summary"
		exit 1
	fi
	;;
gate)
	git fetch -q origin '+refs/notes/*:refs/notes/*'
	for pair in "$head_branch $head_sha" "$base_branch $base_sha"; do
		set -- $pair
		if [ "$(git rev-parse -q --verify "refs/heads/$1" || true)" != "$2" ]; then
			git branch -f "$1" "$2" >/dev/null
		fi
	done
	cd "$scratch/base"
	"$KEEL" hooks install >&2
	case $("$KEEL" ci 2>&1 || true) in
	*"keel ci gate"*)
		"$KEEL" ci gate "$level" "$head_branch" --target "$base_branch" --summary "$summary"
		;;
	*)
		printf '## Keel gate: bootstrap\n\nThe base `%s` predates `keel ci gate`, so its plain `keel verify-dod` judged this tip; a missing sign-off turns it red. The log holds its output.\n' "$base_sha" >>"$summary"
		"$KEEL" verify-dod "$level" "$head_branch" --target "$base_branch"
		;;
	esac
	;;
docs)
	git worktree add -q --detach "$scratch/head" "$head_sha"
	from=$(git merge-base "$base_sha" "$head_sha")
	git diff -z --name-only --no-renames --diff-filter=d "$from" "$head_sha" -- ':(glob)docs/**/*.html' >"$scratch/docs"
	if [ ! -s "$scratch/docs" ]; then
		printf '## Keel docs: no changed doc\n' >>"$summary"
		echo "ci-gate: no changed doc under docs/" >&2
		exit 0
	fi
	cd "$scratch/head"
	if xargs -0 "$KEEL" verify-doc <"$scratch/docs"; then
		printf '## Keel docs: green\n\n%s changed docs checked by keel verify-doc.\n' "$(tr -cd '\000' <"$scratch/docs" | wc -c | tr -d ' ')" >>"$summary"
	else
		printf '## Keel docs: red\n\nkeel verify-doc names each red doc in the log.\n' >>"$summary"
		exit 1
	fi
	;;
esac
