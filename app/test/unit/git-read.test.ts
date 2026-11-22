// The git reader's parsers, each on fabricated output as git prints it, and the targets whose merges it reads.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { refOk } from '../../src/main/commands.ts';
import { keelBranch, isEpicBranch, isUnitBranch, isTaskBranch } from '../../src/shared/names.ts';
import { seeded, pick } from '../support/seeded.ts';
import { parseRefs, parseMerged, parseWorktrees, heldBy, openEpics, openUnits, openTips, onMainLine, mayBeFresh, NOTE_TIPS, parseReview, parseTime, timeTips, TIME_TIPS, openSets, parseRemote } from '../../src/main/reader/git.ts';

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

test('what each target holds is read for its own kind: main the epics and work units, an epic its work units, a work unit its tasks', () => {
  const names = ['main', 'epic/E-A', 'epic/E-B', 'wu/E-A/w1', 'wu/E-A/w1_look-variants', 'wu/E-A/w2', 'wu/E-B/w1', 'topic', 'epic/bad name'];
  assert.deepEqual(heldBy('main', names), ['epic/E-A', 'epic/E-B', 'wu/E-A/w1', 'wu/E-A/w2', 'wu/E-B/w1'], 'main is read for both; the view publishes only its epics');
  assert.deepEqual(heldBy('epic/E-A', names), ['wu/E-A/w1', 'wu/E-A/w1_look-variants', 'wu/E-A/w2']);
  assert.deepEqual(heldBy('wu/E-A/w1', names), ['wu/E-A/w1_look-variants']);
});

test('the open epics are those not in main (all of them when there is no main); the open work units are those of an open epic not yet in it', () => {
  const names = ['main', 'epic/E-A', 'epic/E-B', 'wu/E-A/w1', 'wu/E-A/w2', 'wu/E-A/w2_task', 'wu/E-B/w1', 'wu/E-C/w1', 'epic/bad name'];
  assert.deepEqual(openEpics(names, ['epic/E-B']), ['epic/E-A']);
  assert.deepEqual(openEpics(names, undefined), ['epic/E-A', 'epic/E-B']);
  // An epic main holds but whose tip sits on main's own line has no commit of its own yet: it is open.
  assert.deepEqual(openEpics(names, ['epic/E-A', 'epic/E-B'], new Set(['epic/E-B'])), ['epic/E-B']);
  assert.deepEqual(openUnits(names, ['epic/E-A'], { 'epic/E-A': ['wu/E-A/w1'] }), ['wu/E-A/w2']);
  assert.deepEqual(openUnits(names, ['epic/E-A', 'epic/E-B'], {}), ['wu/E-A/w1', 'wu/E-A/w2', 'wu/E-B/w1']);
});

const br = (name: string, tip: string) => ({ name, tip, upstream: null, ahead: 0, behind: 0, gone: false });

test('the tips whose notes are read: each epic not yet in main, each work unit of such an epic not yet in it, each tip once; not tasks, main or other branches', () => {
  const D = 'd'.repeat(40);
  const E = 'e'.repeat(40);
  const F = 'f'.repeat(40);
  const G = '9'.repeat(40);
  const branches = [br('main', A), br('epic/E-A', B), br('epic/E-B', C), br('wu/E-A/w1', D), br('wu/E-A/w2', E), br('wu/E-A/w2_task', A), br('wu/E-C/w1', F), br('wu/E-B/w7', G), br('topic', C)];
  const merged = { main: ['epic/E-B'], 'epic/E-A': ['wu/E-A/w1'] };
  // wu/E-C/w1 has no epic branch, wu/E-B/w7 belongs to an epic already in main: neither is open, however long kept.
  assert.deepEqual(openTips(branches, merged), { tips: [B, E], over: false });
  assert.equal(NOTE_TIPS, 40);
});

test('past NOTE_TIPS open branches, only the first are read, and it says so', () => {
  const many = [br('epic/E-A', 'e'.repeat(40)), ...Array.from({ length: NOTE_TIPS + 3 }, (_, i) => br(`wu/E-A/w${i}`, i.toString(16).padStart(40, '0')))];
  const r = openTips(many, {});
  assert.equal(r.tips.length, NOTE_TIPS);
  assert.equal(r.over, true);
});

test('a fresh epic main holds (its tip on main\'s own line) stays open, and so does its work unit, whose notes are read', () => {
  const M = 'm'.replace('m', '1').repeat(40);
  const W = '2'.repeat(40);
  const branches = [br('main', M), br('epic/E-X', M), br('wu/E-X/plan', W)];
  const merged = { main: ['epic/E-X'] };
  assert.deepEqual(openTips(branches, merged, new Set(['epic/E-X'])), { tips: [M, W], over: false });
  assert.deepEqual(openTips(branches, merged), { tips: [], over: false }, 'without the fresh set the epic reads as closed');
});

test('name-rev\'s answer: main or main~k is on main\'s own line (no commit of its own); a ^ means reached through a merge (merged in)', () => {
  for (const n of ['main', 'main~1', 'main~42']) assert.equal(onMainLine(`${n}\n`), true, n);
  for (const n of ['main~3^2', 'main^2', 'main~2^2~1', '', 'undefined', 'other~1', 'main~x']) assert.equal(onMainLine(`${n}\n`), false, n);
});

test('an epic main holds may be fresh only while one of its units is outside main and none of its units main holds has a tip of its own: else no name-rev is asked (review round 4, git.ts:196)', () => {
  const M = '1'.repeat(40);
  const W = '2'.repeat(40);
  const U = '3'.repeat(40);
  const fresh = [br('epic/E-X', M), br('wu/E-X/plan', W), br('wu/E-X/w2', M)];
  // plan is outside main; w2 is in main but at the epic's own tip (cut from it, no commit): still may be fresh.
  assert.equal(mayBeFresh('epic/E-X', fresh, new Set(['wu/E-X/w2'])), true);
  // a unit main holds with a tip of its own was merged into the epic: the epic has commits of its own, closed.
  const closed = [br('epic/E-X', M), br('wu/E-X/w1', U), br('wu/E-X/w9', W)];
  assert.equal(mayBeFresh('epic/E-X', closed, new Set(['wu/E-X/w1'])), false);
  // no unit outside main: nothing open to read, no question.
  assert.equal(mayBeFresh('epic/E-X', [br('epic/E-X', M), br('wu/E-X/w1', U)], new Set(['wu/E-X/w1'])), false);
  // a task is not a unit.
  assert.equal(mayBeFresh('epic/E-X', [br('epic/E-X', M), br('wu/E-X/w1_task', W)], new Set()), false);
});

test('a review note reads as its verdict and open counts; anything else reads as no verdict', () => {
  assert.deepEqual(parseReview('{"schema":"cr/v1","verdict":"PASS","blockers":0,"importants":0,"code_diff_hash":"x"}\n'), { verdict: 'PASS', blockers: 0, importants: 0 });
  assert.deepEqual(parseReview('{"schema":"cr/v1","verdict":"FAIL","blockers":2,"importants":1}'), { verdict: 'FAIL', blockers: 2, importants: 1 });
  assert.deepEqual(parseReview('{"schema":"cr/v1","verdict":"WAIVED","waiver":"cap","blockers":1,"importants":0}'), { verdict: 'WAIVED', blockers: 1, importants: 0 });
  for (const out of ['', 'not json', '[]', 'null', '{"schema":"cr/v1","verdict":"pass"}', '{"schema":"mr/v1","verdict":"PASS"}', '{"verdict":"PASS"}', '{"schema":"cr/v1","verdict":7}']) {
    assert.deepEqual(parseReview(out), { verdict: null, blockers: 0, importants: 0 }, out);
  }
  assert.deepEqual(parseReview('{"schema":"cr/v1","verdict":"FAIL","blockers":-1,"importants":1.5}'), { verdict: 'FAIL', blockers: 0, importants: 0 }, 'odd counts read as 0');
});

test('a commit time reads as its ISO 8601 line; anything else as none', () => {
  assert.equal(parseTime('2026-11-22T08:20:00+07:00\n'), '2026-11-22T08:20:00+07:00');
  for (const out of ['', 'yesterday', '2026-11-22', '2026-11-22T08:20:00+07:00\nmore']) assert.equal(parseTime(out), null, out);
});

test('the tips whose times the list shows: main, each open epic, each open work unit and each task of one, once each, at most TIME_TIPS', () => {
  const b = (name: string, tip: string) => ({ name, tip, upstream: null, ahead: 0, behind: 0, gone: false });
  const branches = [b('main', 'm'.repeat(40)), b('epic/E-A', 'e'.repeat(40)), b('epic/E-B', 'f'.repeat(40)), b('wu/E-A/w1', '1'.repeat(40)), b('wu/E-A/w1_t', '2'.repeat(40)), b('wu/E-A/w2', '3'.repeat(40)), b('wu/E-B/w1', '4'.repeat(40)), b('feature/x', '5'.repeat(40)), b('wu/E-A/w3', '1'.repeat(40))];
  const merged = { main: ['epic/E-B'], 'epic/E-A': ['wu/E-A/w2'], 'wu/E-A/w1': [] };
  assert.deepEqual(timeTips(branches, merged), { tips: ['m'.repeat(40), 'e'.repeat(40), '1'.repeat(40), '2'.repeat(40)], over: false });
  const many = Array.from({ length: TIME_TIPS + 5 }, (_, i) => b(`wu/E-A/w${i + 1}`, i.toString(16).padStart(40, '0')));
  const r = timeTips([b('epic/E-A', 'e'.repeat(40)), ...many], {});
  assert.equal(r.tips.length, TIME_TIPS);
  assert.equal(r.over, true);
});

test('W6 review round 1 (items.ts:82): the open sets the list draws from, by the reader\'s one rule - keel\'s names only, and a unit or task only when its target was read', () => {
  const names = ['main', 'epic/E-A', 'epic/E-B', 'epic/E-C', 'wu/E-A/w1', 'wu/E-A/w2', 'wu/E-A/w1_probe', 'wu/E-A/w1_caf\u00e9', 'wu/E-A/w1_done', 'wu/E-B/w1', 'wu/E-C/w1', 'wu/E-A/w\u00e9', 'feature/x'];
  const merged = { main: ['epic/E-B'], 'epic/E-A': ['wu/E-A/w2'], 'wu/E-A/w1': ['wu/E-A/w1_done'], 'epic/E-B': [] };
  assert.deepEqual(openSets(names, merged), {
    epics: ['epic/E-A', 'epic/E-C'],
    units: ['wu/E-A/w1'],
    tasks: ['wu/E-A/w1_probe'],
  }, 'E-C: its epic read failed, so its unit is not listed; a name keel does not name is never listed; a held task is not');
  // A fresh epic main holds (no commit of its own) stays open, as the notes read has it.
  assert.deepEqual(openSets(names, { ...merged, 'epic/E-B': [] }, new Set(['epic/E-B'])).epics, ['epic/E-A', 'epic/E-B', 'epic/E-C']);
  // Main's read failed: no epic is listed as open; with no main at all, every epic is.
  assert.deepEqual(openSets(names, { 'epic/E-A': [] }), { epics: [], units: [], tasks: [] });
  assert.deepEqual(openSets(names.filter((n) => n !== 'main'), { 'epic/E-A': [] }).epics, ['epic/E-A', 'epic/E-B', 'epic/E-C']);
  // A unit with a task whose own read failed lists no task.
  assert.deepEqual(openSets(names, { main: [], 'epic/E-A': [], 'epic/E-B': [], 'epic/E-C': [] }).tasks, []);
});

test('W6 review round 2 (items.ts:103): one grammar for keel\'s branch names: the fixed commands\' ref check and the reader\'s open sets both read src/shared/names.ts', () => {
  const rng = seeded(0xb4a7);
  const parts = ['main', 'epic/', 'wu/', 'E-A', 'w1', '-x', '_t', '/', '.', '..', '.lock', 'é', ' ', 'a'];
  for (let i = 0; i < 3000; i++) {
    const name = Array.from({ length: 1 + Math.floor(rng() * 6) }, () => pick(rng, parts)).join('');
    assert.equal(refOk(`refs/heads/${name}`), keelBranch(name), name);
    const sets = openSets(['main', name], { main: [] });
    if (sets.epics.includes(name)) assert.ok(isEpicBranch(name), name);
  }
  for (const n of ['epic/E-A', 'epic/foo']) assert.ok(isEpicBranch(n), n);
  for (const n of ['wu/E-A/w1-x', 'wu/E-A/reader']) assert.ok(isUnitBranch(n) && !isTaskBranch(n), n);
  for (const n of ['wu/E-A/w1-x_probe']) assert.ok(isTaskBranch(n) && !isUnitBranch(n), n);
  for (const n of ['wu/E-A/w1_a_b', 'wu/E-A/caf\u00e9', 'epic/a/b', 'main']) assert.ok(!isUnitBranch(n) && !isTaskBranch(n) && !isEpicBranch(n), n);
});

test('W6 review round 3 (git.ts:350): main\'s remote as the config read answers it; none for one starting with -, a control character, empty or overlong', () => {
  assert.equal(parseRemote('origin\n'), 'origin');
  assert.equal(parseRemote('team/origin\n'), 'team/origin');
  assert.equal(parseRemote('.\n'), '.');
  for (const out of ['--receive-pack=touch p\n', '-x\n', '\n', '', 'a\u0007b\n', `${'a'.repeat(201)}\n`]) assert.equal(parseRemote(out), null, JSON.stringify(out));
});
