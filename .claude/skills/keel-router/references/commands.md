# Command templates (Keel)

Ported from Trawler's `trawler-router/references/commands.md` (2026-11-06), rewritten
in `keel` terms and re-run on 2026-11-08 against a scratch clone of Keel
(`keel` built from this repo; read-only lines executed, owner lines executed up
to the `.mr-allowed` refusal, merges and pushes executed against a scratch
origin). macOS/zsh-first, like `worker-ops.md`.

Variables: `M` = the directory holding the checkouts · `MAIN=$M/<Repo>` (main
checkout, first row of `git worktree list`) · `EWT` = the checkout holding the
epic branch · `WT` = the WU's worktree · `E` = epic id (`E-ROUTER`) · `W` = WU
slug · `T` = full 40-char tip · `ID` = the owner's `--as` id. Prefixes come from
`.keel/config.toml` `[branches]` (defaults `epic/`, `wu/`; shown below).
Owner commands are printed for the owner, never run by the router or a worker;
every one is `cd <abs> && …` with a full sha.

**`--target` is not optional with a sha.** `keel verify-dod`, `keel preflight`
and `keel mr confirm` infer a WU's epic only from a `wu/<E>/<slug>` *branch
name*; given a sha they measure against `main`. Checked: on a scratch WU tip,
`keel verify-dod wu <sha>` failed `6·doc-note` on the whole epic's docs, and the
same call with `--target epic/<E>` did not. Always pass `--target epic/$E` for a
WU (an epic's default target, main, is right).

## Router: verify a worker's claim (read-only)

Under the router posture a read-only verify worker runs this block and reports
raw output; the router itself runs only the whitelisted lines (marked `[R]`,
one command per call).
```sh
git log -1 --format=%H wu/<E>/<W>          # [R] = the claimed tip (type the values: the whitelist refuses $)
cd $WT && git branch --show-current && git rev-parse HEAD && git status --short | head -3
git -C $WT merge-base --is-ancestor epic/$E $T && echo based-on-epic
for r in test doc cr mr dod; do echo "$r: $(git -C $WT log -1 --notes=$r --format=%N $T 2>/dev/null | head -c 120)"; done
cd $WT && keel verify-dod wu $T --target epic/$E   # before signing: red ONLY on 8·manual-review
cd $WT && keel verify-dod epic $T                  # epic tip, target main
git -C $WT diff --cached --name-only               # before staging keel mr commit: only the rule-bearing doc
git -C $MAIN log --oneline -3 main; git -C $MAIN rev-parse main origin/main; git -C $MAIN tag | tail -3
```
`git log --notes=<ref>` on a ref that does not exist yet prints "notes ref …
is invalid" on stderr: no note, not an error. `keel verify-dod` is read-only on
red and writes the `dod` receipt only on a full green. `keel preflight wu $T
--target epic/$E` (a worker's step, before the review) also runs `keel check`,
so it builds and tests: not a quick probe.

## Owner: sign and merge
```sh
# rule-bearing doc (rule_bearing in config): the worker left only that doc staged
cd $WT && keel mr commit -m $'<E>.<Wn>: <subject>\n\n<body; apostrophes as \'>' --as $ID
# WU: sign, then merge into the epic (in the checkout holding epic/$E; switch is a no-op if already on it)
cd $WT && keel mr confirm wu $T --as $ID --target epic/$E --report docs/work/<date>-<epic>-w<n>-<slug>-result.html
cd $EWT && git switch epic/$E && git merge --no-ff wu/$E/$W
# epic: sign, merge into main, push
cd $EWT && keel mr confirm epic $T --as $ID --report docs/.archive/<date>-<slug>-epic-result.html
cd $MAIN && git merge --no-ff epic/$E
cd $MAIN && git push origin main
```
`--report` is required when more than one doc carries the level's result role
(`data-artifact="wu-result"` / `"epic-result"`), and an epic sign-off passes
`--report <path>` unless its result doc is committed in the epic's diff; always
passing the one the owner read is simplest. Under `[mr] report = "none"`, drop `--report` (it is refused). `git switch epic/$E` fails if
the epic branch is checked out in another worktree: then `EWT` is that worktree.
Without the owner's identity in `.mr-allowed`, `keel mr` refuses before signing
("<id> is not listed in …/.mr-allowed").

## Router: watch each staged owner action

A watch worker runs one per staged action (`run_in_background`, one plain
`until` loop, no inner `&`), and the router says `Watching: …` in the ask.
Probe table and CI / htmldoc loops: `worker-ops.md` "Watch every staged owner
action". Ready-to-run forms:
```sh
until [ -n "$(git -C $WT log -1 --notes=mr --format=%N $T 2>/dev/null)" ]; do sleep 20; done; echo "SIGNED $T"       # keel mr confirm
until [ "$(git -C $WT rev-parse HEAD)" != $T ]; do sleep 20; done; echo COMMITTED                                  # keel mr commit
until git -C $EWT merge-base --is-ancestor $T epic/$E && [ ! -e "$(git -C $EWT rev-parse --path-format=absolute --git-path MERGE_HEAD)" ]; do sleep 20; done; echo MERGED
until git -C $MAIN fetch -q origin main && git -C $MAIN merge-base --is-ancestor $T origin/main; do sleep 30; done; echo PUSHED
```
For an epic, the MERGED probe is `git -C $MAIN merge-base --is-ancestor $T main`.
PUSHED is true only once origin's main contains `$T`, so it is safe to arm when
the action is staged (an equality check against local `main` fires at once, and
one against `$T` never fires: the `--no-ff` merge pushes a new commit).

## Worker: worktree for a parallel WU
```sh
git -C $MAIN worktree add ../<Repo>-<E>-$W -b wu/$E/$W epic/$E
```
The worker creates it (first step of its brief) and never touches another WU's
worktree. Keel's `post-checkout` hook (after `keel hooks install`) copies the
files listed in `.gitworktreecopy` (e.g. the untracked `.mr-allowed`) into it.

## Router: retire a closed worktree (a worker runs it)

Only when merged, clean and no process runs in it; keep the branch (its notes
live on it).
```sh
cd $M && b=$(git -C <Repo>-<slug> branch --show-current) && git -C $MAIN merge-base --is-ancestor $b epic/$E && echo merged   # main for an epic
git -C $M/<Repo>-<slug> status --short | wc -l                                  # 0
ps -axo pid,command | grep '<Repo>-<slug>' | grep -v grep                        # nothing (but the checker's own shell if its command names the path)
mkdir -p $MAIN/docs/.archive && cp -Rn $M/<Repo>-<slug>/docs/.archive/. $MAIN/docs/.archive/   # git-ignored: lost with the worktree otherwise
(cd $M/<Repo>-<slug>/docs/.archive && find . -type f) | while read -r f; do cmp -s "$M/<Repo>-<slug>/docs/.archive/$f" "$MAIN/docs/.archive/$f" || echo "DIFF $f"; done
git -C $MAIN worktree remove $M/<Repo>-<slug> && git -C $MAIN worktree prune
```
`git status --short` does not show git-ignored files and `git worktree remove`
deletes them without `--force` (checked): copy `docs/.archive/` (ignored in Keel)
and any other ignored artifact out first, and compare.
