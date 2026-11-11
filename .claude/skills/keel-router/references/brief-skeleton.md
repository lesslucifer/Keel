# Worker brief skeleton

Ported from Trawler's `trawler-router/references/brief-skeleton.md` (2026-11-06);
every line exists because a worker once failed without it (`lessons.md` ids in
the right column below). The router writes the brief itself (SKILL.md
"Substance comes from workers"): a background brief to
`<ROOT>/.router/briefs/<name>.md` (`Read` it back before spawning), an
in-session one as the `Agent` prompt. Commands: `commands.md`.

```markdown
# Brief: <E-CODE.Wn> <job> — fresh session (<model>, effort <level>)
Read first: <previous round's brief or report, abs path>. Router `<current name>` (older names: ignore).
ONE non-stop run; status messages are not stopping points; stop only at your final self-contained report.
No questions mid-run: decide reversible choices with a stated recommendation, or stop and report the options.
## Hard rules
- Never run `keel mr`; stage `cd <worktree> && keel mr confirm|commit … --as <owner id>` and stop.
- Never merge into a branch the owner merges, push, tag, dispatch workflows, `gh secret set`, or read
  signing keys or tokens. Read-only `gh` is fine.
- Never amend/rebase a gated branch; never hand-write or copy a gate note (`refs/notes/*`).
- A permission denial is not routed around: report the exact command for the owner.
- <the project's machine-safety rules from its project-router skill: temp HOME, held ports, real data dirs>
- Follow AGENTS.md and the keel-workflow, keel-plan and keel-distill skills; docs per the project's doc
  convention; no estimates.
## State
Worktree <abs>, branch <b>, tip <full sha>. First: git branch --show-current, git log --oneline -6, git status --short.
Base is <epic/<E> | main> at <sha>; if it moved, merge it in with a merge commit (never rebase).
Owner rulings: <Qn = x, ...>.
## Read first / evidence
<docs, cleaned CI log path, screenshots> · <failing step, error text, run id, file:line; verified vs hypothesis>
## Your job
1. Reproduce as tests red on the old code first (testing charter: WU → integration).
2. Root cause with evidence (third-party source at the pinned version); fix once where all callers route.
3. Audit the same bug class; predict the next CI layer before asking for a run.
4. Gates: keel check · keel notes write test|doc · keel preflight wu <tip> --target epic/<E> ·
   keel-code-review skill (code paths only) → keel notes write cr · keel verify-dod wu <tip> --target epic/<E>
   red only on 8·manual-review · actionlint on changed workflows.
5. Rule-bearing file (`rule_bearing` in .keel/config.toml): leave only it staged, stage
   `keel mr commit -m $'…' --as <id>` for the owner; notes come in a fresh session after.
6. Try-it: a TESTED helper at a stable path (free port, sample data), cd-prefixed run steps,
   backup/restore of any real state it touches.
## Report to <router name> by SendMessage, then end your turn (stay alive for follow-ups)
What is signed · doc (abs path, branch confirmed) · full tip · ordered cd-prefixed owner commands · gate state ·
doc only or how to try · what no gate reviewed (paths outside code_paths) · follow-ups.
```

| Line | Failure it prevents (`lessons.md`) |
|---|---|
| fresh session, explicit model and effort | C6, B7 |
| current router name | C4, H1 |
| ONE non-stop run · no mid-run questions | C2 · C1 |
| never run `keel mr`, merge, push, tag | D1 |
| never copy a gate note · report denials | D2 |
| merge the base in, never rebase | B9 (`review-checklists.md` F) |
| machine-safety rules | F1–F3 |
| tested try-it helper | C7 |
| `$'…'` for `keel mr commit -m` | E1 |

| Variant | Add |
|---|---|
| CI fix round | per-job result, failing step, cleaned log path, hypotheses; "stage push + dispatch" for the owner |
| Step between (after the owner's `keel mr commit`) | new tip D; `keel notes write test\|doc`, the keel-code-review skill on D (never copy the old cr note), `keel preflight`, `keel verify-dod` on D |
| Epic close | merge main in if it moved; distill (keel-distill skill); archive commit (`git rm` every T3 doc, files kept in git-ignored `docs/.archive/`) with `keel notes write doc --distill …`; epic result report in `docs/.archive/`; epic test note (e2e); `keel preflight epic <tip>`; epic-scope review vs main; `keel verify-dod epic <tip>` red only on 8·manual-review; copy `docs/.archive/` to the main checkout; stage `keel mr confirm epic … --report docs/.archive/<…>-epic-result.html` + merge + push |
| Review fix round | findings verbatim with severity; fix vs record; re-gate on the new tip (notes and review are per tip) |
| Parallel WU | create your own worktree (`commands.md`); never touch another WU's worktree |
| Read-only check / watch | no edits, no commits; report raw output; the exact probe and its EVENT line |
