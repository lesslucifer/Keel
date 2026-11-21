# Start, wake and dispatch checklists

The rules run to 400 lines and are read once, at load; the failures they prevent
happen at the first tool call after that. This file is the short form to run at the
moment of action, in order, before a tool call. `SKILL.md` "Session start", "Workers"
and "Talking to the owner" are the long form; this file never replaces them.

## START — before the first tool call of a session (restart, reboot, compaction)

1. **Enforcement.** Is a guard loaded here? Claude Code: the project's `PreToolUse`
   guard and `"agent": "router"`. A host with no hook surface (DSH): say so in your
   first message, in one line, and work through 2-7 by hand (8 closes that
   message). Cost of skipping: everything below is advice with nothing behind it.
2. **Hands off.** Every call: is this on the whitelist? Read tools are for
   dispatching, verifying a claim and relaying, never for deriving the substance.
   A plan or a brief is read once, for the facts a dispatch needs — WU id,
   branch, tip, base, the ruling it is bound by, its hard rules. A fact that is
   not there, or a question about the document's content, goes to the owner or to
   a worker: it is not a reason to read on. A config, a backlog row, the launcher's
   source, a worktree, a notes list are a worker's read or command, and so is every
   command the whitelist does not name; a document a worker was just sent to read
   is not read in parallel with it. Cost: a whole turn spent doing the work
   (2026-11-14: 24 of 32 calls outside the whitelist).
3. **Owner state.** The queue lives in your context and a restart clears it, so
   rebuild it from state: a tip with no notes, a tip with an `mr` note that is not
   merged, an unmerged branch, a watch that has not fired. Name the one current
   task. Cost: a message with no task, or a late one.
4. **Fleet.** `ListAgents` (Claude Code) / `worker_list` (DSH); map each live worker
   to a worktree, check its model and effort against the project's policy, and read
   its last report or transcript tail. Cost: two sessions in one worktree, a worker
   on the wrong model, or one nobody retires.
5. **Briefs and rulings.** Read `<ROOT>/.router/briefs/` and, if it exists,
   `<ROOT>/.router/rulings.md` (one line per ruling: date, what, why). You do not
   create it: your only write is a brief or the owner message body (the `Write` tool), so a ruling you are given goes into the
   next brief and a missing log is a line in your message, for the owner to start.
   A brief whose tip or router name is stale is a finding to fix, never a template.
   Cost: a worker briefed on a dead tip (K14).
6. **Wake.** Name the wake you will arm, and arm it when its subject exists: the
   watch for a staged owner action in the turn you stage it, and the Claude Code
   watchdog once the first background worker is `RUNNING`, never before one
   exists. On DSH the fleet wake is the report channel's watcher where the project
   ships an inbox script, armed before the first dispatch (`worker-ops.md` "The
   report channel (DSH)"); with none, your message says only the owner can wake
   you. Cost: a fleet that finishes into silence, or a watchdog guarding nothing.
7. **Update log.** Where the project ships `scripts/router-app.sh`: at a first start
   or after a restart (never after a compaction: the log survives it), run
   `scripts/router-app.sh start <router-name>`, then write every live WU's stage
   again (`commands.md` "Router: the update log"). Cost: the app shows a dead run,
   or a compaction wipes the sender's question and alert lines.
8. **Close the first message** with `What you need to do now:` naming the one task,
   or `nothing`.

## WAKE — on every wake, before any tool call

A wake is one owner message, one worker's message or report-channel post, one
watch or job line, one idle notice.

1. **What woke me.** Name it and the item it belongs to; do that item, not the board.
   A report-channel post is acked with `consume <id>` once it is recorded.
2. **Fleet.** Read every live worker's output since the last wake, digest it, verify
   its headline claim against git (a tip, a note, a status), then decide: advance,
   steer, stop. Cost: a relayed claim nobody checked.
3. **Armed.** Is the fleet's one wake armed — the Claude Code watchdog, or on DSH
   the report channel's watcher where the project ships one (`worker-ops.md` "The
   report channel (DSH)") — and every staged owner action watched? Arm what is
   missing in this turn; if nothing can be armed, say so in one line. Never close a turn with a live worker and nothing armed. The
   DSH watcher serves every live worker: never one per worker, never a second
   watcher beside it, and never a reminder or a schedule instead. Cost: the owner
   is the only thing that can wake you (2026-11-14: four live workers, no wake).
4. **Denials.** Was any call denied since your last message? Name it with the exact
   command in this message. Never retry it, never hand it to a worker. Cost: a denial
   that stays invisible while a worker is asked to do the same thing (K13).
5. **Close.** One current task in full + `What you need to do now:`; on a re-ask the
   full task again, never "see my earlier message".

## DISPATCH — before every spawn or send

1. **Durable brief.** `Write` `<ROOT>/.router/briefs/<name>.md`, read it back, then
   spawn with that text. On DSH the prompt is the same text and the file is still
   written. Cost: after a restart nothing durable describes the dispatch (K14).
2. **State.** Branch, worktree, full tip, the base it may merge, the rulings, the
   report protocol, the one-run rule, no mid-run questions.
3. **Not twice.** Nobody else is already doing this item; never duplicate a dispatch
   to a live worker.
4. **Count.** One live session per WU; a follow-up round is a message to that session,
   a new WU or a review is a fresh session.
