# Router guard: the one PreToolUse hook (every tool) behind the router posture
# (E-ROUTER Q2 ruling 2026-11-08; hardened at the E-ROUTER close review).
# Wired fail-closed in .claude/settings.json: jq failing denies the call.
# Sessions whose agent_type is not "router" (workers, their subagents) pass untouched.
# Input: Claude Code's PreToolUse JSON. Output: a deny decision, or nothing (allow).

def deny($why): {hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny",
  permissionDecisionReason: ("router guard: " + $why + ". Delegate anything else to a worker via scripts/router-worker.sh.")}};

# Must equal the tools: line of .claude/agents/router.md (tests/router.rs). No Agent:
# a subagent runs under its own agent_type, outside this guard, with full tools.
def tools: ["Read", "Grep", "Glob", "Write", "Bash", "Skill", "ToolSearch", "TodoWrite", "AskUserQuestion", "SendMessage", "ListAgents"];

# Skills that run inline; a skill that forks a subagent would escape the guard like Agent.
def skills: ["keel-router", "project-router", "keel-workflow", "keel-plan", "keel-distill"];

# Plain words only: no quote, escape, glob, brace, substitution, redirect, separator or
# control character survives this set, so the string judged here is the argv the shell
# builds (a quoted "--out""put=" once slipped past a substring check). "--ou" also
# catches git's abbreviated long options. gh takes only named query flags and bare
# words without ":" - no -R/--repo (also inside a -dR cluster), no URL argument (both
# point gh at any host), no -q/--jq/-t/--template (gh's jq reads the environment), no --web.
# Each flag token must mean to gh (pflag) what it means here: a long name only after "--"
# (gh reads "-label" as -l with value "abel"), a short one only as one letter, and a
# value never starting with "-" (so it can never be read as, or hide, a flag of its own).
def gh_args: "( ((--(json|state|limit|author|base|head|label|search)|-(L|s|A|B|H|l|S))[ =][A-Za-z0-9._,/][A-Za-z0-9._,/-]*|[A-Za-z0-9][A-Za-z0-9._/-]*))*";
def bash_ok:
  test("^[A-Za-z0-9 ._/:=@%,+^~-]*$")
  and (test("--ou") | not)
  and test("^(git (status|log|diff|show)( .*)?|git branch (-v|-vv|-a|-av|-va|-a -v|--all|--all -v)|gh pr (view|list|status)" + gh_args + "|ps (aux|-ef)|scripts/router-worker\\.sh (spawn|resume|wait) [A-Za-z0-9._/ -]+|scripts/router-watch\\.sh (commit|signed|merged|pushed|tagged)( [A-Za-z0-9._][A-Za-z0-9._/-]*)+)$")
  and ((test("^scripts/") and test("\\.\\.")) | not);

if .agent_type != "router" then empty
elif (.tool_name | IN(tools[]) | not) then deny("tool \(.tool_name) is not one of the router's tools (\(tools | join(", ")))")
elif .tool_name == "Bash" then
  (.tool_input.command // "") as $c
  # length first: it bounds the regex work below (an unbounded trim once ran past the hook timeout)
  | if ($c | length) > 1000 or ($c | sub("^ +"; "") | sub(" +$"; "") | bash_ok | not)
    then deny("Bash whitelist: git status/log/diff/show, git branch -v/-a, gh pr view/list/status (no -R), ps aux/-ef, scripts/router-worker.sh spawn|resume|wait, scripts/router-watch.sh commit|signed|merged|pushed|tagged <args not starting with -> (read-only owner-action watch); one command of plain words, no quotes/$/globs/pipes/redirects, no --output")
    else empty end
elif .tool_name == "Write" then
  .cwd as $d | (.tool_input.file_path // "") as $p
  | if ($p | startswith($d + "/.router/briefs/")) and ($p | ltrimstr($d + "/.router/briefs/") | test("^[A-Za-z0-9_-][A-Za-z0-9._-]*\\.md$"))
    then empty else deny("Write only worker briefs: <checkout>/.router/briefs/<name>.md") end
elif .tool_name == "Skill" then
  if (.tool_input.skill // "" | IN(skills[])) then empty else deny("Skill only \(skills | join(", "))") end
# Subscribe only: one idle/exit notice from a worker, never content. A worker is a full-tool session,
# so any text the router sent it would be an instruction outside the brief the launcher snapshots.
# Unknown keys are denied too, so a future content-bearing parameter cannot ride along.
elif .tool_name == "SendMessage" then
  .tool_input as $i
  | if ($i | type) == "object" and ($i.to | type) == "string" and $i.notify_when_idle == true
       and (($i.message // "") == "") and ($i | keys - ["to", "notify_when_idle", "message", "summary"] | length) == 0
    then empty else deny("SendMessage only as a pure subscription: {to, notify_when_idle: true}, no message") end
else empty end
