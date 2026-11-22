// The git reader's parsers, each on fabricated output as git prints it, and the targets whose merges it reads.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseRefs, parseNotes, parseMerged, parseWorktrees, mergeTargets, heldBy } from '../../src/main/reader/git.ts';

const A = 'a'.repeat(40);
const B = 'b'.repeat(40);
const C = 'c'.repeat(40);
const row = (...f: string[]) => f.join('\u0000');

test('the refs read gives each local branch with its tip, upstream and where it stands, and which notes refs exist', () => {
  const out = [
    row('refs/heads/main', A, 'origin/main', 'ahead 2, behind 1'),
    row('refs/heads/epic/E-APPLIST', B, '', ''),
    row('refs/heads/wu/E-APPLIST/w5-reader-watcher', C, 'origin/wu/E-APPLIST/w5-reader-watcher', 'gone'),
    row('refs/heads/topic', A, 'origin/topic', 'behind 3'),
    row('refs/notes/mr', B, '', ''),
    row('refs/notes/cr', C, '', ''),
    row('refs/notes/test', C, '', ''),
    '',
  ].join('\n');
  const r = parseRefs(out);
  assert.deepEqual(r.branches, [
    { name: 'main', tip: A, upstream: 'origin/main', ahead: 2, behind: 1, gone: false },
    { name: 'epic/E-APPLIST', tip: B, upstream: null, ahead: 0, behind: 0, gone: false },
    { name: 'wu/E-APPLIST/w5-reader-watcher', tip: C, upstream: 'origin/wu/E-APPLIST/w5-reader-watcher', ahead: 0, behind: 0, gone: true },
    { name: 'topic', tip: A, upstream: 'origin/topic', ahead: 0, behind: 3, gone: false },
  ]);
  assert.deepEqual([...r.noteKinds].sort(), ['cr', 'mr']);
});

test('a refs row that is not four fields, or a tip that is not a full id, is dropped; a sha-256 id is kept', () => {
  const D = 'd'.repeat(64);
  const out = [row('refs/heads/a', 'abc', '', ''), row('refs/heads/b', A, ''), 'garbage', row('refs/tags/v1', A, '', ''), row('refs/heads/c', D, '', '')].join('\n');
  assert.deepEqual(parseRefs(out).branches.map((b) => b.name), ['c']);
  assert.deepEqual(parseRefs('').branches, []);
});

test('a notes list gives the annotated objects that are branch tips, nothing else', () => {
  const n1 = '1'.repeat(40);
  const out = `${n1} ${A}\n${n1} ${C}\n${n1} ${'e'.repeat(40)}\nbad line\n`;
  assert.deepEqual(parseNotes(out, new Set([A, B, C])), [A, C]);
  assert.deepEqual(parseNotes('', new Set([A])), []);
});

test('a merged read gives the branch names, refs outside refs/heads/ dropped', () => {
  assert.deepEqual(parseMerged('refs/heads/main\nrefs/heads/epic/E-X\nrefs/remotes/o/x\n\n'), ['main', 'epic/E-X']);
});

test('the worktree list gives each worktree\'s path, branch and tip; a detached or bare one has no branch', () => {
  const out = [
    'worktree /Users/me/Keel', `HEAD ${A}`, 'branch refs/heads/main', '',
    'worktree /Users/me/Keel-w5', `HEAD ${B}`, 'branch refs/heads/wu/E-APPLIST/w5-reader-watcher', 'locked', '',
    'worktree /Users/me/scratch', `HEAD ${C}`, 'detached', 'prunable gitdir file points to non-existent location', '',
    'worktree /Users/me/bare', 'bare', '',
  ].join('\n');
  assert.deepEqual(parseWorktrees(out), [
    { path: '/Users/me/Keel', branch: 'main', tip: A },
    { path: '/Users/me/Keel-w5', branch: 'wu/E-APPLIST/w5-reader-watcher', tip: B },
    { path: '/Users/me/scratch', branch: null, tip: C },
    { path: '/Users/me/bare', branch: null, tip: null },
  ]);
  assert.deepEqual(parseWorktrees(''), []);
  assert.deepEqual(parseWorktrees('worktree relative/path\n\n'), []);
});

test('the merge targets are main, each epic and each work unit that has a task branch; each holds only its own kind', () => {
  const names = ['main', 'epic/E-A', 'epic/E-B', 'wu/E-A/w1', 'wu/E-A/w1_look-variants', 'wu/E-A/w2', 'wu/E-B/w1', 'topic', 'epic/bad name'];
  assert.deepEqual(mergeTargets(names), ['main', 'epic/E-A', 'epic/E-B', 'wu/E-A/w1']);
  assert.deepEqual(heldBy('main', names), ['epic/E-A', 'epic/E-B']);
  assert.deepEqual(heldBy('epic/E-A', names), ['wu/E-A/w1', 'wu/E-A/w1_look-variants', 'wu/E-A/w2']);
  assert.deepEqual(heldBy('wu/E-A/w1', names), ['wu/E-A/w1_look-variants']);
  assert.deepEqual(mergeTargets(['epic/E-A']), ['epic/E-A']);
});
