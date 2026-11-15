#!/bin/sh
# The GitHub Pages site of every live branch's docs (E-GHFLOW2.W8).
#   scripts/pages-build.sh <out-dir>           write the site into <out-dir>, which is absent or empty
#   scripts/pages-build.sh --folder <branch>   print the branch's folder in the site; exit 1 if unpublished
# Run in a clone whose remote PAGES_REMOTE (default origin) holds the branches as
# refs/remotes/<remote>/*, as actions/checkout leaves a full-depth clone.
#
# The site, rebuilt whole each time, so a deleted branch's folder is gone at the next build:
#   index.html                          every published branch
#   <branch>/index.html                 the branch's docs
#   <branch>/docs/<path>                each tracked regular file under the branch tip's docs/
#   <branch>/docs/_report/report.html   an epic tip's report note (refs/notes/report), bytes as stored
# A branch's folder is its name: main, epic/<E> or wu/<E>/<x>, each part in [A-Za-z0-9._-] and not
# starting with a dot. Git refuses a branch next to one under it (a and a/b), so no two branches share
# a folder. A file keeps its path, so a page's address is <site>/<branch>/<repo path> and links
# between docs resolve as in the repository.
#
# Git data only: nothing a branch holds is run, sourced or read as configuration. Every file is a
# blob written by git cat-file, never a link: the Pages upload archives with --dereference, so a
# committed symbolic link would publish a runner file. Skipped, each with a log line: a submodule
# or link, a path outside the safe set or with a part starting with a dot, anything under
# docs/_report/ (reserved for the note), a branch of another shape. Paths git prints quoted (a
# control or non-ASCII byte) fall outside the safe set.
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

part='[A-Za-z0-9_-][A-Za-z0-9._-]*'
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
# path_ok <path>: a tracked path the site publishes
path_ok() {
	safe "$1" || return 1
	case $1 in docs/_report | docs/_report/*) return 1 ;; esac
	printf '%s\n' "$1" | grep -Eqx "docs(/$part)+"
}

usage() { die "usage: scripts/pages-build.sh <out-dir> | --folder <branch>"; }

case ${1:-} in
--folder)
	[ $# -eq 2 ] || usage
	folder "$2" || exit 1
	exit 0
	;;
'' | -*) usage ;;
esac
[ $# -eq 1 ] || usage
out=$1
remote=${PAGES_REMOTE:-origin}
case $remote in '' | -* | *[!A-Za-z0-9._-]*) die "PAGES_REMOTE is not a remote name: '$(shown "$remote")'" ;; esac
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
git for-each-ref --format='%(refname)' "refs/remotes/$remote/" >"$tmp/refs"
: >"$tmp/branches"
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
	mkdir -p -- "$out/$f"
	: >"$tmp/files"
	git ls-tree -r --full-tree "$tip" -- docs >"$tmp/tree"
	while IFS= read -r line; do
		meta=${line%%"$tab"*}
		path=${line#*"$tab"}
		mode=${meta%% *}
		sha=${meta##* }
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
		case $path in */*) mkdir -p -- "$out/$f/${path%/*}" ;; esac
		git cat-file blob "$sha" >"$out/$f/$path"
		printf '%s\n' "$path" >>"$tmp/files"
	done <"$tmp/tree"
	report=
	case $b in epic/*)
		if [ -n "$notes" ] && blob=$(git notes --ref="$notes" list "$tip" 2>/dev/null); then
			mkdir -p -- "$out/$f/docs/_report"
			git cat-file blob "$blob" >"$out/$f/docs/_report/report.html"
			report=1
		fi
		;;
	esac
	{
		printf '<!DOCTYPE html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n'
		printf '<meta name="viewport" content="width=device-width, initial-scale=1">\n'
		printf '<title>Keel docs: %s</title>\n</head>\n<body>\n<main>\n' "$b"
		printf '<h1>%s</h1>\n<p>Tip <code>%s</code>. <a href="%s">All branches</a></p>\n' "$b" "$tip" "$(printf '%s/' "$f" | sed 's|[^/]*/|../|g')index.html"
		[ -z "$report" ] || printf '<p><a href="docs/_report/report.html">The epic result report (report note)</a></p>\n'
		if [ -s "$tmp/files" ]; then
			printf '<ul>\n'
			while IFS= read -r p; do printf '<li><a href="%s">%s</a></li>\n' "$p" "$p"; done <"$tmp/files"
			printf '</ul>\n'
		else
			printf '<p>No doc on this branch.</p>\n'
		fi
		printf '</main>\n<script type="module" src="https://htmldoc.wfomobile.com/htmldoc.js"></script>\n</body>\n</html>\n'
	} >"$out/$f/index.html"
	printf '%s %s %s\n' "$f" "$tip" "$(wc -l <"$tmp/files" | tr -d ' ')" >>"$tmp/branches"
	log "built $b at $tip: $(wc -l <"$tmp/files" | tr -d ' ') files${report:+ and the report note}"
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

# GitHub refuses a site over 1 GB; say so here rather than in a failed deploy
kb=$(du -sk "$out" | cut -f1)
[ "$kb" -le 1048576 ] || die "the site is ${kb} KB, over Pages' 1 GB limit"
log "site written: $(wc -l <"$tmp/branches" | tr -d ' ') branches, ${kb} KB"
