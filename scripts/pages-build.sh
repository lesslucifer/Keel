#!/bin/sh
# The GitHub Pages site of every live branch's docs (E-GHFLOW2.W8).
#   scripts/pages-build.sh <out-dir>           write the site into <out-dir>, which is absent or empty
#   scripts/pages-build.sh --folder <branch>   print the branch's folder in the site; exit 1 if unpublished
#   scripts/pages-build.sh --path <path>       exit 0 if a tracked file at <path> is published, else 1
# Run in a clone whose remote PAGES_REMOTE (default origin) holds the branches as
# refs/remotes/<remote>/*, as actions/checkout leaves a full-depth clone. The remote is read without
# credentials (git ls-remote and a fetch of refs/notes/report; the workflow keeps none), so the
# repository must be public, as Pages on GitHub Free requires anyway.
#
# The site, rebuilt whole each time, so a deleted branch's folder is gone at the next build:
#   index.html                          every published branch
#   <branch>/index.html                 the branch's docs
#   <branch>/docs/<path>                each published file under the branch tip's docs/
#   <branch>/docs/_report/report.html   an epic tip's report note (refs/notes/report), bytes as stored
# A branch's folder is its name: main, epic/<E> or wu/<E>/<x>, each part in [A-Za-z0-9._-], not
# starting with a dot, at most 200 bytes. Git refuses a branch next to one under it (a and a/b), so no
# two branches share a folder. A file keeps its path, so a page's address is <site>/<branch>/<repo
# path> and links between docs resolve as in the repository.
#
# What is published (the security review of W8, F2 and F3): a tracked regular file under docs/ whose
# path parts follow the branch rule, at most 400 bytes in all, outside docs/_report/ (reserved for the
# note), of a doc's file type: html css png jpg jpeg gif webp svg drawio. The site is served under the
# owner's name, so no script, binary or archive a branch holds is served. Everything else is skipped
# with a log line: a link or submodule, another path or file type, a branch of another shape. Paths
# git prints quoted (a control or non-ASCII byte) fall outside the safe set.
#
# Budgets, so no one branch can stop every deploy: a branch whose published files (and note) cost more
# than PAGES_BRANCH_BUDGET bytes (default 100 MiB) is skipped whole, and so is a branch that would take
# the site past PAGES_SITE_BUDGET (default 1000000000, under Pages' 1 GB). Each file costs its size
# rounded up to 4 KiB, as a disk counts it. Branches are taken main first, then epic/, then wu/.
#
# Git data only: nothing a branch holds is run, sourced or read as configuration. Every file is a
# blob written by git cat-file, never a link: the Pages upload archives with --dereference, so a
# committed symbolic link would publish a runner file.
set -eu
LC_ALL=C
export LC_ALL

die() {
	printf 'pages-build: %s\n' "$*" >&2
	exit 2
}
log() { printf 'pages-build: %s\n' "$*" >&2; }
# a name as printed in a log line: no control character, at most 120 bytes
shown() { printf '%s' "$1" | tr -d '\000-\037\177' | cut -c1-120; }

types='html css png jpg jpeg gif webp svg drawio'
part='[A-Za-z0-9_-][A-Za-z0-9._-]{0,199}'
# safe <name>: only the safe set, no "..", at most 400 bytes (also refuses a newline, so the
# line-wise grep below sees one line)
safe() {
	case $1 in '' | *[!A-Za-z0-9._/-]* | *..*) return 1 ;; esac
	[ ${#1} -le 400 ]
}
# folder <branch>: the branch's folder in the site, or status 1 when it is not published
folder() {
	safe "$1" || return 1
	printf '%s\n' "$1" | grep -Eqx "main|epic/$part|wu/$part/$part" || return 1
	printf '%s\n' "$1"
}
# path_ok <path>: a tracked path the site may hold
path_ok() {
	safe "$1" || return 1
	case $1 in docs/_report | docs/_report/*) return 1 ;; esac
	printf '%s\n' "$1" | grep -Eqx "docs(/$part)+"
}
# type_ok <path>: one of the doc file types
type_ok() {
	for t in $types; do
		case $1 in *."$t") return 0 ;; esac
	done
	return 1
}
# cost <bytes>: the bytes rounded up to 4 KiB
cost() { echo $((($1 + 4095) / 4096 * 4096)); }
digits() { expr "x$1" : 'x[0-9]\{1,15\}$' >/dev/null; }

usage() { die "usage: scripts/pages-build.sh <out-dir> | --folder <branch> | --path <path>"; }

case ${1:-} in
--folder)
	[ $# -eq 2 ] || usage
	folder "$2" || exit 1
	exit 0
	;;
--path)
	[ $# -eq 2 ] || usage
	path_ok "$2" && type_ok "$2" || exit 1
	exit 0
	;;
'' | -*) usage ;;
esac
[ $# -eq 1 ] || usage
out=$1
remote=${PAGES_REMOTE:-origin}
case $remote in '' | -* | *[!A-Za-z0-9._-]*) die "PAGES_REMOTE is not a remote name: '$(shown "$remote")'" ;; esac
branch_budget=${PAGES_BRANCH_BUDGET:-104857600}
site_budget=${PAGES_SITE_BUDGET:-1000000000}
digits "$branch_budget" || die "PAGES_BRANCH_BUDGET is not a number of bytes: '$(shown "$branch_budget")'"
digits "$site_budget" || die "PAGES_SITE_BUDGET is not a number of bytes: '$(shown "$site_budget")'"
git rev-parse -q --git-dir >/dev/null 2>&1 || die "not in a git repository"

if [ -e "$out" ] || [ -L "$out" ]; then
	[ -d "$out" ] && [ ! -L "$out" ] || die "the out dir is not a directory: $(shown "$out")"
	[ -z "$(ls -A "$out")" ] || die "the out dir must be absent or empty, so no old page survives: $(shown "$out")"
fi

tmp=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/pages-build.XXXXXX") || die "cannot make a scratch directory"
[ -n "$tmp" ] && [ -d "$tmp" ] || die "cannot make a scratch directory"
trap 'rm -rf "$tmp"' EXIT

# the report notes, when origin has them; exit 2 from ls-remote means it has none
rc=0
git ls-remote --exit-code "$remote" refs/notes/report >/dev/null 2>&1 || rc=$?
case $rc in
0)
	git fetch -q "$remote" '+refs/notes/report:refs/notes/report'
	notes=refs/notes/report
	;;
2) notes= ;;
*) die "cannot read the remote $remote (git ls-remote exit $rc)" ;;
esac
mkdir -p -- "$out"

tab=$(printf '\t')
# main first, then epic/, then wu/, then the rest (skipped below), so the budget keeps the first
git for-each-ref --format='%(refname)' "refs/remotes/$remote/" |
	awk -v r="refs/remotes/$remote/" '{ b = substr($0, length(r) + 1); k = 3
		if (b == "main") k = 0; else if (b ~ /^epic\//) k = 1; else if (b ~ /^wu\//) k = 2
		print k " " $0 }' | sort | cut -d' ' -f2- >"$tmp/refs"
: >"$tmp/branches"
spent=0
while IFS= read -r ref; do
	b=${ref#"refs/remotes/$remote/"}
	[ "$b" != HEAD ] || continue
	f=$(folder "$b") || {
		log "skipped branch $(shown "$b"): not main, epic/<E> or wu/<E>/<x> in [A-Za-z0-9._-]"
		continue
	}
	tip=$(git rev-parse -q --verify "$ref^{commit}") || {
		log "skipped branch $b: names no commit"
		continue
	}
	# pass 1: choose the files and price the branch; nothing is written yet
	: >"$tmp/files"
	: >"$tmp/blobs"
	price=0
	git ls-tree -r -l --full-tree "$tip" -- docs >"$tmp/tree"
	while IFS= read -r line; do
		meta=${line%%"$tab"*}
		path=${line#*"$tab"}
		# shellcheck disable=SC2086 # mode, type, object id and size, split on blanks
		set -- $meta
		mode=$1 sha=$3 size=$4
		case $mode in
		100644 | 100755) ;;
		*)
			log "skipped $b:$(shown "$path"): mode $mode is not a regular file"
			continue
			;;
		esac
		path_ok "$path" || {
			log "skipped $b:$(shown "$path"): outside the published paths"
			continue
		}
		type_ok "$path" || {
			log "skipped $b:$(shown "$path"): not a published file type ($types)"
			continue
		}
		price=$((price + $(cost "$size")))
		printf '%s\n' "$path" >>"$tmp/files"
		printf '%s %s\n' "$sha" "$path" >>"$tmp/blobs"
	done <"$tmp/tree"
	note=
	case $b in epic/*)
		if [ -n "$notes" ] && note=$(git notes --ref="$notes" list "$tip" 2>/dev/null); then
			price=$((price + $(cost "$(git cat-file -s "$note")")))
		else
			note=
		fi
		;;
	esac
	if [ "$price" -gt "$branch_budget" ]; then
		log "skipped branch $b: its files cost $price bytes, over the branch budget of $branch_budget"
		continue
	fi
	if [ $((spent + price)) -gt "$site_budget" ]; then
		log "skipped branch $b: its $price bytes would take the site past its budget of $site_budget ($spent spent)"
		continue
	fi
	spent=$((spent + price))
	# pass 2: write the chosen blobs
	mkdir -p -- "$out/$f"
	while read -r sha path; do
		case $path in */*) mkdir -p -- "$out/$f/${path%/*}" ;; esac
		git cat-file blob "$sha" >"$out/$f/$path"
	done <"$tmp/blobs"
	if [ -n "$note" ]; then
		mkdir -p -- "$out/$f/docs/_report"
		git cat-file blob "$note" >"$out/$f/docs/_report/report.html"
	fi
	{
		printf '<!DOCTYPE html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n'
		printf '<meta name="viewport" content="width=device-width, initial-scale=1">\n'
		printf '<title>Keel docs: %s</title>\n</head>\n<body>\n<main>\n' "$b"
		printf '<h1>%s</h1>\n<p>Tip <code>%s</code>. <a href="%s">All branches</a></p>\n' "$b" "$tip" "$(printf '%s/' "$f" | sed 's|[^/]*/|../|g')index.html"
		[ -z "$note" ] || printf '<p><a href="docs/_report/report.html">The epic result report (report note)</a></p>\n'
		if [ -s "$tmp/files" ]; then
			printf '<ul>\n'
			while IFS= read -r p; do printf '<li><a href="%s">%s</a></li>\n' "$p" "$p"; done <"$tmp/files"
			printf '</ul>\n'
		else
			printf '<p>No doc on this branch.</p>\n'
		fi
		printf '</main>\n<script type="module" src="https://htmldoc.wfomobile.com/htmldoc.js"></script>\n</body>\n</html>\n'
	} >"$out/$f/index.html"
	n=$(wc -l <"$tmp/files" | tr -d ' ')
	printf '%s %s %s\n' "$f" "$tip" "$n" >>"$tmp/branches"
	log "built $b at $tip: $n files${note:+ and the report note}, $price bytes"
done <"$tmp/refs"

{
	printf '<!DOCTYPE html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n'
	printf '<meta name="viewport" content="width=device-width, initial-scale=1">\n'
	printf '<title>Keel docs</title>\n</head>\n<body>\n<main>\n<h1>Keel docs, by branch</h1>\n'
	if [ -s "$tmp/branches" ]; then
		printf '<ul>\n'
		while read -r f tip n; do printf '<li><a href="%s/index.html">%s</a> <code>%s</code>, %s files</li>\n' "$f" "$f" "$tip" "$n"; done <"$tmp/branches"
		printf '</ul>\n'
	else
		printf '<p>No published branch.</p>\n'
	fi
	printf '</main>\n<script type="module" src="https://htmldoc.wfomobile.com/htmldoc.js"></script>\n</body>\n</html>\n'
} >"$out/index.html"
log "site written: $(wc -l <"$tmp/branches" | tr -d ' ') branches, $spent bytes of files"
