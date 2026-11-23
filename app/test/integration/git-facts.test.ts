// The git reader at its real boundary: real git in a temp keel project (main with an upstream, an
// epic, work units, a task, notes, a worktree), read through the fixed commands only.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import { GitReader, NOTE_TIPS, TIME_TIPS } from '../../src/main/reader/git.ts';
import { OUTPUT_CAP, ProjectRunner } from '../../src/main/commands.ts';
import { tempDir, realGit, sh, snapshot, mk, oldGit, records, program } from '../support/fs.ts';

const GIT = realGit();
const opts = (dir: string) => ({ path: [path.dirname(GIT), '/usr/bin', '/bin'], source: { HOME: dir, LANG: 'C' } });
const rev = (dir: string, ref: string) => sh(GIT, dir, ['rev-parse', ref]).trim();

/** A keel project: main pushed to a bare upstream then one ahead; epic/E-A holding wu/E-A/w1 (merged, --no-ff); wu/E-A/w2 with a task not merged; notes; a worktree. */
function world(t: { after: (fn: () => void) => void }) {
  const root = tempDir(t, 'git-facts');
  const dir = mk(path.join(root, 'proj'));
  mkdirSync(path.join(dir, '.keel'));
  writeFileSync(path.join(dir, '.keel', 'config.toml'), 'code_paths = ["src/"]\n');
  sh(GIT, dir, ['init', '-q']);
  sh(GIT, dir, ['add', '.']);
  sh(GIT, dir, ['commit', '-q', '-m', 'init']);
  sh(GIT, root, ['init', '-q', '--bare', 'up.git']);
  sh(GIT, dir, ['remote', 'add', 'origin', path.join(root, 'up.git')]);
  sh(GIT, dir, ['push', '-q', '-u', 'origin', 'main']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'ahead']);
  sh(GIT, dir, ['branch', 'epic/E-A']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w1', 'epic/E-A']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'w1']);
  sh(GIT, dir, ['switch', '-q', 'epic/E-A']);
  sh(GIT, dir, ['merge', '-q', '--no-ff', '-m', 'merge w1', 'wu/E-A/w1']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w2', 'epic/E-A']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'w2']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w2_probe', 'wu/E-A/w2']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'task']);
  sh(GIT, dir, ['switch', '-q', 'main']);
  sh(GIT, dir, ['notes', '--ref=mr', 'add', '-m', '{"schema":"mr/v1"}', 'wu/E-A/w1']);
  sh(GIT, dir, ['notes', '--ref=cr', 'add', '-m', '{"schema":"cr/v1"}', 'wu/E-A/w2']);
  sh(GIT, dir, ['notes', '--ref=test', 'add', '-m', '{}', 'wu/E-A/w2']);
  sh(GIT, dir, ['worktree', 'add', '-q', path.join(root, 'wt-w2'), 'wu/E-A/w2']);
  return { root, dir };
}

test('the reader gives the branches with their upstreams, the sign-off and review notes on tips, what each target holds, and the worktrees; it writes nothing', async (t) => {
  const { root, dir } = world(t);
  const before = snapshot(dir);
  const facts = await new GitReader(dir, opts(root)).read(GIT);
  assert.equal(facts.problem, null);
  const byName = Object.fromEntries(facts.branches.map((b) => [b.name, b]));
  assert.deepEqual(Object.keys(byName).sort(), ['epic/E-A', 'main', 'wu/E-A/w1', 'wu/E-A/w2', 'wu/E-A/w2_probe']);
  assert.deepEqual(byName.main, { name: 'main', tip: rev(dir, 'main'), upstream: 'origin/main', ahead: 1, behind: 0, gone: false, remote: 'origin' });
  assert.deepEqual(byName['epic/E-A'], { name: 'epic/E-A', tip: rev(dir, 'epic/E-A'), upstream: null, ahead: 0, behind: 0, gone: false });
  // wu/E-A/w1 is merged into its epic: its item has left, so its notes are not read. wu/E-A/w2 is open.
  assert.deepEqual(facts.notes, { mr: [], cr: [rev(dir, 'wu/E-A/w2')], dod: [] });
  assert.deepEqual(facts.merged, { main: [], 'epic/E-A': ['wu/E-A/w1'], 'wu/E-A/w2': [] });
  assert.deepEqual(facts.worktrees, [
    { path: dir, branch: 'main', tip: rev(dir, 'main') },
    { path: path.join(root, 'wt-w2'), branch: 'wu/E-A/w2', tip: rev(dir, 'wu/E-A/w2') },
  ]);
  assert.deepEqual(snapshot(dir), before);
});

test('a merge, a sign-off note and a task merged into its work unit each show at the next read', async (t) => {
  const { root, dir } = world(t);
  const reader = new GitReader(dir, opts(root));
  await reader.read(GIT);
  sh(GIT, dir, ['notes', '--ref=mr', 'add', '-m', '{}', 'wu/E-A/w2']);
  sh(GIT, path.join(root, 'wt-w2'), ['merge', '-q', '--no-ff', '-m', 'task in', 'wu/E-A/w2_probe']);
  const facts = await reader.read(GIT);
  assert.deepEqual(facts.merged['wu/E-A/w2'], ['wu/E-A/w2_probe']);
  // The work unit's tip moved past its note (the merge of its task): the note stays on the old tip, not on the branch.
  assert.deepEqual(facts.notes.mr, []);
  sh(GIT, dir, ['notes', '--ref=mr', 'add', '-m', '{}', 'wu/E-A/w2']);
  assert.deepEqual((await reader.read(GIT)).notes.mr, [rev(dir, 'wu/E-A/w2')], 'a sign-off note on an open work unit\'s tip shows');
  sh(GIT, dir, ['push', '-q', 'origin', 'main']);
  assert.deepEqual((await reader.read(GIT)).branches.find((b) => b.name === 'main')?.ahead, 0);
});

test('a project with no notes and no other branch reads as main alone', async (t) => {
  const dir = mk(path.join(tempDir(t, 'git-bare'), 'p'));
  sh(GIT, dir, ['init', '-q']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'one']);
  const facts = await new GitReader(dir, opts(dir)).read(GIT);
  assert.deepEqual([facts.problem, facts.branches.map((b) => b.name), facts.notes, facts.merged], [null, ['main'], { mr: [], cr: [], dod: [] }, { main: [] }]);
});

test('a git older than 2.45 reads a project that is not a partial clone without --no-lazy-fetch, after one config read', async (t) => {
  const { root, dir } = world(t);
  const rec = path.join(root, 'old.jsonl');
  const old = oldGit(path.join(mk(path.join(root, 'old')), 'git'), rec, GIT);
  const reader = new GitReader(dir, opts(root));
  const facts = await reader.read(old);
  assert.equal(facts.problem, null);
  assert.equal(facts.branches.length, 5);
  const runs = records(rec).map((r) => r.argv);
  assert.ok(runs.some((a) => a.includes('--get-regexp')), 'the promisor read ran');
  assert.ok(runs.filter((a) => a.includes('for-each-ref')).slice(1).every((a) => !a.includes('--no-lazy-fetch')));
  const n = records(rec).length;
  await reader.read(old);
  assert.ok(records(rec).slice(n).every((r) => !r.argv.includes('--no-lazy-fetch') && !r.argv.includes('--get-regexp')), 'the second read remembers the old git');
});

test('a git that fails says why and reads nothing; the next read with a good git reads again', async (t) => {
  const { root, dir } = world(t);
  const bad = program(path.join(mk(path.join(root, 'bad')), 'git'), '#!/bin/sh\necho "fatal: broken" >&2\nexit 128\n');
  const reader = new GitReader(dir, opts(root));
  const facts = await reader.read(bad);
  assert.deepEqual([facts.branches, facts.worktrees], [[], []]);
  assert.match(facts.problem ?? '', /git/);
  assert.equal((await reader.read(GIT)).problem, null);
});

test('a git answer cut at the output cap is a problem, never read as whole: no branch, merge or note from half an answer', async (t) => {
  const { root, dir } = world(t);
  const row = `refs/heads/wu/E-A/w${'x'.repeat(60)}\u0000${'a'.repeat(40)}\u0000\u0000\n`;
  const rows = path.join(root, 'rows.txt');
  writeFileSync(rows, row.repeat(Math.ceil((OUTPUT_CAP + 1) / Buffer.byteLength(row))));
  const big = program(path.join(mk(path.join(root, 'big')), 'git'), `#!/bin/sh\ncat ${rows}\n`);
  const facts = await new GitReader(dir, opts(root)).read(big);
  assert.deepEqual(facts.branches, []);
  assert.match(facts.problem ?? '', /cut/);
});

test('the notes read is bounded by the open branches, not by history: many old notes add no git run and no answer', async (t) => {
  const { root, dir } = world(t);
  const rec = path.join(root, 'git-runs.txt');
  const counting = program(path.join(mk(path.join(root, 'counting')), 'git'), `#!/bin/sh\necho "$*" >> ${rec}\nexec ${GIT} "$@"\n`);
  const reader = new GitReader(dir, opts(root));
  const before = await reader.read(counting);
  const notesRuns = () => readFileSync(rec, 'utf8').split('\n').filter((l) => l.includes(' notes ')).length;
  const first = notesRuns();
  for (let i = 0; i < 30; i++) {
    sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', `old ${i}`]);
    sh(GIT, dir, ['notes', '--ref=cr', 'add', '-m', '{}', 'HEAD']);
    sh(GIT, dir, ['notes', '--ref=mr', 'add', '-m', '{}', 'HEAD']);
  }
  sh(GIT, dir, ['reset', '-q', '--hard', 'HEAD~30']);
  const after = await reader.read(counting);
  assert.equal(notesRuns() - first, first, 'the same number of notes runs as before the 60 old notes');
  assert.deepEqual(after.notes, before.notes);
});

test('closed history costs no read: an epic in main gets no merged read, and its kept work units and tasks no notes or task read (review round 2, git.ts:95 and :159)', async (t) => {
  const { root, dir } = world(t);
  sh(GIT, dir, ['branch', 'epic/E-OLD', 'main']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-OLD/w1', 'epic/E-OLD']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'merged unit']);
  sh(GIT, dir, ['switch', '-q', 'epic/E-OLD']);
  sh(GIT, dir, ['merge', '-q', '--no-ff', '-m', 'w1 in', 'wu/E-OLD/w1']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-OLD/w9', 'epic/E-OLD']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'kept, never merged']);
  sh(GIT, dir, ['branch', 'wu/E-OLD/w9_probe']);
  sh(GIT, dir, ['notes', '--ref=cr', 'add', '-m', '{}', 'wu/E-OLD/w9']);
  sh(GIT, dir, ['switch', '-q', 'epic/E-OLD']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'epic work']);
  sh(GIT, dir, ['switch', '-q', 'main']);
  sh(GIT, dir, ['merge', '-q', '--no-ff', '-m', 'close E-OLD', 'epic/E-OLD']);
  sh(GIT, dir, ['branch', 'wu/E-GONE/w1', 'main']);
  const rec = path.join(root, 'runs.txt');
  const counting = program(path.join(mk(path.join(root, 'counting')), 'git'), `#!/bin/sh\necho "$*" >> ${rec}\nexec ${GIT} "$@"\n`);
  const facts = await new GitReader(dir, opts(root)).read(counting);
  assert.equal(facts.problem, null);
  assert.deepEqual(Object.keys(facts.merged).sort(), ['epic/E-A', 'main', 'wu/E-A/w2']);
  assert.deepEqual(facts.merged.main, ['epic/E-OLD'], 'the view publishes the epics main holds');
  assert.deepEqual(facts.notes.cr, [rev(dir, 'wu/E-A/w2')], 'the kept work unit of a closed epic has its note unread');
  const runs = readFileSync(rec, 'utf8');
  assert.doesNotMatch(runs, /--merged=refs\/heads\/(epic\/E-OLD|wu\/E-OLD)/);
  assert.doesNotMatch(runs, new RegExp(`list ${rev(dir, 'wu/E-OLD/w9')}`));
  assert.doesNotMatch(runs, new RegExp(`list ${rev(dir, 'wu/E-GONE/w1')}`), 'a work unit whose epic branch is gone is not open');
  assert.doesNotMatch(runs, / name-rev /, 'a closed epic with a merged unit costs no name-rev, though it keeps a unit outside main (review round 4, git.ts:196)');
});

test('a fresh epic cut at main (no commit of its own yet) is open while main moves on: its open work unit\'s notes are read, then once (review round 3, git.ts:79)', async (t) => {
  const { root, dir } = world(t);
  sh(GIT, dir, ['branch', 'epic/E-NEW', 'main']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-NEW/plan', 'epic/E-NEW']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'the plan']);
  sh(GIT, dir, ['notes', '--ref=cr', 'add', '-m', '{}', 'wu/E-NEW/plan']);
  sh(GIT, dir, ['switch', '-q', 'main']);
  const rec = path.join(root, 'runs.txt');
  const counting = program(path.join(mk(path.join(root, 'counting')), 'git'), `#!/bin/sh\necho "$*" >> ${rec}\nexec ${GIT} "$@"\n`);
  const reader = new GitReader(dir, opts(root));
  let facts = await reader.read(counting);
  assert.equal(facts.problem, null);
  assert.ok(facts.notes.cr.includes(rev(dir, 'wu/E-NEW/plan')), 'the plan unit is open: its review note is read');
  assert.deepEqual(facts.merged['epic/E-NEW'], [], 'the fresh epic gets its merged read');
  // main moves on (another epic merged): the fresh epic's tip is now behind main, still on its own line.
  sh(GIT, dir, ['merge', '-q', '--no-ff', '-m', 'merge E-A', 'epic/E-A']);
  facts = await reader.read(counting);
  assert.ok(facts.notes.cr.includes(rev(dir, 'wu/E-NEW/plan')), 'still open behind main');
  assert.ok(!('epic/E-A' in facts.merged), 'a merged epic with its own commits is closed');
  const names = readFileSync(rec, 'utf8').split('\n').filter((l) => l.includes(' name-rev '));
  const third = (await reader.read(counting), readFileSync(rec, 'utf8').split('\n').filter((l) => l.includes(' name-rev ')));
  assert.equal(third.length, names.length, 'an epic tip\'s answer is asked once: the next read spawns no name-rev');
});

test('when main\'s merged read fails, the next good read asks no name-rev again for tips it already knew', async (t) => {
  const { root, dir } = world(t);
  sh(GIT, dir, ['branch', 'epic/E-NEW', 'main']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-NEW/plan', 'epic/E-NEW']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'the plan']);
  sh(GIT, dir, ['switch', '-q', 'main']);
  const rec = path.join(root, 'runs.txt');
  const flag = path.join(root, 'fail-main');
  const flaky = program(path.join(mk(path.join(root, 'flaky')), 'git'), `#!/bin/sh\necho "$*" >> ${rec}\ncase "$*" in *--merged=refs/heads/main*) [ -f ${flag} ] && exit 128;; esac\nexec ${GIT} "$@"\n`);
  const reader = new GitReader(dir, opts(root));
  await reader.read(flaky);
  const count = () => readFileSync(rec, 'utf8').split('\n').filter((l) => l.includes(' name-rev ')).length;
  const asked = count();
  assert.equal(asked, 1);
  writeFileSync(flag, '');
  // A new branch, so main's merged read is asked again: its answer is kept only while no tip it reads moves (epic review round 1, git.ts:315).
  sh(GIT, dir, ['branch', 'wu/E-A/w3', 'epic/E-A']);
  assert.match((await reader.read(flaky)).problem ?? '', /main/);
  rmSync(flag);
  await reader.read(flaky);
  assert.equal(count(), asked, 'the failed read kept what was known');
});

test('W6: each open tip\'s review note is read for its verdict, again when the note is rewritten on the same tip; the list\'s tips carry their commit times', async (t) => {
  const { root, dir } = world(t);
  const reader = new GitReader(dir, opts(root));
  const w2 = rev(dir, 'wu/E-A/w2');
  sh(GIT, dir, ['notes', '--ref=cr', 'add', '-f', '-m', '{"schema":"cr/v1","verdict":"FAIL","blockers":2,"importants":1}', 'wu/E-A/w2']);
  let facts = await reader.read(GIT);
  assert.deepEqual(facts.reviews, { [w2]: { verdict: 'FAIL', blockers: 2, importants: 1 } });
  // The sign-off's waiver rewrites the review note on the same tip: read again, by the note's new id.
  sh(GIT, dir, ['notes', '--ref=cr', 'add', '-f', '-m', '{"schema":"cr/v1","verdict":"WAIVED","waiver":"cap","blockers":2,"importants":1}', 'wu/E-A/w2']);
  facts = await reader.read(GIT);
  assert.deepEqual(facts.reviews, { [w2]: { verdict: 'WAIVED', blockers: 2, importants: 1 } });
  // A note that is not a review reads as no verdict.
  sh(GIT, dir, ['notes', '--ref=cr', 'add', '-f', '-m', 'not json', 'wu/E-A/w2']);
  assert.deepEqual((await reader.read(GIT)).reviews, { [w2]: { verdict: null, blockers: 0, importants: 0 } });
  // Times: main, the open epic, the open unit and its task; not the unit merged into its epic.
  const timed = Object.keys(facts.times).sort();
  assert.deepEqual(timed, [rev(dir, 'main'), rev(dir, 'epic/E-A'), w2, rev(dir, 'wu/E-A/w2_probe')].sort());
  for (const at of Object.values(facts.times)) assert.match(at, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:Z|[+-]\d\d:\d\d)$/);
});

test('W6 review round 1 (items.ts:82): the reader publishes the open sets the list draws from; a task whose name keel does not name is never listed, merged or not', async (t) => {
  const { root, dir } = world(t);
  sh(GIT, dir, ['branch', 'wu/E-A/w2_café', 'wu/E-A/w2']);
  const facts = await new GitReader(dir, opts(root)).read(GIT);
  assert.deepEqual(facts.open, { epics: ['epic/E-A'], units: ['wu/E-A/w2'], tasks: ['wu/E-A/w2_probe'] });
});

test('W6 review round 2 (items.ts:153): the reader gives main\'s upstream remote from branch.main.remote; a local upstream is ., none is null', async (t) => {
  const { root, dir } = world(t);
  const reader = new GitReader(dir, opts(root));
  assert.equal((await reader.read(GIT)).branches.find((b) => b.name === 'main')?.remote, 'origin');
  sh(GIT, dir, ['branch', 'develop', 'main~1']);
  sh(GIT, dir, ['branch', '--set-upstream-to=develop', 'main']);
  const local = (await reader.read(GIT)).branches.find((b) => b.name === 'main');
  assert.deepEqual([local?.upstream, local?.remote, local?.ahead], ['develop', '.', 1]);
  sh(GIT, dir, ['branch', '--unset-upstream', 'main']);
  assert.equal((await reader.read(GIT)).branches.find((b) => b.name === 'main')?.remote, null);
});


test('epic review round 1 (open.ts:131): the project runner the reader and Open share runs a line an old git refuses again without --no-lazy-fetch, refuses a git older than 2.15 and a partial clone with why, and remembers each per git', async (t) => {
  const { root, dir } = world(t);
  const rec = path.join(root, 'runner.jsonl');
  const old = oldGit(path.join(mk(path.join(root, 'old')), 'git'), rec, GIT);
  const older = oldGit(path.join(mk(path.join(root, 'older')), 'git'), rec, GIT, { version: '2.14' });
  const runner = new ProjectRunner(dir, opts(root));
  const top = await runner.run(old, 'toplevel', {});
  assert.equal(typeof top === 'string' ? top : top.code, 0);
  const n = records(rec).length;
  await runner.run(old, 'toplevel', {});
  assert.deepEqual(records(rec).slice(n).map((r) => r.argv.includes('--no-lazy-fetch') || r.argv.includes('--get-regexp')), [false], 'remembered: one run, no config read');
  assert.match(String(await runner.run(older, 'toplevel', {})), /older than 2\.15/);
  sh(GIT, dir, ['config', 'remote.origin.promisor', 'true']);
  assert.match(String(await new ProjectRunner(dir, opts(root)).run(old, 'toplevel', {})), /partial clone needs git 2\.45/);
  assert.equal(typeof (await new ProjectRunner(dir, opts(root)).run(GIT, 'toplevel', {})), 'object', 'a new git reads a partial clone');
});

test('epic review round 1 (git.ts:323, git.ts:315): a read with no ref moved runs no note-of and no --merged; a moved notes ref reads its kind again, a moved head reads what holds it again, and the facts match a fresh reader', async (t) => {
  const { root, dir } = world(t);
  const rec = path.join(root, 'memo-runs.txt');
  const counting = program(path.join(mk(path.join(root, 'memo')), 'git'), `#!/bin/sh\necho "$*" >> ${rec}\nexec ${GIT} "$@"\n`);
  const runs = () => readFileSync(rec, 'utf8').split('\n');
  const count = (from: number, what: string) => runs().slice(from).filter((l) => l.includes(what)).length;
  const reader = new GitReader(dir, opts(root));
  const fresh = async () => new GitReader(dir, opts(root)).read(GIT);
  const first = await reader.read(counting);
  assert.ok(count(0, ' notes ') > 0 && count(0, '--merged=') > 0, 'the first read asks both');
  let mark = runs().length;
  assert.deepEqual(await reader.read(counting), first);
  assert.deepEqual([count(mark, ' notes '), count(mark, '--merged=')], [0, 0], 'nothing moved: no note-of, no --merged');
  // A new sign-off note: the mr ref moved, so mr is asked again (cr is not); no head moved.
  sh(GIT, dir, ['notes', '--ref=mr', 'add', '-m', '{}', 'wu/E-A/w2']);
  mark = runs().length;
  const signed = await reader.read(counting);
  assert.deepEqual(signed.notes.mr, [rev(dir, 'wu/E-A/w2')]);
  assert.ok(count(mark, '--ref=refs/notes/mr') > 0);
  assert.deepEqual([count(mark, '--ref=refs/notes/cr'), count(mark, '--merged=')], [0, 0]);
  assert.deepEqual(signed, await fresh());
  // A commit on wu/E-A/w2: what holds it is read again, and its new tip's notes are asked.
  sh(GIT, path.join(root, 'wt-w2'), ['commit', '-q', '--allow-empty', '-m', 'more']);
  mark = runs().length;
  const moved = await reader.read(counting);
  assert.ok(count(mark, '--merged=refs/heads/epic/E-A') === 1 && count(mark, '--merged=refs/heads/wu/E-A/w2') === 1);
  assert.ok(count(mark, ' notes ') > 0);
  assert.deepEqual(moved, await fresh());
  // The task merged into its unit by a commit elsewhere: the unit's read answers the new truth.
  sh(GIT, path.join(root, 'wt-w2'), ['merge', '-q', '--no-ff', '-m', 'task in', 'wu/E-A/w2_probe']);
  const merged = await reader.read(counting);
  assert.deepEqual(merged.merged['wu/E-A/w2'], ['wu/E-A/w2_probe']);
  assert.deepEqual(merged, await fresh());
});

test('epic review round 2 (git.ts:132, git.ts:120): the reader reads notes, times and tasks only for what the list shows: a task merged into its unit has no time read, a two-underscore branch is no task (no task read for its unit, no time), and a failed main or epic read asks no notes for what it hides', async (t) => {
  const { root, dir } = world(t);
  // A task merged into wu/E-A/w2 (in w2's worktree), and a two-underscore branch off it.
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w2_done', 'wu/E-A/w2']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'done task']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w2_a_b']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'two underscores']);
  sh(GIT, path.join(root, 'wt-w2'), ['merge', '-q', '--no-ff', '-m', 'task in', 'wu/E-A/w2_done']);
  // An open unit wu/E-A/w3 whose only underscore branch has two underscores.
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w3', 'epic/E-A']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'w3']);
  sh(GIT, dir, ['branch', 'wu/E-A/w3_a_b']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'w3 on']);
  sh(GIT, dir, ['switch', '-q', 'main']);
  const rec = path.join(root, 'runs.txt');
  const flag = path.join(root, 'fail');
  const flaky = program(path.join(mk(path.join(root, 'flaky')), 'git'), `#!/bin/sh\necho "$*" >> ${rec}\nif [ -f ${flag} ]; then case "$*" in *"--merged=refs/heads/$(cat ${flag}) "*) exit 128;; esac; fi\nexec ${GIT} "$@"\n`);
  const hidden = ['wu/E-A/w2_done', 'wu/E-A/w2_a_b', 'wu/E-A/w3_a_b'].map((b) => rev(dir, b));
  const facts = await new GitReader(dir, opts(root)).read(flaky);
  assert.equal(facts.problem, null);
  assert.deepEqual(facts.open, { epics: ['epic/E-A'], units: ['wu/E-A/w2', 'wu/E-A/w3'], tasks: ['wu/E-A/w2_probe'] });
  assert.deepEqual(Object.keys(facts.times).sort(), ['main', 'epic/E-A', 'wu/E-A/w2', 'wu/E-A/w3', 'wu/E-A/w2_probe'].map((b) => rev(dir, b)).sort());
  let runs = readFileSync(rec, 'utf8').split('\n');
  const timeRuns = runs.filter((l) => l.includes('--format=%cI'));
  assert.ok(timeRuns.length > 0);
  for (const tip of hidden) assert.ok(timeRuns.every((l) => !l.includes(tip)), `no time read for ${tip}`);
  assert.ok(runs.some((l) => l.includes('--merged=refs/heads/wu/E-A/w2 ')), 'w2 has a task: read for it');
  assert.ok(!runs.some((l) => l.includes('--merged=refs/heads/wu/E-A/w3 ')), 'w3 has no task: its only underscore branch has two');
  // main's read fails: nothing is listed, and no note is asked.
  writeFileSync(rec, '');
  writeFileSync(flag, 'main');
  const noMain = await new GitReader(dir, opts(root)).read(flaky);
  assert.match(noMain.problem ?? '', /main/);
  assert.deepEqual(noMain.open, { epics: [], units: [], tasks: [] });
  runs = readFileSync(rec, 'utf8').split('\n');
  assert.ok(!runs.some((l) => l.includes(' notes ')), 'no note read for a branch the list does not show');
  assert.deepEqual(Object.keys(noMain.times), [rev(dir, 'main')]);
  // The epic's read fails: its units are not listed, and their notes are not asked.
  writeFileSync(rec, '');
  writeFileSync(flag, 'epic/E-A');
  const noEpic = await new GitReader(dir, opts(root)).read(flaky);
  assert.deepEqual(noEpic.open, { epics: ['epic/E-A'], units: [], tasks: [] });
  runs = readFileSync(rec, 'utf8').split('\n');
  assert.ok(!runs.some((l) => l.includes(` list ${rev(dir, 'wu/E-A/w2')}`)), 'the unit\'s notes are not read');
  assert.deepEqual(noEpic.notes.cr, []);
});

test('epic review round 2 (git.ts:132): past TIME_TIPS open branches, the times read says so, as the notes read does past NOTE_TIPS', async (t) => {
  const { root, dir } = world(t);
  for (let i = 0; i < TIME_TIPS; i++) sh(GIT, dir, ['branch', `wu/E-A/x${String(i).padStart(2, '0')}`, i % 2 === 0 ? 'wu/E-A/w2' : 'epic/E-A']);
  // Distinct tips: each new unit gets a commit of its own through commit-tree, no checkout.
  const tree = sh(GIT, dir, ['rev-parse', 'epic/E-A^{tree}']).trim();
  for (let i = 0; i < TIME_TIPS; i++) {
    const name = `wu/E-A/x${String(i).padStart(2, '0')}`;
    const c = sh(GIT, dir, ['commit-tree', tree, '-p', name, '-m', name]).trim();
    sh(GIT, dir, ['update-ref', `refs/heads/${name}`, c]);
  }
  const facts = await new GitReader(dir, opts(root)).read(GIT);
  assert.equal(facts.open.units.length, TIME_TIPS + 1);
  assert.equal(Object.keys(facts.times).length, TIME_TIPS);
  assert.match(facts.problem ?? '', new RegExp(`more than ${TIME_TIPS} open branches: times read for the first ${TIME_TIPS} only`));
  assert.match(facts.problem ?? '', new RegExp(`more than ${NOTE_TIPS} open branches: notes read for the first ${NOTE_TIPS} only`));
  assert.ok(facts.times[rev(dir, 'main')] !== undefined && facts.times[rev(dir, 'epic/E-A')] !== undefined, 'main and the epic first');
});
