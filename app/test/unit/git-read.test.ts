// The git reader's parsers, each on fabricated output as git prints it, and the targets whose merges it reads.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { refOk } from '../../src/main/commands.ts';
import { keelBranch, isEpicBranch, isUnitBranch, isTaskBranch, epicOfUnit, unitOfTask, unitPrefix, epicOfWu, routerKey, KEY, WU_ID, WU_SOURCE, EPIC_SOURCE, wuOfBranch, epicOfBranch } from '../../src/shared/names.ts';
import { seeded, pick } from '../support/seeded.ts';
import { parseRefs, parseMerged, parseWorktrees, heldBy, openEpics, openTips, onMainLine, mayBeFresh, NOTE_TIPS, parseReview, parseTime, timeTips, TIME_TIPS, openSets, parseRemote } from '../../src/main/reader/git.ts';

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
  // Epic review round 1 (git.ts:323): each known notes ref's tip is kept, so an unmoved ref asks nothing again.
  assert.deepEqual([...r.noteTips].sort(), [['cr', C], ['mr', B]]);
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

test('the open epics are those not in main (all of them when there is no main); the open work units are those of an open epic not yet in it, by the open sets', () => {
  const names = ['main', 'epic/E-A', 'epic/E-B', 'wu/E-A/w1', 'wu/E-A/w2', 'wu/E-A/w2_task', 'wu/E-B/w1', 'wu/E-C/w1', 'epic/bad name'];
  assert.deepEqual(openEpics(names, ['epic/E-B']), ['epic/E-A']);
  assert.deepEqual(openEpics(names, undefined), ['epic/E-A', 'epic/E-B']);
  // An epic main holds but whose tip sits on main's own line has no commit of its own yet: it is open.
  assert.deepEqual(openEpics(names, ['epic/E-A', 'epic/E-B'], new Set(['epic/E-B'])), ['epic/E-B']);
  assert.deepEqual(openSets(names, { main: ['epic/E-B'], 'epic/E-A': ['wu/E-A/w1'] }).units, ['wu/E-A/w2']);
  assert.deepEqual(openSets(names.filter((n) => n !== 'main'), { 'epic/E-A': [], 'epic/E-B': [] }).units, ['wu/E-A/w1', 'wu/E-A/w2', 'wu/E-B/w1']);
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
  assert.deepEqual(openTips(branches, openSets(branches.map((b) => b.name), merged)), { tips: [B, E], over: false });
  assert.equal(NOTE_TIPS, 40);
});

test('past NOTE_TIPS open branches, only the first are read, and it says so', () => {
  const many = [br('epic/E-A', 'e'.repeat(40)), ...Array.from({ length: NOTE_TIPS + 3 }, (_, i) => br(`wu/E-A/w${i}`, i.toString(16).padStart(40, '0')))];
  const r = openTips(many, openSets(many.map((b) => b.name), { 'epic/E-A': [] }));
  assert.equal(r.tips.length, NOTE_TIPS);
  assert.equal(r.over, true);
});

test('a fresh epic main holds (its tip on main\'s own line) stays open, and so does its work unit, whose notes are read', () => {
  const M = 'm'.replace('m', '1').repeat(40);
  const W = '2'.repeat(40);
  const branches = [br('main', M), br('epic/E-X', M), br('wu/E-X/plan', W)];
  const merged = { main: ['epic/E-X'] };
  const names = branches.map((b) => b.name);
  assert.deepEqual(openTips(branches, openSets(names, { ...merged, 'epic/E-X': [] }, new Set(['epic/E-X']))), { tips: [M, W], over: false });
  assert.deepEqual(openTips(branches, openSets(names, merged)), { tips: [], over: false }, 'without the fresh set the epic reads as closed');
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
  assert.deepEqual(timeTips(branches, openSets(branches.map((x) => x.name), merged)), { tips: ['m'.repeat(40), 'e'.repeat(40), '1'.repeat(40), '2'.repeat(40)], over: false });
  const many = Array.from({ length: TIME_TIPS + 5 }, (_, i) => b(`wu/E-A/w${i + 1}`, i.toString(16).padStart(40, '0')));
  const all = [b('epic/E-A', 'e'.repeat(40)), ...many];
  const r = timeTips(all, openSets(all.map((x) => x.name), { 'epic/E-A': [] }));
  assert.equal(r.tips.length, TIME_TIPS);
  assert.equal(r.over, true);
});

test('epic review round 2 fix (git.ts:369): TIME_TIPS bounds the open branches\' tips, main apart: main and exactly TIME_TIPS open tips are not over, one more is; main sharing an open tip is read once; no main is no main tip', () => {
  const b = (name: string, tip: string) => ({ name, tip, upstream: null, ahead: 0, behind: 0, gone: false });
  const tip = (i: number) => i.toString(16).padStart(40, '0');
  const units = (n: number) => Array.from({ length: n }, (_, i) => b(`wu/E-A/w${i + 1}`, tip(i + 100)));
  const open = (bs: { name: string }[]) => ({ epics: [], units: bs.map((x) => x.name), tasks: [] });
  const at = units(TIME_TIPS);
  assert.deepEqual(timeTips([b('main', tip(1)), ...at], open(at)), { tips: [tip(1), ...at.map((x) => x.tip)], over: false });
  const past = units(TIME_TIPS + 1);
  const r = timeTips([b('main', tip(1)), ...past], open(past));
  assert.deepEqual(r, { tips: [tip(1), ...past.slice(0, TIME_TIPS).map((x) => x.tip)], over: true });
  // Main on an open epic's tip: that tip once, main's place first.
  const epic = b('epic/E-A', tip(1));
  assert.deepEqual(timeTips([b('main', tip(1)), epic, ...at], { epics: ['epic/E-A'], units: at.map((x) => x.name), tasks: [] }), { tips: [tip(1), ...at.slice(0, TIME_TIPS - 1).map((x) => x.tip)], over: true });
  // No main branch: only the open tips.
  assert.deepEqual(timeTips(at, open(at)), { tips: at.map((x) => x.tip), over: false });
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

test('epic review round 1 (git.ts:106): a work unit or task branch names its epic branch, and an epic branch the prefix of its units, by one rule each way', () => {
  assert.equal(epicOfUnit('wu/E-A/w1-x'), 'epic/E-A');
  assert.equal(epicOfUnit('wu/E-A/w1-x_probe'), 'epic/E-A');
  assert.equal(unitPrefix('epic/E-A'), 'wu/E-A/');
  const rng = seeded(0x1d06);
  const seg = ['E-A', 'E-APPLIST', 'foo', 'x1', 'w2-y'];
  for (let i = 0; i < 500; i++) {
    const epic = `epic/${pick(rng, seg)}`;
    const unit = `${unitPrefix(epic)}${pick(rng, seg)}`;
    assert.equal(epicOfUnit(unit), epic, unit);
    assert.ok(heldBy(epic, [unit]).length === (isUnitBranch(unit) ? 1 : 0), unit);
  }
});

test('epic review round 1 (items.ts:30): one work unit id pattern: the id, a branch\'s id and an epic\'s id are built from it', () => {
  assert.equal(WU_ID.source, `^${WU_SOURCE}$`);
  assert.ok(WU_SOURCE.startsWith(EPIC_SOURCE));
  const rng = seeded(0x30a1);
  const parts = ['E-', 'A', 'APPLIST', '0', '.W', 'W', '1', '12', '1234', 'x', '.'];
  for (let i = 0; i < 2000; i++) {
    const id = Array.from({ length: 1 + Math.floor(rng() * 5) }, () => pick(rng, parts)).join('');
    const m = /^(.*)\.W([0-9]+)$/.exec(id);
    // A branch carries the id exactly when the id is one (its number without leading zeros).
    if (WU_ID.test(id) && m !== null && String(Number(m[2])) === m[2]) assert.equal(wuOfBranch(`wu/${m[1] as string}/w${m[2] as string}-x`), id, id);
    if (WU_ID.test(id)) assert.equal(epicOfWu(id), id.slice(0, id.lastIndexOf('.W')), id);
    assert.equal(epicOfBranch(`epic/${id}`) !== null, new RegExp(`^${EPIC_SOURCE}$`).test(id), id);
  }
});

test('epic review round 1 (items.ts:172): a router key is read by the log\'s one grammar: routerKey reads exactly the keys KEY takes, and names what each is about', () => {
  assert.deepEqual(routerKey('router'), { kind: 'router' });
  assert.deepEqual(routerKey('facts'), { kind: 'facts' });
  assert.deepEqual(routerKey('wu/E-A.W2/stage'), { kind: 'stage', wu: 'E-A.W2' });
  assert.deepEqual(routerKey('wu/E-A.W2/conflict'), { kind: 'conflict', wu: 'E-A.W2' });
  assert.deepEqual(routerKey('epic/E-A/paused'), { kind: 'paused', epic: 'E-A' });
  assert.deepEqual(routerKey('question/q-1'), { kind: 'question', name: 'q-1' });
  assert.deepEqual(routerKey('worker/w1'), { kind: 'worker', name: 'w1' });
  assert.deepEqual(routerKey('worker/w1/report'), { kind: 'report', name: 'w1' });
  const rng = seeded(0x4e7);
  const parts = ['router', 'facts', 'wu/', 'E-A.W1', 'E-A', '/stage', '/conflict', '/paused', 'epic/', 'question/', 'worker/', 'q-1', '/report', 'X', '/', '-'];
  for (let i = 0; i < 3000; i++) {
    const key = Array.from({ length: 1 + Math.floor(rng() * 4) }, () => pick(rng, parts)).join('');
    assert.equal(routerKey(key) !== null, KEY.test(key), key);
  }
});

test('epic review round 2 (git.ts:132): the notes and times reads take their tips from the open sets, the one rule (main too for the times): a failed main read, a failed epic read and a task merged into its unit ask no tip the list does not show', () => {
  const T = (c: string) => c.repeat(40);
  const branches = [br('main', T('0')), br('epic/E-A', T('1')), br('epic/E-B', T('2')), br('wu/E-A/w1', T('3')), br('wu/E-A/w1_done', T('4')), br('wu/E-A/w1_probe', T('5')), br('wu/E-B/w1', T('6'))];
  const names = branches.map((b) => b.name);
  const tips = (merged: Record<string, string[]>) => {
    const open = openSets(names, merged);
    return { notes: openTips(branches, open).tips, times: timeTips(branches, open).tips };
  };
  // All read: E-A's unit open with one task, its other task merged into it; E-B's unit open.
  assert.deepEqual(tips({ main: [], 'epic/E-A': [], 'epic/E-B': [], 'wu/E-A/w1': ['wu/E-A/w1_done'] }), { notes: [T('1'), T('2'), T('3'), T('6')], times: [T('0'), T('1'), T('2'), T('3'), T('6'), T('5')] }, 'the merged task has no time');
  // Main's read failed: nothing is open, so no note and no time but main's.
  assert.deepEqual(tips({ 'epic/E-A': [], 'epic/E-B': [], 'wu/E-A/w1': [] }), { notes: [], times: [T('0')] });
  // E-B's read failed: its unit is not listed, so not read.
  assert.deepEqual(tips({ main: [], 'epic/E-A': [], 'wu/E-A/w1': [] }), { notes: [T('1'), T('2'), T('3')], times: [T('0'), T('1'), T('2'), T('3'), T('4'), T('5')] });
  // The open sets in, the tips out: nothing else decides.
  assert.deepEqual(openTips(branches, { epics: ['epic/E-B'], units: ['wu/E-A/w1'], tasks: ['wu/E-A/w1_probe'] }).tips, [T('2'), T('3')]);
  assert.deepEqual(timeTips(branches, { epics: [], units: [], tasks: ['wu/E-A/w1_probe', 'nope'] }).tips, [T('0'), T('5')]);
});

test('epic review round 2 (git.ts:132): branches the list does not show never crowd a shown one out of NOTE_TIPS or TIME_TIPS', () => {
  const tip = (i: number) => i.toString(16).padStart(40, '0');
  // E-A's read failed: its NOTE_TIPS + 5 units sort before E-B's open unit and are not listed.
  const units = Array.from({ length: NOTE_TIPS + 5 }, (_, i) => br(`wu/E-A/w${i + 1}`, tip(i + 10)));
  const branches = [br('main', tip(1)), br('epic/E-A', tip(2)), br('epic/E-B', tip(3)), ...units, br('wu/E-B/w1', tip(4))];
  const open = openSets(branches.map((b) => b.name), { main: [], 'epic/E-B': [] });
  assert.deepEqual(open.units, ['wu/E-B/w1']);
  assert.deepEqual(openTips(branches, open), { tips: [tip(2), tip(3), tip(4)], over: false });
  // TIME_TIPS + 5 tasks merged into w1 sort before the open unit w2.
  const tasks = Array.from({ length: TIME_TIPS + 5 }, (_, i) => br(`wu/E-C/w1_t${i}`, tip(i + 200)));
  const more = [br('main', tip(1)), br('epic/E-C', tip(5)), br('wu/E-C/w1', tip(6)), ...tasks, br('wu/E-C/w2', tip(7))];
  const openC = openSets(more.map((b) => b.name), { main: [], 'epic/E-C': [], 'wu/E-C/w1': tasks.map((b) => b.name) });
  assert.deepEqual(timeTips(more, openC), { tips: [tip(1), tip(5), tip(6), tip(7)], over: false });
  // Main and the epics come first: a crowd of open tasks never pushes them out.
  const openTasks = Array.from({ length: TIME_TIPS + 5 }, (_, i) => `wu/E-C/w1_t${i}`);
  const r = timeTips(more, { epics: ['epic/E-C'], units: ['wu/E-C/w1', 'wu/E-C/w2'], tasks: openTasks });
  assert.deepEqual(r.tips.slice(0, 4), [tip(1), tip(5), tip(6), tip(7)]);
  assert.equal(r.tips.length, TIME_TIPS + 1, 'main and TIME_TIPS open tips');
  assert.equal(r.over, true);
});

test('epic review round 2 (git.ts:120): a work unit\'s tasks are named by names.ts alone: wu/E-A/w1_a_b is no task of wu/E-A/w1 to heldBy, to the open sets or to the times read', () => {
  const names = ['main', 'epic/E-A', 'wu/E-A/w1', 'wu/E-A/w1_a', 'wu/E-A/w1_a_b', 'wu/E-A/w10', 'wu/E-A/w1-x_t'];
  assert.ok(keelBranch('wu/E-A/w1_a_b') && !isTaskBranch('wu/E-A/w1_a_b'));
  assert.deepEqual(heldBy('wu/E-A/w1', names), ['wu/E-A/w1_a']);
  assert.deepEqual(heldBy('wu/E-A/w1-x', names), ['wu/E-A/w1-x_t']);
  const open = openSets(names, { main: [], 'epic/E-A': [], 'wu/E-A/w1': [] });
  assert.deepEqual(open.tasks, ['wu/E-A/w1_a']);
  const branches = names.map((n, i) => br(n, i.toString(16).padStart(40, '0')));
  assert.ok(!timeTips(branches, open).tips.includes(branches[4]?.tip as string), 'no time read for wu/E-A/w1_a_b');
  // The one rule, on generated names: heldBy(unit) is exactly the names isTaskBranch and unitOfTask give that unit.
  const rng = seeded(0x7a5c);
  const parts = ['w1', 'w2', '_', 'a', 'b', '-', 'x', '.', 'é'];
  for (let i = 0; i < 2000; i++) {
    const pool = Array.from({ length: 6 }, () => `wu/E-A/${Array.from({ length: 1 + Math.floor(rng() * 5) }, () => pick(rng, parts)).join('')}`);
    for (const unit of pool.filter(isUnitBranch)) {
      assert.deepEqual(heldBy(unit, pool), pool.filter((n) => isTaskBranch(n) && unitOfTask(n) === unit), `${unit} in ${pool.join(' ')}`);
    }
  }
});
