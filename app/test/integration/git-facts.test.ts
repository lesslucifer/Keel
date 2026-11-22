// The git reader at its real boundary: real git in a temp keel project (main with an upstream, an
// epic, work units, a task, notes, a worktree), read through the fixed commands only.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { GitReader } from '../../src/main/reader/git.ts';
import { OUTPUT_CAP } from '../../src/main/commands.ts';
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
  assert.deepEqual(byName.main, { name: 'main', tip: rev(dir, 'main'), upstream: 'origin/main', ahead: 1, behind: 0, gone: false });
  assert.deepEqual(byName['epic/E-A'], { name: 'epic/E-A', tip: rev(dir, 'epic/E-A'), upstream: null, ahead: 0, behind: 0, gone: false });
  assert.deepEqual(facts.notes, { mr: [rev(dir, 'wu/E-A/w1')], cr: [rev(dir, 'wu/E-A/w2')], dod: [] });
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
  // The work unit's tip moved past its note: the note stays on the old tip, not on the branch.
  assert.deepEqual(facts.notes.mr, [rev(dir, 'wu/E-A/w1')]);
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
