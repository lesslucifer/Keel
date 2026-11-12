# Message templates

**Sign-off ask (terminal)**
```
**<Item> is ready for you to sign.** Checked: tip `<full sha>` on `<branch>`, tree clean, keel verify-dod red only on 8·manual-review.
**What you're signing:** <2–5 bullets in user terms>
**Doc only** | **Try it in the real app** (tested helper; restore steps)
**Gate state:** <green> · **Not independently reviewed:** <paths | none>
**Read first:** cd <abs worktree> && open <result report>
**Run in order:**
1. cd <abs> && keel mr confirm wu <tip> --as <id> --target epic/<slug>
2. cd <abs> && git switch epic/<slug> && git merge --no-ff <wu branch>
**Watching:** <probe that detects done> · **Up next:** <one line per queued task>
**What you need to do now:** <this one task>
```

**Decision ask**
```
**<Item> needs one decision.** <context>
| Option | Means | Cost |  — mark **A (my pick)**
**Default if you don't answer:** A.
**What you need to do now:** 1. Reply A or B.
```

**Reminder / "again"**: the current task's ask re-sent whole (read-first,
commands, what they will see, `Watching`); no history, no other tasks.

**Status board**: `| Worker | Doing | You'll get |` · **Your current task:** one,
in full · **Up next:** one line each · **Running without you:** … · closing line
(promise a push only after arming the watcher).

**Push**: `Sign-off ready: <item>` / `Doc only | Try in app` / one line what /
`cd <abs> && <command>` / `Then: <next>`. Plain text, ≤ ~10 lines, same turn as
the terminal message, report the id.

**Independent review prompt** (read-only subagent, strongest model, explicit)
```
Independent READ-ONLY review (very thorough) of <what>, which the automated gate does not bind.
Worktree <abs>, branch <b>, tip <sha>; base <sha>. Do not edit, run <reserved commands: keel mr, merges>, push, tag, or execute
downloaded binaries; git/grep/read and read-only gh api only. Round <n> of at most 5.
Scope: <rounds 1-2: `git diff <base> HEAD -- <paths>` + <files> | rounds 3-5: `git diff <last reviewed sha> HEAD -- <paths>`;
read any file, report only on these changes>.
Rules: <decision docs>. Earlier rounds' findings (fixed, recorded or refuted): <list, or "none">. A finding is a repeat
only when it is surely the same defect in the same code. A repeat of a fixed one is refuted unless you show, at this
tip, that the fix does not hold: name the failing case. A repeat of a recorded or refuted one is refuted only while the
code it judged is unchanged since (its own file and any file its reason names). In any doubt, report it as new.
A missing test for an edge case the code handles correctly, or a test-harness tidy-up that changes no result, is low.
Hunt: (1) exit paths skipping a safety check (2) unsafe/FFI soundness, confinement (3) secret exposure: only in
the step that needs it, never logged, unreachable by third-party code (4) workflow traps: ref injection in run:,
unpinned actions, caches, pipefail, artifact naming (5) tests touching real data/ports/processes; teardown.
Report file:line, severity (blocker|high|medium|low), CONFIRMED vs PLAUSIBLE, concrete failure scenario; a PLAUSIBLE
finding counts at its severity until refuted. List what you checked clean. Concise.
```
