# Router lessons

Source (lineage): a router session on the Trawler project (a Rust + Tauri app run on the same epic → WU → commit workflow Keel packages), 2026-11-05/06, on a weaker model throughout. The incidents keep their concrete details — ports, tools, versions — because they are the evidence behind each rule; the rules themselves are project-agnostic. In Keel, `mr` means `keel mr`. Format: what happened → rule. Add this project's own lessons the same way, ranked by cost.

## Top 10 (ranked by cost of the failures they prevent)
1. Send the push in the same turn as the ask and say "Sent (id N)"; never promise a future message. (A7)
2. Read the real state (git, origin, tags, runs) before every instruction and watch it after: the owner may already have acted; never wait to be told. (A10, B5, B6, C10)
3. Verify third-party behaviour a design depends on in source at the pinned version; PLAUSIBLE is a lead. (B3, B4)
4. Pass the model explicitly on every worker and review; check yours and theirs at start. (B7)
5. One current task per message, ending "What you need to do now:"; self-contained; never point back, even on a re-ask. (A1–A5, A8, A13, A14)
6. Headless workers: fresh per phase, `ls`-checked durable brief, one non-stop run, no mid-run questions, current router name. (C1–C4, C6)
7. Never do the work, never launder a denial: the owner runs `mr`, merges, pushes, tags. (C5, D1, D2)
8. Independent read-only Opus review of everything outside the gate before the owner signs. (B7, G2)
9. Protect the owner's machine: backup/restore, temp homes, one e2e at a time, `lsof` first. (F1–F3)
10. Locate a defect before blocking a sign-off; label OS/tool predictions unverified. (B1, B2)

## A. Talking to the owner
- A1 Asks named worker internals ("the session says…") → rewrite for a reader who sees only the router.
- A2 Commands said "run it in <path>" → every command `cd <abs> && …`, path checked in `git worktree list`.
- A3 Owner had to ask "can I test it or is it just a doc?" → first line: Doc only / Try in the real app.
- A4 Three times "give me that again" → never point to an earlier message.
- A5 "so do I just need to wait?" → closing `What you need to do now:` even when nothing.
- A6 Telegram status was a wall of text ("nobody read it") → header, short blocks, ≤ ~10 lines.
- A7 "I'll message you on Telegram" ~24×; nothing sent 15:34 → 04:18 next day while ~15 asks waited → send in the same turn.
- A8 Push said "sign with the command in my message" → the push carries the command.
- A9 Asked the owner for a run id → find ids, logs, states yourself.
- A10 Staged "merge, push, tag" the owner had already done; assumed merge-before-tag (release.yml builds the tagged commit) → read state; check what the workflow requires.
- A11 Same unanswered gate question for hours → ask twice, state the default, record a follow-up *(inference)*.
- A12 Kept: stating readings ("pic" = packaging epic, "1990" = 1.99.0) and structured questions for mixed replies.
- A13 Several WUs' full command sets in one message; asks sent mid-task were missed ("my attention is limited, one task at a time") → owner queue, one current task in full, `Up next` one line each, next task sent when the watcher fires.
- A14 Re-asks and reminders pointed back, statuses carried other WUs and forwarded worker reports; the owner lost the thread ("give me the detailed information of that task again … I can't remember things from the past, you sent me a ton of messages") → attention is the first principle: every message self-contained for its one task (read-first, cd command, what they will see, how done is detected); re-asks re-send it in full; digest reports, never forward.

## B. Accuracy
- B1 "Don't sign W6" for a bug in W4's code, withdrawn → find the commit/WU holding the defect first.
- B2 Predicted macOS Login Items row would clear; it stays (Apple DTS) → vendor docs first; verify by behaviour.
- B3 Reviewer's PLAUSIBLE "rpm passes 0.1.0-1 unchanged" shaped the version scheme; run v0.1.0-2 failed "invalid version" → verify in source.
- B4 Relayed "Tauri CLI records no version in signatures", withdrawn 7 min later → label findings; hold alarming ones.
- B5 Owner's `open` of a result failed: a worker had switched the worktree's branch → no branch moves under a pending ask.
- B6 Checked a merge while hooks still ran → done = `.git/MERGE_HEAD` gone and tip is the merge.
- B7 22+ workers, 4 reviews and the router ran on Sonnet ("they should use opus 5.5") → explicit `--model`; Opus re-reviews of non-gated work found 4 release defects.
- B8 Said "I can't start sessions" ("Find ways for YOU to do that") → probe the CLI first (`claude -p -n` works).

## C. Workers
- C1 Headless workers asked questions and exited; router claimed they would "go with recommendations" → no mid-run questions; answers in the next brief.
- C2 A worker stopped after its first status note → "ONE non-stop run" in every brief.
- C3 Brief path typo (`-misc/Trawler/` for `-misc-Trawler`) → one `$S`; `ls` every brief.
- C4 Reboot: workers dead, `/tmp` briefs gone, router renamed → durable briefs; re-announce the name; rebuild from git.
- C5 Router opened a doc to address comments ("you ask the worker to check") → relay only.
- C6 Workers ran 4–5 h across WUs → one fresh session per phase; kept a worker alive while its code was uncommitted (right).
- C7 Workers' try helpers broken (syntax error, HTML for a JSON test, helper in another scratchpad) → run once; stable path.
- C8 Router's temp review worktree held the epic branch; back-merge blocked → remove temp worktrees.
- C9 Hotfix landed as a one-epic collapse straight to main; owner wanted an epic with parallel WUs, main only after green → ask or default to that.
- C10 The owner had to say "done" after every sign/merge/push/tag; the router idled until told → watcher per staged action, same turn; 10-minute heartbeat.
- C11 A worker was announced as spawned but never started (`command -v claude` empty) → absolute claude path; announce only after `ps` shows it.

## D. Permissions
- D1 Router merged twice, then "Merge Without Review" was denied → the owner merges; check capability before promising automation.
- D2 Not laundered (keep): cr note copy, `kill 15638`, LaunchAgent write ("Unauthorized Persistence"), running a downloaded actionlint → exact command to the owner.
- D3 Kept: read only the `.pub` key, secrets by name (`gh secret list`), flagged a world-readable key for `chmod 600`; owner generates keys (Q22 b).

## E. Tooling
- E1 `mr commit -m "…\n\n…"` landed a literal `\n` (c9421406) → `$'…'`; rewrite worker-supplied commands.
- E2 `( until …; … ) &` in a background call fired at once → one plain until loop.
- E3 Watcher took "latest completed run on branch" → filter by head sha.
- E4 jq, macOS sed, `rev-parse --short A B`, zsh globs, `head -0`, foreground `sleep` blocked, Monitor expiry → pitfalls table in `worker-ops.md`.

## F. Owner's machine
- F1 Recipe wrote `{"keepCapturing":true}` into the real file; stayed on and broke an e2e quit helper; an e2e spec could delete it → backup/restore; temp homes; assert before deleting.
- F2 Parallel e2e on port 4445 collided; orphan `trawler-ui` (ppid 1) held it; a stray Trawler on 7420 answered a test → one e2e; `lsof`; tests refuse held ports.
- F3 Recipe `chmod a-w` on the real data dir (restore given) → prefer a temp HOME *(inference)*.

## G. CI and release
- G1 Windows CI took 5 outward rounds, one layer each → brief "predict and fix every layer reachable locally".
- G2 Release run 1 died at step 1 (`grep -q` under pipefail, exit 141) after 4 reviews + actionlint → run `run:` steps locally under the runner's flags *(inference)*.
- G3 fail-fast cancelled the other rows in runs 1–2 → `fail-fast: false` until green once.
- G4 Kept: `v0.1.0-1/-2/-3` each a version bump, never a moved tag.

## H. Context
- H1 Two compactions, three names; pushes stopped ~13 h after the second → re-read the communication rules and re-arm watchers after every summary *(inference)*.
