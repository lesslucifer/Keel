# Review rubric — universal (what every finder looks for)

Read by every finder before it sweeps the diff. **Floor, not ceiling**: meet
every item, then judge beyond it — a finding is never invalid merely because no
line here predicted it.

This file ships with the plugin and holds only what is true of any project.
**Project-local** knowledge — the concrete instances of each principle in this
codebase, its layer boundaries, its hot paths, its lint policy — lives in the
file `[review] rubric` names in `.keel/config.toml` (default `.keel/rubric.md`,
tracked in the project, optional). Read it too when it exists; where the two
disagree on a project fact, the project file wins.

## Dimensions — one finder each

| Dimension | When | Question it asks | Typical catch | Not its job |
|---|---|---|---|---|
| **Correctness** | always | What does this change do that its tests don't cover? | edge cases, races, protocol violations, error paths that crash on hostile input | style, taste, formatting |
| **Efficiency** | always | Does the hot path pay avoidable costs? | buffering data designed to stream, per-request allocations and copies, unbounded queues or maps | micro-optimisation outside hot paths |
| **Altitude** | the diff's code paths span **more than one module** (`[review] modules`) | Is the logic at the right layer? | policy leaking into transport, domain rules inside an API adapter — boundaries per the project's architecture doc | re-architecting the system |

**Retired: reuse and simplification.** Lineage (Trawler DEC-50, 198 recorded
rounds: 2,098 candidates, 624 verified, 19 severe code findings): reuse
produced 3 of the 19 severe findings for ~30% of candidate volume;
simplification produced 1 in 198 rounds, and its ground-truth findings all read
as nits. Re-adding either needs new evidence, not a wish for "more coverage".
Duplication of a rule that will drift is still caught — as an **important**
under correctness or altitude.

## Severity — what the verdict reads

A finder proposes; the **verifier's** severity is the one the verdict reads.

| Severity | The test it must pass | Effect |
|---|---|---|
| **blocker** | The code is wrong, unsafe, or breaks a stated contract — a caller or user gets a wrong answer, a crash, a hang, or a silent loss. | Fails the review. |
| **important** | Correct, but carries a cost the project has ruled against: a duplicated rule that will drift, a reimplementation of something the tree already owns, an altitude break, a cost that grows with the data. | Fails the review. |
| **nit** | Neither. Nothing observable changes and nothing drifts if it is never fixed — naming, ordering, a clearer local shape. | Does not fail. Recorded on the cr note with exactly one disposition (`fixed` or `waived`). |

**Uncertainty resolves upward, to important.** `nit` is the narrow exit, not
the default: a finding is one only after it survives *nothing observable changes
and nothing drifts*.

## Principles

| # | Principle | What it means anywhere |
|---|---|---|
| P1 | **Bounded resources** | Nothing accumulates without an explicit bound; data designed to flow is never collected wholesale. No unbounded queues, maps, buffers, or backlogs. |
| P2 | **Execution-context discipline** | Code honours the constraints of the context running it; work that would stall that context moves out of it (no blocking I/O on an event loop, no lock held across an await or a slow call, minimal critical sections). |
| P3 | **Sensitive-data containment** | Data with a secrecy or trust boundary never crosses it as a side effect: keys, tokens and user payloads stay out of logs, error strings and APIs unless redacted. |
| P4 | **Hostile-input resilience** | Anything from outside the process is untrusted wherever it flows; malformed input degrades gracefully — never a crash, never corrupted state. |
| P5 | **Intent legibility** | Names and structure carry intent by themselves; a comment exists only where the *why* cannot live in code. |

The project rubric grows by instances: a recurring verified finding joins an
existing principle as a project instance (rarely, a new principle at the same
altitude), and an instance is deleted the moment it can be mechanised into a
lint.

## Exclusions — never report

- Anything `gate0` already catches: formatting, the configured lint policy,
  failing tests. Such a finding is itself a process bug.
- Pre-existing issues the reviewed diff neither introduces nor worsens.
- Pure taste with no dimension or principle behind it.
- Documents: plans, results and notes are not this review's subject.
