# Router guard: the one PreToolUse hook (every tool) behind the router posture
# (E-ROUTER Q2 ruling 2026-11-08; hardened at the E-ROUTER close review).
# Wired fail-closed in .claude/settings.json: jq failing denies the call.
# Sessions whose agent_type is not "router" (workers, their subagents) pass untouched.
# Run with `--arg watchdog_to <router-name>` it is the watchdog session's only tool hook
# (E-ROUTER3.W9, DEC-45): CronCreate and a SendMessage to that router pass, nothing else does,
# whatever the agent_type; the project hook passes no argument, so this branch never runs there.
# Input: Claude Code's PreToolUse JSON. Output: a deny decision, or nothing (allow).

def deny($why): {hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny",
  permissionDecisionReason: ("router guard: " + $why + ". Delegate anything else to a worker via scripts/router-worker.sh.")}};

# Must equal the tools: line of .claude/agents/router.md (tests/suite/router.rs). No Agent:
# a subagent runs under its own agent_type, outside this guard, with full tools.
def tools: ["Read", "Grep", "Glob", "Write", "Bash", "Skill", "ToolSearch", "TodoWrite", "AskUserQuestion", "SendMessage", "ListAgents"];

# Skills that run inline; a skill that forks a subagent would escape the guard like Agent.
def skills: ["keel-router", "project-router", "keel-workflow", "keel-plan", "keel-distill"];

# The owner sender (E-RGUARD.W2): one plain shape per mode, so a quoted body cannot smuggle a
# command; the body itself travels through <checkout>/.router/notify/owner.md, which Write allows.
# Plain words only: no quote, escape, glob, brace, substitution, redirect, separator or
# control character survives this set (jq's "$" also matches before one final newline,
# which the shell reads as the end of the one command), so the string judged here is the
# argv the shell builds (a quoted "--out""put=" once slipped past a substring check). "--ou" also
# catches git's abbreviated long options. gh takes only named query flags and bare
# words without ":" - no -R/--repo (also inside a -dR cluster), no URL argument (both
# point gh at any host), no -q/--jq/-t/--template (gh's jq reads the environment), no --web.
# Each flag token must mean to gh (pflag) what it means here: a long name only after "--"
# (gh reads "-label" as -l with value "abel"), a short one only as one letter, and a
# value never starting with "-" (so it can never be read as, or hide, a flag of its own).
def gh_args: "( ((--(json|state|limit|author|base|head|label|search)|-(L|s|A|B|H|l|S))[ =][A-Za-z0-9._,/][A-Za-z0-9._,/-]*|[A-Za-z0-9][A-Za-z0-9._/-]*))*";
# The update log (E-APPLIST.W1, K14): the router's three shapes of scripts/router-app.sh, the words
# that script takes (tests/suite/router.rs ties the two). set and alert carry free JSON, which no
# plain-word guard can admit (the inbox's post precedent): only the watchdog and the sender run them.
def app_shapes: "scripts/router-app\\.sh (start [a-z0-9][a-z0-9-]{0,31}|stage E-[A-Z0-9]{1,16}\\.W[0-9]{1,3} (plan-in-work|plan-to-read|plan-approved|building|in-review)( [0-9a-f]{7,40})?|del (question/[a-z0-9][a-z0-9-]{0,40}|wu/E-[A-Z0-9]{1,16}\\.W[0-9]{1,3}/stage))";
def bash_ok:
  test("^[A-Za-z0-9 ._/:=@%,+^~-]*$")
  and (test("--ou") | not)
  and test("^(git (status|log|diff|show)( .*)?|git branch (-v|-vv|-a|-av|-va|-a -v|--all|--all -v)|gh pr (view|list|status)" + gh_args + "|ps (aux|-ef)|scripts/router-worker\\.sh (spawn|stop) [A-Za-z0-9._/ -]+|scripts/router-worker\\.sh worktree (add|remove) (epic|wu)/[A-Za-z0-9._/-]+|scripts/router-worker\\.sh watchdog (start|stop) [a-z0-9][a-z0-9-]{0,31}|scripts/router-watch\\.sh (commit|signed|merged|pushed|tagged)( [.]*[A-Za-z0-9_][A-Za-z0-9._/-]*)+|scripts/notify-owner\\.py (--check|--send|--dry-run)|scripts/router-inbox\\.sh (init|status|watch [0-9]{1,7}|consume [A-Za-z0-9][A-Za-z0-9._-]*)|" + app_shapes + ")$")
  and ((test("^scripts/") and test("\\.\\.")) | not);

if $ARGS.named.watchdog_to != null then
  $ARGS.named.watchdog_to as $to
  # only the tick's shape is admitted (one call judged at a time): a repeat of that same call passes
  | if $to != "" and ((.tool_name == "CronCreate" and (.tool_input | type) == "object"
        and .tool_input.cron == "*/5 * * * *" and .tool_input.prompt == "tick" and .tool_input.recurring == true)
      or (.tool_name == "SendMessage" and (.tool_input | type) == "object" and .tool_input.to == $to))
    then empty
    else {hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny",
      permissionDecisionReason: "watchdog guard: only CronCreate of the */5 tick and a SendMessage to \($to) are allowed"}} end
elif .agent_type != "router" then empty
elif (.tool_name | IN(tools[]) | not) then deny("tool \(.tool_name) is not one of the router's tools (\(tools | join(", ")))")
elif .tool_name == "Bash" then
  (.tool_input.command // "") as $c
  # length first: it bounds the regex work below (an unbounded trim once ran past the hook timeout)
  | if ($c | length) > 1000 or ($c | sub("^ +"; "") | sub(" +$"; "") | bash_ok | not)
    then deny("Bash whitelist: git status/log/diff/show, git branch -v/-a, gh pr view/list/status (no -R), ps aux/-ef, scripts/router-worker.sh spawn|stop, scripts/router-worker.sh worktree add|remove <epic/E or wu/E/slug>, scripts/router-worker.sh watchdog start|stop <router-name>, scripts/router-watch.sh commit|signed|merged|pushed|tagged <args not starting with -> (read-only owner-action watch), scripts/notify-owner.py --send|--dry-run|--check (the owner message body lives in .router/notify/owner.md), scripts/router-inbox.sh init|status|watch <seconds>|consume <id> (the worker report channel; the router never posts), scripts/router-app.sh start <name>|stage <wu> <stage> [<tip>]|del question/<id>|del wu/<wu>/stage (the update log); one command of plain words, no quotes/$/globs/pipes/redirects, no --output")
    else empty end
elif .tool_name == "Write" then
  .cwd as $d | (.tool_input.file_path // "") as $p
  | if (($p | startswith($d + "/.router/briefs/")) and ($p | ltrimstr($d + "/.router/briefs/") | test("^[A-Za-z0-9_-][A-Za-z0-9._-]*\\.md$")))
      or ($p == $d + "/.router/notify/owner.md")
    then empty else deny("Write only worker briefs <checkout>/.router/briefs/<name>.md and the owner body <checkout>/.router/notify/owner.md") end
elif .tool_name == "Skill" then
  if (.tool_input.skill // "" | IN(skills[])) then empty else deny("Skill only \(skills | join(", "))") end
# Any message to a named worker (DEC-42): an idle subscription or content that steers a live worker.
# The harness adds keys of its own (type, recipient, content), so only the addressee is judged.
elif .tool_name == "SendMessage" then
  if (.tool_input | type) == "object" and (.tool_input.to | type) == "string" and .tool_input.to != ""
    then empty else deny("SendMessage needs a named recipient: to must be a non-empty string") end
else empty end
