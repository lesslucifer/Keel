#!/bin/sh
# The owner's check that GitHub's live settings are the ones E-GHFLOW2.W4 asks for: the rulesets
# that make the merge button the gate, the merge methods, the Actions token and Pages. Read-only: it
# sends GET requests only and changes nothing on GitHub or here.
#   scripts/gh-settings-check.sh [<owner>/<name>]   (default: [github] repo in .keel/config.toml at HEAD)
# Input, from the environment:
#   GH_TOKEN  optional: a token that may read the repository's settings (for example `gh auth token`,
#             or a fine-grained token with Administration: read). Without one GitHub hides a
#             ruleset's bypass list and the Actions settings, and those lines print UNREAD.
#   API, CURL test seams (https://api.github.com and curl by default)
# Output: one line per rule, OK <id> <what>, RED <id> <what>: <what GitHub says>, or UNREAD <id> <what>:
# <why>; BY-HAND lines for what no API call shows; then SETTINGS green|red (<n> red, <m> unread).
# Exit 0 = green; 1 = a rule is red or unread; 2 = bad input. The token reaches curl only on its stdin.
set -euf
LC_ALL=C
export LC_ALL

die() {
	printf 'gh-settings-check: %s\n' "$*" >&2
	exit 2
}

[ $# -le 1 ] || die "usage: scripts/gh-settings-check.sh [<owner>/<name>]"
repo=${1:-}
if [ -z "$repo" ]; then
	repo=$(git show HEAD:.keel/config.toml 2>/dev/null | awk '
		/^[ \t]*\[/ { t = $0; sub(/[ \t]*#.*$/, "", t); on = (t == "[github]"); next }
		on && /^repo[ \t]*=/ { l = $0; sub(/^repo[ \t]*=[ \t]*"/, "", l); sub(/".*$/, "", l); print l; exit }') ||
		repo=
	[ -n "$repo" ] || die "no [github] repo in .keel/config.toml at HEAD: name the repository"
fi
printf '%s\n' "$repo" | grep -Eqx '[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}' || die "not owner/name: '$repo'"
tok=${GH_TOKEN:-}
unset GH_TOKEN
case $tok in *[!!-~]* | *\"* | *\'* | *\\*) die "GH_TOKEN holds a character no GitHub token has" ;; esac
[ ${#tok} -le 4096 ] || die "GH_TOKEN is longer than any GitHub token"
API=${API:-https://api.github.com}
CURL=${CURL:-curl}
case $API in https://*) ;; *) die "API must be an https URL" ;; esac

scratch=$(mktemp -d "${TMPDIR:-/tmp}/gh-settings.XXXXXX") || die "cannot make a scratch directory"
trap 'rm -rf "$scratch"' EXIT
chmod 700 "$scratch"

# get <name> <path>: GitHub's answer to GET <path> as {code, body} in $scratch/<name>.json; the
# request, its Authorization header included, reaches curl only on its stdin
get() {
	rc=0
	code=$({
		printf 'url = "%s%s"\nrequest = "GET"\n' "$API" "$2"
		[ -z "$tok" ] || printf 'header = "Authorization: Bearer %s"\n' "$tok"
		printf 'header = "Accept: application/vnd.github+json"\nheader = "X-GitHub-Api-Version: 2022-11-28"\nheader = "User-Agent: keel-settings-check"\n'
		printf 'output = "%s"\nwrite-out = "%%{http_code}"\nsilent\nproto = "=https"\nconnect-timeout = 15\nmax-time = 60\n' "$scratch/body"
	} | "$CURL" -q --config -) || rc=$?
	printf '%s\n' "$code" | grep -Eqx '[1-5][0-9][0-9]' || code=0
	[ "$rc" -eq 0 ] || code=0
	[ -f "$scratch/body" ] || : >"$scratch/body"
	jq -e . "$scratch/body" >/dev/null 2>&1 || printf 'null' >"$scratch/body"
	jq -c --argjson c "$code" '{code: $c, body: .}' "$scratch/body" >"$scratch/$1.json"
	rm -f "$scratch/body"
}

get repo "/repos/$repo"
get rulesets "/repos/$repo/rulesets?includes_parents=true&per_page=100"
printf '[]' >"$scratch/details.json"
for id in $(jq -r 'select(.code == 200) | .body[]? | .id | select(type == "number") | tostring' "$scratch/rulesets.json"); do
	get one "/repos/$repo/rulesets/$id"
	jq -c -s '.[0] + [.[1]]' "$scratch/details.json" "$scratch/one.json" >"$scratch/all" && mv "$scratch/all" "$scratch/details.json"
done
get token "/repos/$repo/actions/permissions/workflow"
get forks "/repos/$repo/actions/permissions/fork-pr-contributor-approval"
get pages "/repos/$repo/pages"
get env "/repos/$repo/environments/github-pages"
get envb "/repos/$repo/environments/github-pages/deployment-branch-policies"
get owners "/repos/$repo/contents/.github/CODEOWNERS"
get ownerr "/repos/$repo/codeowners/errors"

jq -r -n \
	--slurpfile repo "$scratch/repo.json" --slurpfile list "$scratch/rulesets.json" --slurpfile rs "$scratch/details.json" \
	--slurpfile token "$scratch/token.json" --slurpfile forks "$scratch/forks.json" --slurpfile pages "$scratch/pages.json" \
	--slurpfile env "$scratch/env.json" --slurpfile envb "$scratch/envb.json" \
	--slurpfile owners "$scratch/owners.json" --slurpfile ownerr "$scratch/ownerr.json" '
# The expected list (E-GHFLOW2.W4 plan, "The rules you set"). 15368 is the GitHub Actions app.
def checks: ["keel/ci", "keel/gate", "keel/docs", "keel/sign-off"];
def actions_app: 15368;
def why($a): if $a.code == 0 then "GitHub unreachable" elif $a.code == 404 then "not found (HTTP 404)" else "HTTP \($a.code)" end;
def ok($id; $what): {s: "OK", id: $id, what: $what};
def red($id; $what; $found): {s: "RED", id: $id, what: $what, found: $found};
def unread($id; $what; $found): {s: "UNREAD", id: $id, what: $what, found: $found};
def is($a; $id; $what; f; $shown):
  if $a.code != 200 then unread($id; $what; why($a))
  elif ($a.body | f) then ok($id; $what) else red($id; $what; $shown) end;

$repo[0] as $r | $list[0] as $l | $rs[0] as $rs | $token[0] as $t | $forks[0] as $f | $pages[0] as $p | $env[0] as $e | $envb[0] as $eb |
$owners[0] as $o | $ownerr[0] as $oe |
# GitHub shows the bypass list of a ruleset only to an admin; a read by anyone else may show it
# empty, so an empty list counts only from an admin token (security review)
($r.code == 200 and $r.body.permissions.admin == true) as $admin |
($r.body.owner.login // "") as $owner |
# the active rulesets of a target whose include holds a pattern for $ref and whose exclude is empty
def covering($target; $pats): [$rs[] | select(.code == 200) | .body
  | select(.target == $target and .enforcement == "active")
  | select(any(.conditions.ref_name.include[]?; . as $i | any($pats[]; . == $i)))];
def rule($set; $type): [$set[] | .rules[]? | select(.type == $type)];
def branch_rules($ref; $label; $pats):
  covering("branch"; $pats) as $set |
  if $l.code != 200 then [unread("\($ref).ruleset"; "an active ruleset covers \($label)"; why($l))]
  elif ($set | length) == 0 then [red("\($ref).ruleset"; "an active ruleset covers \($label)"; "none")]
  else
    [ok("\($ref).ruleset"; "an active ruleset covers \($label)")] +
    [ if any($set[]; (.conditions.ref_name.exclude // []) | length > 0)
      then red("\($ref).exclude"; "no ruleset of \($label) excludes a branch"; [$set[] | .conditions.ref_name.exclude[]?] | join(", "))
      else ok("\($ref).exclude"; "no ruleset of \($label) excludes a branch") end ] +
    [ if ($admin | not) or any($set[]; has("bypass_actors") | not)
      then unread("\($ref).bypass"; "nobody bypasses the rules of \($label)"; "only an admin token shows the bypass list: run with GH_TOKEN=$(gh auth token)")
      elif any($set[]; .bypass_actors | length > 0)
      then red("\($ref).bypass"; "nobody bypasses the rules of \($label)"; [$set[] | .bypass_actors[] | "\(.actor_type) \(.actor_id // "") \(.bypass_mode)"] | join(", "))
      else ok("\($ref).bypass"; "nobody bypasses the rules of \($label)") end ] +
    [ rule($set; "pull_request") as $pr |
      if ($pr | length) == 0 then red("\($ref).pull-request"; "a pull request is required, merged as a merge commit only"; "no pull request rule")
      elif all($pr[]; .parameters.allowed_merge_methods == ["merge"]) then ok("\($ref).pull-request"; "a pull request is required, merged as a merge commit only")
      else red("\($ref).pull-request"; "a pull request is required, merged as a merge commit only"; "merge methods " + ([$pr[] | .parameters.allowed_merge_methods // [] | join("+")] | join(", "))) end ] +
    # the approval of the owner as code owner: a check a pull request reports for itself (a workflow
    # its head adds) never merges it alone; a push after the approval dismisses it (security review)
    [ rule($set; "pull_request") as $pr | "a code owner approves, and a push dismisses the approval" as $w |
      if ($pr | length) == 0 then red("\($ref).review"; $w; "no pull request rule")
      elif any($pr[]; .parameters | (.required_approving_review_count // 0) >= 1 and .require_code_owner_review == true and .dismiss_stale_reviews_on_push == true)
      then ok("\($ref).review"; $w)
      else red("\($ref).review"; $w; [$pr[] | .parameters | "approvals \(.required_approving_review_count // 0), code owner \(.require_code_owner_review // false), dismiss stale \(.dismiss_stale_reviews_on_push // false)"] | join("; ")) end ] +
    [ [rule($set; "required_status_checks")[] | .parameters.required_status_checks[]?] as $req |
      checks[] as $c | [$req[] | select(.context == $c)] as $hit |
      if ($hit | length) == 0 then red("\($ref).check.\($c)"; "\($c) is required, from GitHub Actions"; "not required")
      elif all($hit[]; .integration_id == actions_app) then ok("\($ref).check.\($c)"; "\($c) is required, from GitHub Actions")
      else red("\($ref).check.\($c)"; "\($c) is required, from GitHub Actions"; "source " + ([$hit[] | .integration_id // "any" | tostring] | join(", "))) end ] +
    [ if (rule($set; "non_fast_forward") | length) > 0 then ok("\($ref).no-force-push"; "no force push to \($label)")
      else red("\($ref).no-force-push"; "no force push to \($label)"; "allowed") end ] +
    [ if (rule($set; "deletion") | length) > 0 then ok("\($ref).no-deletion"; "\($label) cannot be deleted")
      else red("\($ref).no-deletion"; "\($label) cannot be deleted"; "allowed") end ]
  end;
def tag_rules:
  covering("tag"; ["~ALL", "refs/tags/**", "refs/tags/*"]) as $set |
  if $l.code != 200 then [unread("tags.ruleset"; "an active ruleset covers every tag"; why($l))]
  elif ($set | length) == 0 then [red("tags.ruleset"; "an active ruleset covers every tag"; "none")]
  else
    [ok("tags.ruleset"; "an active ruleset covers every tag")] +
    [ ["creation", "update", "deletion"][] as $k |
      if (rule($set; $k) | length) > 0 then ok("tags.\($k)"; "tag \($k) is restricted")
      else red("tags.\($k)"; "tag \($k) is restricted"; "not restricted") end ] +
    [ if ($admin | not) or any($set[]; has("bypass_actors") | not)
      then unread("tags.bypass"; "only a repository admin (the owner) bypasses the tag rule"; "only an admin token shows the bypass list: run with GH_TOKEN=$(gh auth token)")
      elif all($set[]; [.bypass_actors[] | {actor_type, actor_id}] == [{actor_type: "RepositoryRole", actor_id: 5}])
      then ok("tags.bypass"; "only a repository admin (the owner) bypasses the tag rule")
      else red("tags.bypass"; "only a repository admin (the owner) bypasses the tag rule"; [$set[] | .bypass_actors[] | "\(.actor_type) \(.actor_id // "")"] | join(", ")) end ]
  end;

(
  [ is($r; "merge.commit"; "merge commits are allowed"; .allow_merge_commit == true; "off"),
    is($r; "merge.squash"; "squash merging is off"; .allow_squash_merge == false; "allowed"),
    is($r; "merge.rebase"; "rebase merging is off"; .allow_rebase_merge == false; "allowed"),
    is($r; "merge.auto"; "auto-merge is allowed"; .allow_auto_merge == true; "off") ]
  + branch_rules("main"; "main"; ["~DEFAULT_BRANCH", "refs/heads/main", "~ALL"])
  + branch_rules("epic"; "every epic/ branch"; ["refs/heads/epic/**", "refs/heads/epic/*", "~ALL"])
  + tag_rules
  + [ (if $o.code == 404 then red("codeowners.file"; "CODEOWNERS on the default branch names the owner for every path"; "no .github/CODEOWNERS")
       elif $o.code != 200 then unread("codeowners.file"; "CODEOWNERS on the default branch names the owner for every path"; why($o))
       elif ($o.body.content // "" | gsub("\\s"; "") | @base64d | split("\n") | map(sub("#.*$"; "") | gsub("^\\s+|\\s+$"; "")) | index("* @\($owner)")) != null and $owner != ""
       then ok("codeowners.file"; "CODEOWNERS on the default branch names the owner for every path")
       else red("codeowners.file"; "CODEOWNERS on the default branch names the owner for every path"; "no line * @\($owner)") end),
      (if $oe.code == 404 then red("codeowners.valid"; "GitHub reads CODEOWNERS without an error"; "no CODEOWNERS")
       else is($oe; "codeowners.valid"; "GitHub reads CODEOWNERS without an error"; (.errors // [1]) == []; [$oe.body.errors[]? | .message // "?"] | join("; ")) end) ]
  + [ is($t; "actions.token"; "the workflow token reads by default"; .default_workflow_permissions == "read"; $t.body.default_workflow_permissions // "?"),
      is($t; "actions.approve"; "Actions may not approve pull requests"; .can_approve_pull_request_reviews == false; "may approve"),
      is($f; "actions.forks"; "every outside contributor waits for approval"; .approval_policy == "all_external_contributors"; $f.body.approval_policy // "?"),
      (if $p.code == 404 then red("pages.source"; "Pages builds from GitHub Actions"; "Pages is off")
       else is($p; "pages.source"; "Pages builds from GitHub Actions"; .build_type == "workflow"; $p.body.build_type // "?") end),
      is($e; "pages.env"; "the github-pages environment takes chosen branches only"; .deployment_branch_policy == {protected_branches: false, custom_branch_policies: true}; ($e.body.deployment_branch_policy | tostring)),
      is($eb; "pages.env-branches"; "the chosen branch is main alone"; [.branch_policies[]? | {name, type: (.type // "branch")}] == [{name: "main", type: "branch"}]; ([$eb.body.branch_policies[]? | .name] | join(", "))) ]
) as $all
| ($all[] | if .s == "OK" then "OK \(.id) \(.what)" else "\(.s) \(.id) \(.what): \(.found)" end),
  "BY-HAND actions.pull_request_target Settings > Actions > Policies allows pull_request_target for .github/workflows/keel-gate.yml only (GitHub blocks it by default from 2026-12-02)",
  ([$all[] | select(.s == "RED")] | length) as $red | ([$all[] | select(.s == "UNREAD")] | length) as $unread |
  "SETTINGS \(if $red + $unread == 0 then "green" else "red" end) (\($red) red, \($unread) unread)"
' >"$scratch/out" || die "cannot judge GitHub's answers"
cat "$scratch/out"
tail -n 1 "$scratch/out" | grep -q '^SETTINGS green ' || exit 1
