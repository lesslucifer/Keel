---
name: keel-write
description: How to write any doc a person reads - plans, result reports, use cases, decisions, backlog rows - short, direct and in plain words, problem first and details last. Nine rules, each with a before/after example, which rules apply to which doc, a self-check to run before a doc is marked ready, and a short ready epic plan as the example (references/example.html). Use before writing or editing any such doc. Not for skill files or messages between agents.
---

# Writing docs people can read

A doc has two readers. A person reads it first, with little time. An agent
acts on it later. Write for the person; keep every fact exact for the agent.

This covers every doc a person reads. Skill files and messages between agents
are out.

**The top** of a plan or result report is everything above the work: the
problem, what changes, and any example or terms. In any other doc it is the
first screen. The title may carry the id.

## Which rules apply

- **Every doc:** rules 1, 2, 7, 8 and 9, and rule 3's terms.
- **Plans and result reports:** all nine rules, and the order below.
- **A decision record, use case or backlog row** keeps its own fields. Write
  each field by rules 1–3.
- **A doc you edit:** rewrite only what you touch. Never restructure the rest.

## The rules

### 1 · Short

Cut every word you can. The top fits on one screen.

- Before: "This section provides an overview of the changes this epic will introduce."
- After: "What changes:"

### 2 · Direct

Short sentences. One idea each. No filler.

- Before: "It should be noted that, to keep reviews from running on without end, a limit of three rounds is enforced."
- After: "A review stops after 3 failed rounds."

### 3 · Plain words

No code or ids in the top. Define each term in a Terms table or where you
first use it.

- Before: "W2 adds `0·review-cap` to preflight via `[review] max_rounds`."
- After: "The tool refuses a 4th review round." The check id and the config key go in the work list or under Details.

### 4 · Problem first

Then what changes for the reader. Then the work.

- Before: a plan that opens with "W1 · add `--verdict WAIVED` to the notes command".
- After: a plan that opens with "A review can loop without end. After this epic, the tool stops it at 3 rounds and asks you."

### 5 · Questions, answer first

Each open point is a question. Its answer comes first, then why.

- Before: "Point 3: round limit. Options: (a) 3, (b) 5, (c) configurable."
- After: "How many rounds before you decide? **3**, changeable in config. Fewer would stop good reviews."

### 6 · Details last

Mechanics, tests, evidence and history go at the end, under one heading.

- Before: the test list sits between the problem and the work.
- After: the test list sits under "Details", after the open points.

### 7 · Diagram labels in quotes and plain words

An unquoted `|` or `·` can break a Mermaid diagram.

- Before: `R[preflight wu|epic] --> S[cr PASS]`
- After: `R["Review round"] -->|"PASS"| S["Owner signs"]`

### 8 · Show by example

Explain a flow with a real case, in numbered steps.

- Before: "The waiver is recorded and later checked at sign-off against the signed hash."
- After:
  1. Round 3 fails.
  2. You say "waive it: the finding is cosmetic".
  3. The review is recorded as WAIVED, with your reason.
  4. You accept the waiver and sign off.

### 9 · Check before "ready"

Run the self-check below. Fix what it finds, then mark the doc ready.

- Before: the doc check passes, so the plan is marked ready. Its first screen is a list of files.
- After: the self-check finds the file list. It moves to Details, the problem moves up, and then the plan is marked ready.

## The order of a plan

1. The problem, in two or three sentences.
2. What changes for the reader: a today/after table.
3. A real case in numbered steps, when the flow is new.
4. Terms, for any term not defined where it is first used.
5. The work. For each part: one "In short" of two or three sentences, then a
   list.
6. Open points: questions, answer first.
7. Details: acceptance, tests, scope, evidence, history.

Items 1–4 are the top. A result report keeps the same order: what happened,
what changed, details. Skill `keel-plan` says which tags a plan needs. They
go wherever the content is.

## Self-check

For a plan or result report, read only the top. Can the owner say:

1. the problem?
2. what changes for them?
3. what they must decide, if anything?
4. Is there code or an id in the top? Move it to the work or Details.

For every doc, check what you wrote:

- A sentence with two ideas? Split it.
- A term not defined where first used, nor in Terms?
- A diagram label without quotes?
- A paragraph that could be a list, a table or numbered steps?

## The example

`references/example.html` is a short, ready epic plan. It passes
`keel verify-doc`. Read it before you write your first plan. Start from the
htmldoc skeleton, then copy the example's shape, not its words. The example
has no htmldoc id: your doc gets its own.
