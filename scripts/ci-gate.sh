#!/bin/sh
# The Keel gate's CI jobs (E-GHFLOW.W1; base-defined since E-GHFLOW2.W4), one per argument, run the
# same on a runner and a laptop.
#   scripts/ci-gate.sh build|ci|gate|docs
# On GitHub, .github/workflows/keel-gate.yml runs this file from main's commit for every pull request
# (pull_request_target), so a pull request never changes the script or the workflow that judges it.
# The checkout is main's; the head and the base are git data, each in its own scratch worktree.
# Input, from the workflow's env (never from a run: line):
#   HEAD_BRANCH  the pull request's head branch, or main for a push of main
#   HEAD_SHA     its commit
#   BASE_BRANCH  optional: the pull request's base; else derived (wu/<e>/<x> -> epic/<e>, epic/<x> -> main);
#                for a wu/ or epic/ head any other base is refused
#   BASE_SHA     optional: the pull request's base commit; else origin/<BASE_BRANCH>
#   REPO, HEAD_REPO  optional: owner/name of this repository and of the head's; when REPO is set, a
#                head from any other repository (a fork) is refused before any git call
#   KEEL         optional: a keel to use; else one is built from BASE_SHA, never from the head
#   KEEL_SHA256  optional: when set, KEEL's SHA-256 must be this (the build job's output); the keel
#                is made executable only after it matches (an artifact keeps no mode bits)
#   KEEL_OUT     build only: the absolute directory the built keel is copied to
#   GITHUB_STEP_SUMMARY  optional: where the evidence page is appended (default: stdout)
#   GITHUB_OUTPUT        optional, build only: where keel_sha256=<hex> is written
# Jobs. build: keel from the base, its hash printed. ci: keel check on the head merged into the base,
# in a scratch worktree (git hooks off); a push of main checks main's tree. gate: keel ci gate, from
# a base worktree, on git data only. docs: keel verify-doc on the docs the range changed, in a head
# worktree. Only ci runs head code (its tests and build scripts); its token is read-only.
# A back-merge (head main, base epic/<E>): gate and docs are green when the head is on origin's main,
# whose every commit passed its own gate; anything else named main is refused.
# The checkout's remote "origin" holds the branches and notes.
set -eu

die() {
	printf 'ci-gate: %s\n' "$*" >&2
	exit 2
}

usage() {
	die "usage: scripts/ci-gate.sh build|ci|gate|docs   (env HEAD_BRANCH, HEAD_SHA; optional BASE_BRANCH, BASE_SHA, REPO, HEAD_REPO, KEEL, KEEL_SHA256, KEEL_OUT)"
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

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum <"$1" | cut -c1-64
	else
		shasum -a 256 <"$1" | cut -c1-64
	fi
}

job=${1:-}
[ $# -eq 1 ] || usage
case $job in build | ci | gate | docs) ;; *) usage ;; esac

if [ -n "${REPO+x}" ]; then
	printf '%s\n' "$REPO" | grep -Eqx '[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}' || die "REPO is not owner/name: '$REPO'"
	[ "${HEAD_REPO:-}" = "$REPO" ] || die "the head is in another repository ('${HEAD_REPO:-}'), not $REPO: a fork's pull request is never gated here, and no job passes it"
fi

head_branch=${HEAD_BRANCH:-}
head_sha=${HEAD_SHA:-}
base_branch=${BASE_BRANCH:-}
base_sha=${BASE_SHA:-}
branch_ok "$head_branch" || die "HEAD_BRANCH is not a branch name: '$head_branch'"
sha_ok "$head_sha" || die "HEAD_SHA is not a full commit id: '$head_sha'"

push_to_main=
backmerge=
[ "$head_branch" = main ] && [ -z "$base_branch$base_sha" ] && push_to_main=1
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
*)
	level=
	derived=main
	case $head_branch:$base_branch in main:epic/?*) backmerge=1 ;; esac
	if [ -z "$push_to_main$backmerge" ]; then
		# any other head name: built and checked against its given base, never gated
		case $job:$base_branch:$base_sha in
		ci:?*:?* | build:?*:?*) ;;
		*) die "HEAD_BRANCH '$head_branch' is neither wu/<epic>/<name>, epic/<name>, main into an epic, nor a push to main" ;;
		esac
	fi
	;;
esac
[ -n "$base_branch" ] || base_branch=$derived
branch_ok "$base_branch" || die "BASE_BRANCH is not a branch name: '$base_branch'"
# a WU merges into its epic and an epic into main, nothing else: a pull request retargeted elsewhere
# (a WU into main) would skip the gate of the level it skips (E-GHFLOW2.W4 security review)
if [ -n "$level" ] && [ "$base_branch" != "$derived" ]; then
	die "$head_branch merges into $derived, not $base_branch: retarget the pull request to $derived"
fi
if [ -n "$push_to_main" ]; then
	case $job in ci | build) ;; *) die "a push to main has no head and base to gate; only the ci job runs" ;; esac
	base_sha=$head_sha
fi
if [ -z "$base_sha" ]; then
	base_sha=$(git rev-parse -q --verify "refs/remotes/origin/$base_branch^{commit}") ||
		die "the base branch $base_branch is not on origin - push it first"
fi
sha_ok "$base_sha" || die "BASE_SHA is not a full commit id: '$base_sha'"
if [ -n "${KEEL_SHA256+x}" ]; then
	printf '%s\n' "$KEEL_SHA256" | grep -Eqx '[0-9a-f]{64}' ||
		die "KEEL_SHA256 is not a SHA-256: '$KEEL_SHA256' (did the build job fail?)"
fi
if [ "$job" = build ]; then
	case ${KEEL_OUT:-} in /*) ;; *) die "KEEL_OUT must be an absolute directory: '${KEEL_OUT:-}'" ;; esac
fi
git cat-file -e "$head_sha^{commit}" 2>/dev/null || die "HEAD_SHA $head_sha is not in this clone"
git cat-file -e "$base_sha^{commit}" 2>/dev/null || die "BASE_SHA $base_sha is not in this clone"

summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
scratch=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/keel-ci.XXXXXX") || die "cannot make a scratch directory"
[ -n "$scratch" ] && [ -d "$scratch" ] || die "cannot make a scratch directory"
repo=$(pwd)
cleanup() {
	cd "$repo" || return
	for w in base head merge; do
		git worktree remove --force "$scratch/$w" >/dev/null 2>&1 || true
	done
	rm -rf "$scratch"
}
trap cleanup EXIT

# a back-merge carries only main's commits, each of which passed its own gate
if [ -n "$backmerge" ] && { [ "$job" = gate ] || [ "$job" = docs ]; }; then
	if git merge-base --is-ancestor "$head_sha" refs/remotes/origin/main 2>/dev/null; then
		printf '## Keel %s: back-merge of main\n\n`%s` is on main, whose every commit passed its gate; nothing new to judge.\n' "$job" "$head_sha" >>"$summary"
		echo "ci-gate: $head_sha is on main: a back-merge into $base_branch, nothing new to judge" >&2
		exit 0
	fi
	printf '## Keel %s: red\n\nThe head `%s` is named main but is not on main.\n' "$job" "$head_sha" >>"$summary"
	echo "ci-gate: the head $head_sha is named main but is not on origin's main: refused" >&2
	exit 1
fi

git worktree add -q --detach "$scratch/base" "$base_sha"
if [ -z "${KEEL:-}" ]; then
	echo "ci-gate: building keel from the base $base_branch at $base_sha" >&2
	(cd "$scratch/base" && CARGO_TARGET_DIR=$scratch/target cargo build --locked --quiet) >&2
	KEEL=$scratch/target/debug/keel
fi
case $KEEL in /*) ;; *) die "KEEL must be an absolute path: '$KEEL'" ;; esac
[ -f "$KEEL" ] || die "no keel at $KEEL"
if [ -n "${KEEL_SHA256+x}" ]; then
	got=$(sha256 "$KEEL")
	[ "$got" = "$KEEL_SHA256" ] || die "the keel at $KEEL has SHA-256 $got, not the build job's $KEEL_SHA256: refused"
	[ -x "$KEEL" ] || chmod u+x "$KEEL"
fi
[ -x "$KEEL" ] || die "no keel at $KEEL"

case $job in
build)
	mkdir -p "$KEEL_OUT"
	cp "$KEEL" "$KEEL_OUT/keel"
	sum=$(sha256 "$KEEL_OUT/keel")
	[ -z "${GITHUB_OUTPUT:-}" ] || printf 'keel_sha256=%s\n' "$sum" >>"$GITHUB_OUTPUT"
	printf 'keel_sha256=%s\n' "$sum"
	printf '## Keel build: keel from base `%s`\n\nSHA-256 `%s`; the other jobs refuse any other.\n' "$base_sha" "$sum" >>"$summary"
	;;
ci)
	if [ -n "$push_to_main" ]; then
		tree=$repo
		what="Tree of main \`$head_sha\`"
	else
		git worktree add -q --detach "$scratch/merge" "$base_sha"
		if ! git -C "$scratch/merge" -c core.hooksPath=/dev/null -c gc.auto=0 -c maintenance.auto=false -c commit.gpgsign=false \
			-c user.name=keel-ci -c user.email=keel-ci@invalid merge -q --no-ff --no-edit "$head_sha" >&2; then
			printf '## Keel check: red\n\nThe head `%s` does not merge cleanly into base `%s`: merge %s into the branch, then push.\n' "$head_sha" "$base_sha" "$base_branch" >>"$summary"
			echo "ci-gate: $head_sha conflicts with $base_branch at $base_sha" >&2
			exit 1
		fi
		tree=$scratch/merge
		what="Head \`$head_sha\` merged into base \`$base_sha\`"
	fi
	echo "ci-gate: keel check on $what, keel from $base_sha" >&2
	if (cd "$tree" && "$KEEL" check); then
		printf '## Keel check: green\n\n%s, keel built from base `%s`.\n' "$what" "$base_sha" >>"$summary"
	else
		printf '## Keel check: red\n\n%s, keel built from base `%s`. The log names the failing lint or test.\n' "$what" "$base_sha" >>"$summary"
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
