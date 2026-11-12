#!/bin/sh
# Worker file guard (E-WMODE review round 2, owner ruling 2026-11-12 option A, DEC-37): the PreToolUse
# hook scripts/router-worker.sh passes to every worker in --settings, run fail-closed (a failure of this
# script denies the call). The Bash sandbox bounds the shell only; the file tools go through the
# permission system, where auto's classifier, not the owner, judges a call outside the worktree.
#   router-file-guard.sh <worktree> <home>     stdin: Claude Code's PreToolUse JSON
# Output: a deny decision, or nothing (the call goes on to the session's permission mode).
#   Edit, Write, MultiEdit, NotebookEdit: allowed only on a path whose resolved form lies under the
#     resolved worktree, and never on its .git (the link git follows, H2) or its .claude/settings.json
#     and settings.local.json (loaded, hooks and all, by the next session there).
#   Read: denied under <home>/.ssh and <home>/.claude/channels (the sandbox's denyRead for the shell).
#   Grep, Glob: denied when their path is, lies under, or contains one of those two directories.
#   Every other tool passes untouched.
# Resolution: every existing component is resolved (symlinks too: a link in the worktree to a file
# outside it is judged by its target); a path that does not exist yet is judged by its deepest
# existing ancestor plus the rest. Relative paths and "." or ".." components are refused for the
# file tools (Claude Code passes absolute paths); a Grep or Glob path is taken against the session's
# cwd. A file with more than one hard link is not edited (an in-place write would reach the other
# name). Case: APFS is usually case-insensitive and `pwd -P` keeps the caller's spelling, so the deny
# checks compare ASCII-lowercased (tr) and the worktree check compares exactly (a case-variant
# spelling of the worktree is denied: fail closed). Known gaps, waived by the owner 2026-11-12 (E-WMODE
# review round 3; backlog e-router-v2 R31-R37): a Unicode case-fold variant (U+017F for "s") escapes the
# deny checks (R31); tools this guard does not name pass untouched (R34); a path ending in a newline
# loses it to command substitution and is judged without it (R35); the path is resolved before the
# tool opens it (R36).
set -u
[ $# -eq 2 ] || { echo "usage: router-file-guard.sh <worktree> <home>" >&2; exit 2; }
deny() {
  jq -n -c --arg r "worker file guard: $1" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $r}}'
  exit 0
}
nl='
'
lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }
# the resolved form of an absolute path (see above); fails when it cannot be resolved
canon() {
  p=$1 tail= n=0
  while :; do
    if [ -L "$p" ]; then
      n=$((n + 1)); [ "$n" -le 40 ] || return 1
      t=$(readlink "$p") || return 1
      case "$t" in /*) ;; *) t=$(dirname "$p")/$t;; esac
      d=$(cd -P "$(dirname "$t")" 2>/dev/null && pwd -P) || return 1
      b=$(basename "$t")
      case "$b" in .|..) p=$(cd -P "$d/$b" 2>/dev/null && pwd -P) || return 1;; *) p=${d%/}/$b;; esac
    elif [ -e "$p" ]; then
      if [ -d "$p" ]; then p=$(cd -P "$p" 2>/dev/null && pwd -P) || return 1
      else d=$(cd -P "$(dirname "$p")" 2>/dev/null && pwd -P) || return 1; p=${d%/}/$(basename "$p"); fi
      r=${p%/}$tail
      printf '%s\n' "${r:-/}"
      return 0
    else
      [ "$p" != / ] || return 1
      tail=/$(basename "$p")$tail p=$(dirname "$p")
    fi
  done
}
# $1 lies under (or is) directory $2, compared lowercased
within_ci() { a=$(lower "$1") b=$(lower "$2"); b=${b%/}; [ "$a" = "$b" ] || case "$a" in "$b"/*) true;; *) false;; esac; }

wt=$(cd -P "$1" 2>/dev/null && pwd -P) || { echo "router-file-guard: no worktree $1" >&2; exit 2; }
home=$(cd -P "$2" 2>/dev/null && pwd -P) || home=${2%/}
in=$(cat)
tool=$(printf '%s' "$in" | jq -r '.tool_name // ""') || exit 2
case "$tool" in
  Edit|Write|MultiEdit) kind=edit key=file_path;;
  NotebookEdit) kind=edit key=notebook_path;;
  Read) kind=read key=file_path;;
  Grep|Glob) kind=search key=path;;
  *) exit 0;;
esac
p=$(printf '%s' "$in" | jq -r --arg k "$key" '(.tool_input // {})[$k] // "" | if type == "string" then . else error("not a string") end' 2>/dev/null) || deny "$tool: $key is not a string"
case "$p" in *"$nl"*) deny "$tool: the path holds a newline";; esac
if [ "$kind" = search ]; then
  cwd=$(printf '%s' "$in" | jq -r '.cwd // ""') || exit 2
  case "$p" in "") p=$cwd;; /*) ;; *) p=${cwd%/}/$p;; esac
fi
case "$p" in "") deny "$tool: no $key";; /*) ;; *) deny "$tool: $key must be an absolute path ($p)";; esac
while [ "$p" != / ] && [ "${p%/}" != "$p" ]; do p=${p%/}; done
case "/$p/" in */./*|*/../*) deny "$tool: $key holds a . or .. component ($p)";; esac
c=$(canon "$p") || deny "$tool: $p cannot be resolved"

if [ "$kind" = edit ]; then
  case "$c" in "$wt"/*) ;; *) deny "$tool outside the worktree $wt ($p resolves to $c); a worker edits only its own worktree";; esac
  rel=$(lower "${c#"$wt"/}")
  case "$rel" in
    .git|.git/*) deny "$tool of the worktree's .git ($p): git follows it, so a worker may not change it";;
    .claude/settings.json|.claude/settings.local.json) deny "$tool of $p: the next session in this worktree loads it";;
  esac
  if [ -f "$c" ]; then
    links=$(stat -c %h "$c" 2>/dev/null || stat -f %l "$c" 2>/dev/null) || deny "$tool: cannot stat $p"
    [ "$links" -le 1 ] || deny "$tool of $p: it has $links hard links, and an in-place write would reach the others"
  fi
  exit 0
fi
for d in "$home/.ssh" "$home/.claude/channels"; do
  d=$(canon "$d") || deny "$tool: $d cannot be resolved"
  within_ci "$c" "$d" && deny "$tool of $p: $d is not readable to a worker"
  [ "$kind" = search ] && within_ci "$d" "$c" && deny "$tool over $p: it contains $d, which is not readable to a worker"
done
exit 0
