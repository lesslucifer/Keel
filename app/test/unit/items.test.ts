// The item rules (src/shared/items.ts): which item each fact makes, its group, its words, and when
// it leaves; on hand-made views, and a seeded property over generated ones.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { itemsOf, countsOf, wuOfBranch, epicOfBranch, itemId, shortHash, shQuote, type ItemInput } from '../../src/shared/items.ts';
import { ITEM_ID_SOURCE, SLOT_CAP, type GitFacts, type Item, type RouterView } from '../../src/shared/contract.ts';
import { openSets } from '../../src/main/reader/git.ts';
import { seeded, pick, type Rng } from '../support/seeded.ts';

const T = (c: string) => c.repeat(40);
const MAIN = T('9');
const EPIC = T('a');
const W1 = T('1');
const W2 = T('2');
const TASK = T('3');

/** Facts as the reader gives them: its open sets drawn by its own rule (git.ts openSets) from the branches and merged reads. */
function withOpen(g: Omit<GitFacts, 'open'> & { open?: GitFacts['open'] }, fresh: ReadonlySet<string> = new Set()): GitFacts {
  return { ...g, open: openSets(g.branches.map((b) => b.name), g.merged, fresh) };
}

function git(over: Partial<GitFacts> = {}, fresh: ReadonlySet<string> = new Set()): GitFacts {
  return withOpen(rawGit(over), fresh);
}

function rawGit(over: Partial<GitFacts> = {}): GitFacts {
  return {
    open: { epics: [], units: [], tasks: [] },
    branches: [
      { name: 'main', tip: MAIN, upstream: 'origin/main', ahead: 0, behind: 0, gone: false },
      { name: 'epic/E-A', tip: EPIC, upstream: null, ahead: 0, behind: 0, gone: false },
      { name: 'wu/E-A/w1-reader', tip: W1, upstream: null, ahead: 0, behind: 0, gone: false },
    ],
    notes: { mr: [], cr: [], dod: [] },
    merged: { main: [], 'epic/E-A': [] },
    worktrees: [],
    reviews: {},
    times: { [MAIN]: '2026-11-22T08:00:00Z', [EPIC]: '2026-11-22T08:10:00Z', [W1]: '2026-11-22T08:20:00Z' },
    problem: null,
    ...over,
  };
}

function router(values: Record<string, unknown> = {}, over: Partial<RouterView> = {}): RouterView {
  return { run: 'keel-1c.20261122-080000.1', n: 9, values, heard: true, lastAt: '2026-11-22T08:30:00Z', problem: null, ...over };
}

const input = (over: Partial<ItemInput> = {}): ItemInput => ({ git: git(), router: router(), sent: new Set(), ...over });
const only = (items: Item[]): Item => {
  assert.equal(items.length, 1, JSON.stringify(items.map((i) => i.id)));
  return items[0] as Item;
};

test('a work unit and an epic are named from their branches by keel\'s naming, and nothing else is', () => {
  assert.equal(wuOfBranch('wu/E-APPLIST/w6-list-screens'), 'E-APPLIST.W6');
  assert.equal(wuOfBranch('wu/E-A/w12'), 'E-A.W12');
  assert.equal(wuOfBranch('wu/E-A/w06-x'), 'E-A.W6');
  assert.equal(wuOfBranch('wu/E-A/w1-reader_probe'), 'E-A.W1');
  for (const b of ['main', 'epic/E-A', 'wu/E-A/reader', 'wu/e-a/w1', 'wu/E-A/w1000-x', 'wu/E-A/w1x', 'wu/E-A/b/w1']) assert.equal(wuOfBranch(b), null, b);
  assert.equal(epicOfBranch('epic/E-APPLIST'), 'E-APPLIST');
  for (const b of ['main', 'epic/e-a', 'epic/E-A/x', 'wu/E-A/w1', 'epic/E-ABCDEFGHIJKLMNOPQ']) assert.equal(epicOfBranch(b), null, b);
});

test('an item id is kind/subject@version within the cap; a long or odd subject becomes its short hash', () => {
  const re = new RegExp(ITEM_ID_SOURCE);
  assert.equal(itemId('result', 'E-A.W1', W1), 'result/E-A.W1@111111111111');
  assert.equal(itemId('plan', 'E-A.W1', 'abcdef0'), 'plan/E-A.W1@abcdef0');
  const long = itemId('epic-result', 'E-ABCDEFGHIJKLMNOP', W1);
  assert.ok(re.test(long) && long.length <= SLOT_CAP, long);
  const odd = itemId('task', 'wu/E-A/w1_probe', W1);
  assert.equal(odd, `task/${shortHash('wu/E-A/w1_probe')}@111111111111`);
  assert.match(shortHash('x'), /^[0-9a-f]{8}$/);
  assert.equal(shortHash('same'), shortHash('same'));
  assert.notEqual(shortHash('a'), shortHash('b'));
});

test('nothing waits on a quiet project: no item, counts zero', () => {
  const items = itemsOf(input());
  assert.deepEqual(items, []);
  assert.deepEqual(countsOf(items), { yours: 0, notYours: 0, sent: 0 });
  assert.deepEqual(itemsOf({ git: null, router: router({}, { run: null, n: 0, heard: false, lastAt: null }), sent: new Set() }), []);
});

test('a result enters on a PASS review note on an open work unit\'s tip, turns to merge on the sign-off note, and leaves when the epic holds the tip', () => {
  const reviewed = git({ notes: { mr: [], cr: [W1], dod: [] }, reviews: { [W1]: { verdict: 'PASS', blockers: 0, importants: 0 } } });
  const sign = only(itemsOf(input({ git: reviewed })));
  assert.deepEqual(sign, {
    id: itemId('result', `E-A.W1-${shortHash('wu/E-A/w1-reader')}`, W1),
    kind: 'result',
    group: 'yours',
    wu: 'E-A.W1',
    epic: 'E-A',
    title: 'Result to sign',
    branch: 'wu/E-A/w1-reader',
    tip: '1111111',
    since: '2026-11-22T08:20:00Z',
    why: 'The review passed. Your sign-off is missing.',
    open: 'page',
    markable: false,
    worker: null,
    line: null,
    text: [],
  });
  const signed = only(itemsOf(input({ git: { ...reviewed, notes: { mr: [W1], cr: [W1], dod: [] } } })));
  assert.equal(signed.title, 'Result to merge');
  assert.equal(signed.why, 'Signed. The merge into epic/E-A is yours, in your terminal.');
  assert.equal(signed.line, 'git merge --no-ff --no-edit wu/E-A/w1-reader');
  assert.equal(signed.id, sign.id, 'the same tip is the same item');
  assert.deepEqual(itemsOf(input({ git: withOpen({ ...reviewed, merged: { main: [], 'epic/E-A': ['wu/E-A/w1-reader'] } }) })), []);
});

test('a waived review is still yours to sign, saying what is open; a failed or unreadable one is its worker\'s, naming the worker', () => {
  const g = (verdict: 'WAIVED' | 'FAIL' | null) => git({ notes: { mr: [], cr: [W1], dod: [] }, reviews: { [W1]: { verdict, blockers: 2, importants: 1 } } });
  const waived = only(itemsOf(input({ git: g('WAIVED') })));
  assert.equal(waived.group, 'yours');
  assert.equal(waived.why, 'The review was waived with 2 blockers and 1 important open. Your sign-off is missing.');
  const worker = { 'worker/e-a-w1': { state: 'busy', branch: 'wu/E-A/w1-reader' } };
  const failed = only(itemsOf(input({ git: g('FAIL'), router: router(worker) })));
  assert.deepEqual([failed.group, failed.title, failed.why, failed.worker, failed.open], ['not-yours', 'Review failed', '2 blockers and 1 important open: its worker fixes them.', 'e-a-w1', 'none']);
  const unread = only(itemsOf(input({ git: g(null) })));
  assert.deepEqual([unread.group, unread.title], ['not-yours', 'Review note unreadable']);
  const missing = only(itemsOf(input({ git: git({ notes: { mr: [], cr: [W1], dod: [] } }) })));
  assert.equal(missing.title, 'Review note unreadable', 'a cr note whose review was not read');
});

test('an epic result enters on a PASS note on an open epic\'s tip, turns to merge into main on the sign-off, and leaves when main holds it', () => {
  const reviewed = git({ notes: { mr: [], cr: [EPIC], dod: [] }, reviews: { [EPIC]: { verdict: 'PASS', blockers: 0, importants: 0 } } });
  const sign = only(itemsOf(input({ git: reviewed })));
  assert.deepEqual([sign.id, sign.kind, sign.title, sign.epic, sign.wu, sign.open, sign.tip], ['epic-result/E-A@aaaaaaaaaaaa', 'epic-result', 'Epic result to sign', 'E-A', null, 'archive', 'aaaaaaa']);
  const signed = only(itemsOf(input({ git: { ...reviewed, notes: { mr: [EPIC], cr: [EPIC], dod: [] } } })));
  assert.deepEqual([signed.title, signed.why, signed.line], ['Epic result to merge', 'Signed. The merge into main is yours, in your terminal.', 'git merge --no-ff --no-edit epic/E-A']);
  // Main holds it: closed (its units too).
  assert.deepEqual(itemsOf(input({ git: withOpen({ ...reviewed, merged: { main: ['epic/E-A'] } }) })), []);
  // Main holds it only because it has no commit of its own yet: still open (W5's fresh epic: a key in merged).
  assert.equal(itemsOf(input({ git: withOpen({ ...reviewed, merged: { main: ['epic/E-A'], 'epic/E-A': [] } }, new Set(['epic/E-A'])) })).length, 1);
});

test('a plan to read enters on the router\'s stage, leaves on plan approved, on its removal, or when the branch moves past the plan\'s tip', () => {
  const stage = (s: Record<string, unknown>) => router({ 'wu/E-A.W1/stage': s });
  const plan = only(itemsOf(input({ router: stage({ stage: 'plan to read', tip: '1111111' }) })));
  assert.deepEqual([plan.id, plan.kind, plan.group, plan.title, plan.branch, plan.tip, plan.since, plan.open, plan.markable], ['plan/E-A.W1@1111111', 'plan', 'yours', 'Plan to read', 'wu/E-A/w1-reader', '1111111', '2026-11-22T08:20:00Z', 'page', true]);
  assert.equal(plan.why, 'The router waits for your word on the plan.');
  assert.deepEqual(itemsOf(input({ router: stage({ stage: 'plan approved', tip: '1111111' }) })), []);
  assert.deepEqual(itemsOf(input({ router: router() })), []);
  assert.deepEqual(itemsOf(input({ router: stage({ stage: 'plan to read', tip: '2222222' }) })), [], 'the branch moved past the plan');
  // A plan whose branch is not here stays (the router said so), with no time.
  const away = only(itemsOf(input({ router: router({ 'wu/E-A.W7/stage': { stage: 'plan to read', tip: 'abcdef0' } }) })));
  assert.deepEqual([away.wu, away.branch, away.since, away.tip], ['E-A.W7', null, null, 'abcdef0']);
});

test('work in motion is not yours: a plan being written, building, in review; each names the worker on its branch; a result on the tip replaces it', () => {
  const w = { 'worker/e-a-w1': { state: 'busy', branch: 'wu/E-A/w1-reader' } };
  for (const [stage, title, why] of [
    ['plan in work', 'Plan being written', 'Its worker writes the plan.'],
    ['building', 'Building', 'Its worker builds it.'],
    ['in review', 'In review', 'A reviewer reads it.'],
  ] as const) {
    const m = only(itemsOf(input({ router: router({ ...w, 'wu/E-A.W1/stage': { stage } }) })));
    assert.deepEqual([m.kind, m.group, m.title, m.why, m.worker, m.wu, m.tip], ['motion', 'not-yours', title, why, 'e-a-w1', 'E-A.W1', '1111111'], stage);
  }
  const reviewed = git({ notes: { mr: [], cr: [W1], dod: [] }, reviews: { [W1]: { verdict: 'PASS', blockers: 0, importants: 0 } } });
  const items = itemsOf(input({ git: reviewed, router: router({ 'wu/E-A.W1/stage': { stage: 'in review', tip: '1111111' } }) }));
  assert.deepEqual(items.map((i) => i.kind), ['result']);
});

test('a question enters on its router line with its lines as text, its work unit if it names a good one, and leaves when the router removes it', () => {
  const q = only(itemsOf(input({ router: router({ 'question/merge-order': { headline: 'Merge W1 first?', lines: ['W2 builds on it.'], wu: 'E-A.W1' } }) })));
  assert.deepEqual([q.kind, q.group, q.title, q.why, q.text, q.wu, q.open, q.markable], ['question', 'yours', 'Question from the router', 'Merge W1 first?', ['W2 builds on it.'], 'E-A.W1', 'text', true]);
  assert.match(q.id, /^question\/merge-order@[0-9a-f]{8}$/);
  const bare = only(itemsOf(input({ router: router({ 'question/q2': { headline: 'Go?', wu: '../x' } }) })));
  assert.deepEqual([bare.text, bare.open, bare.wu], [[], 'none', null]);
  assert.deepEqual(itemsOf(input({ router: router({ 'question/q3': { lines: ['no headline'] } }) })), [], 'a question with no headline is not drawn');
  const other = only(itemsOf(input({ router: router({ 'question/merge-order': { headline: 'Merge W2 first?' } }) })));
  assert.notEqual(other.id, q.id, 'a new question is a new item');
});

test('a worker waiting on the owner is yours, with the attach line as text; a worker not waiting is no item', () => {
  const v = { state: 'waiting', waiting: 'a permission prompt', session: 'b7c1e2d4-5f60-4a1b-9c3d-2e1f00123456', branch: 'wu/E-A/w1-reader', active: '2026-11-22T08:25:00Z' };
  const w = only(itemsOf(input({ router: router({ 'worker/e-a-w1': v }) })));
  assert.deepEqual([w.kind, w.group, w.title, w.why, w.worker, w.line, w.wu, w.branch, w.since], ['worker', 'yours', 'Worker waits on you', 'Waits on a permission prompt: answer it in its session.', 'e-a-w1', 'claude attach b7c1e2d4-5f60-4a1b-9c3d-2e1f00123456', 'E-A.W1', 'wu/E-A/w1-reader', '2026-11-22T08:25:00Z']);
  assert.deepEqual(itemsOf(input({ router: router({ 'worker/e-a-w1': { ...v, waiting: '' } }) })), []);
  const odd = only(itemsOf(input({ router: router({ 'worker/e-a-w1': { ...v, session: 'x; rm -rf /' } }) })));
  assert.equal(odd.line, null, 'a session that is not an id gives no line');
  assert.equal(only(itemsOf(input({ router: router({ 'worker/e-a-w1': { ...v, active: '2026-11-22T08:29:00Z' } }) }))).id, w.id, 'activity alone is the same item');
});

test('main not pushed enters while main is ahead of a live upstream, with the push line, and leaves when it matches', () => {
  const ahead = (n: number, over: object = {}) => git({ branches: [{ name: 'main', tip: MAIN, upstream: 'origin/main', ahead: n, behind: 0, gone: false, ...over }] });
  const p = only(itemsOf(input({ git: ahead(1) })));
  assert.deepEqual([p.id, p.kind, p.title, p.why, p.line, p.branch, p.since], ['push/main@999999999999', 'push', 'Main not pushed', 'main is 1 commit ahead of origin/main.', 'git push origin main', 'main', '2026-11-22T08:00:00Z']);
  assert.equal(only(itemsOf(input({ git: ahead(3) }))).why, 'main is 3 commits ahead of origin/main.');
  assert.deepEqual(itemsOf(input({ git: ahead(0) })), []);
  assert.deepEqual(itemsOf(input({ git: ahead(2, { gone: true }) })), []);
  assert.deepEqual(itemsOf(input({ git: ahead(2, { upstream: null }) })), []);
});

test('a task waits on the owner while its tip is outside its open work unit and no busy worker is on it; it leaves once merged', () => {
  const g = (held: string[]) => {
    const f = rawGit({ merged: { main: [], 'epic/E-A': [], 'wu/E-A/w1-reader': held } });
    f.branches.push({ name: 'wu/E-A/w1-reader_probe', tip: TASK, upstream: null, ahead: 0, behind: 0, gone: false });
    f.times[TASK] = '2026-11-22T08:25:00Z';
    return withOpen(f);
  };
  const t = only(itemsOf(input({ git: g([]) })));
  assert.deepEqual([t.kind, t.group, t.title, t.wu, t.branch, t.tip, t.line, t.since], ['task', 'yours', 'Task into its work unit', 'E-A.W1', 'wu/E-A/w1-reader_probe', '3333333', 'git merge --no-ff wu/E-A/w1-reader_probe', '2026-11-22T08:25:00Z']);
  assert.equal(t.why, 'No worker is busy on it. The merge into wu/E-A/w1-reader is yours, in its folder.');
  assert.deepEqual(itemsOf(input({ git: g(['wu/E-A/w1-reader_probe']) })), []);
  assert.deepEqual(itemsOf(input({ git: g([]), router: router({ 'worker/e-a-w1-probe': { state: 'busy', branch: 'wu/E-A/w1-reader_probe' } }) })).map((i) => i.kind), []);
  const idle = itemsOf(input({ git: g([]), router: router({ 'worker/e-a-w1-probe': { state: 'idle', branch: 'wu/E-A/w1-reader_probe' } }) }));
  assert.deepEqual(idle.map((i) => i.kind), ['task']);
  const silent = only(itemsOf(input({ git: g([]), router: router({}, { heard: false }) })));
  assert.match(silent.why, /^The router is not heard, so a worker may still be on it\./);
});

test('when the router is not heard, or has no run, its items are not drawn; what git proves stays', () => {
  const values = {
    'wu/E-A.W1/stage': { stage: 'plan to read', tip: '1111111' },
    'question/q1': { headline: 'Go?', lines: ['x'] },
    'worker/w': { state: 'waiting', waiting: 'a prompt' },
    'wu/E-A.W2/stage': { stage: 'building' },
  };
  const g = git({ branches: [{ name: 'main', tip: MAIN, upstream: 'origin/main', ahead: 1, behind: 0, gone: false }] });
  assert.deepEqual(itemsOf(input({ git: g, router: router(values) })).map((i) => i.kind).sort(), ['motion', 'plan', 'push', 'question', 'worker']);
  assert.deepEqual(itemsOf(input({ git: g, router: router(values, { heard: false }) })).map((i) => i.kind), ['push']);
  assert.deepEqual(itemsOf(input({ git: g, router: router({}, { run: null, n: 0, heard: false }) })).map((i) => i.kind), ['push']);
});

test('a sent mark moves a plan or a question to Sent; it never moves another item, and a moved plan is a new item the mark does not reach', () => {
  const r = router({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: '1111111' }, 'question/q1': { headline: 'Go?', lines: ['x'] } });
  const all = itemsOf(input({ router: r }));
  const ids = all.map((i) => i.id);
  const reviewed = git({ notes: { mr: [], cr: [W1], dod: [] }, reviews: { [W1]: { verdict: 'PASS', blockers: 0, importants: 0 } } });
  const sent = itemsOf(input({ git: reviewed, router: r, sent: new Set([...ids, 'result/E-A.W1@111111111111']) }));
  assert.deepEqual(sent.map((i) => [i.kind, i.group]), [['result', 'yours'], ['question', 'sent'], ['plan', 'sent']]);
  assert.equal(sent.find((i) => i.kind === 'plan')?.why, 'You marked it sent: it leaves when the router moves the stage on.');
  assert.equal(sent.find((i) => i.kind === 'question')?.why, 'You marked it sent: it leaves when the router removes the question.');
  const moved = itemsOf(input({ git: git({ branches: [...rawGit().branches.filter((b) => b.name !== 'wu/E-A/w1-reader'), { name: 'wu/E-A/w1-reader', tip: W2, upstream: null, ahead: 0, behind: 0, gone: false }] }), router: router({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: '2222222' } }), sent: new Set(ids) }));
  assert.deepEqual(moved.map((i) => i.group), ['yours']);
});

test('the list is ordered yours, not yours, sent; within a group by kind (results first, motion last), then by id', () => {
  const g = git({
    branches: [
      { name: 'main', tip: MAIN, upstream: 'origin/main', ahead: 1, behind: 0, gone: false },
      { name: 'epic/E-A', tip: EPIC, upstream: null, ahead: 0, behind: 0, gone: false },
      { name: 'wu/E-A/w1-reader', tip: W1, upstream: null, ahead: 0, behind: 0, gone: false },
      { name: 'wu/E-A/w2-writer', tip: W2, upstream: null, ahead: 0, behind: 0, gone: false },
    ],
    notes: { mr: [], cr: [W1, EPIC], dod: [] },
    reviews: { [W1]: { verdict: 'PASS', blockers: 0, importants: 0 }, [EPIC]: { verdict: 'FAIL', blockers: 1, importants: 0 } },
  });
  const r = router({
    'wu/E-A.W2/stage': { stage: 'building' },
    'question/b': { headline: 'B?' },
    'question/a': { headline: 'A?' },
    'worker/w': { state: 'waiting', waiting: 'a prompt' },
  });
  const items = itemsOf(input({ git: g, router: r }));
  assert.deepEqual(items.map((i) => `${i.group}:${i.kind}`), ['yours:result', 'yours:question', 'yours:question', 'yours:worker', 'yours:push', 'not-yours:epic-result', 'not-yours:motion']);
  assert.ok((items[1] as Item).id < (items[2] as Item).id);
  assert.deepEqual(countsOf(items), { yours: 5, notYours: 2, sent: 0 });
});

// ---- generated views ----

const NAMES = ['main', 'epic/E-A', 'epic/E-B', 'wu/E-A/w1-a', 'wu/E-A/w2-b', 'wu/E-B/w1-c', 'wu/E-A/w1-a_task', 'wu/E-B/w1-c_fix', 'wu/E-C/w3', 'feature/x', 'epic/e-lower', 'wu/E-A/odd'];
const VERDICTS = ['PASS', 'FAIL', 'WAIVED', null] as const;
const STAGES = ['plan in work', 'plan to read', 'plan approved', 'building', 'in review', 'nonsense', 7];

function tipOf(rng: Rng): string {
  return Array.from({ length: 40 }, () => pick(rng, [...'0123456789abcdef'])).join('');
}

function generated(rng: Rng): ItemInput {
  const names = NAMES.filter(() => rng() < 0.7);
  const branches = names.map((name) => ({ name, tip: tipOf(rng), upstream: rng() < 0.5 ? 'origin/x' : null, ahead: Math.floor(rng() * 3), behind: 0, gone: rng() < 0.1 }));
  const tips = branches.map((b) => b.tip);
  const some = (xs: string[]) => xs.filter(() => rng() < 0.4);
  const cr = some(tips);
  const reviews = Object.fromEntries(cr.filter(() => rng() < 0.9).map((t) => [t, { verdict: pick(rng, VERDICTS), blockers: Math.floor(rng() * 3), importants: Math.floor(rng() * 3) }]));
  const merged: Record<string, string[]> = {};
  if (names.includes('main')) merged.main = some(names.filter((n) => n.startsWith('epic/')));
  for (const n of names) if (n.startsWith('epic/') || (n.startsWith('wu/') && rng() < 0.5)) merged[n] = some(names.filter((x) => x.startsWith('wu/')));
  const values: Record<string, unknown> = {};
  for (const wu of ['E-A.W1', 'E-A.W2', 'E-B.W1', 'E-C.W3', 'E-Z.W9']) {
    if (rng() < 0.5) values[`wu/${wu}/stage`] = rng() < 0.8 ? { stage: pick(rng, STAGES), ...(rng() < 0.7 ? { tip: pick(rng, tips.length > 0 ? tips : ['abcdef0']).slice(0, 7 + Math.floor(rng() * 5)) } : {}) } : 'junk';
  }
  for (const q of ['q1', 'merge-order']) if (rng() < 0.4) values[`question/${q}`] = rng() < 0.8 ? { headline: pick(rng, ['Go?', '', 'Merge?']), lines: ['x'], wu: pick(rng, ['E-A.W1', '../x', null]) } : [1, 2];
  for (const w of ['w-a', 'w-b']) if (rng() < 0.5) values[`worker/${w}`] = { state: pick(rng, ['busy', 'idle', 'waiting']), waiting: pick(rng, ['', 'a prompt', 3]), branch: pick(rng, names.length > 0 ? names : ['main']), session: pick(rng, ['abc-123', 'x y']) };
  const heard = rng() < 0.7;
  const run = rng() < 0.85 ? 'keel-1c.20261122-080000.1' : null;
  const fresh = new Set(names.filter((n) => n.startsWith('epic/') && rng() < 0.2));
  const g: GitFacts = withOpen({ branches, notes: { mr: some(cr), cr, dod: [] }, merged, worktrees: [], reviews, times: Object.fromEntries(some(tips).map((t) => [t, '2026-11-22T08:00:00Z'])), problem: null }, fresh);
  return { git: rng() < 0.95 ? g : null, router: { run, n: run === null ? 0 : 5, values: run === null ? {} : values, heard: run !== null && heard, lastAt: null, problem: null }, sent: new Set() };
}

test('generated views: every item id is unique, well formed and within the cap; the list is the same for the same facts in any branch order; groups are ordered and counted', () => {
  const rng = seeded(0x6e17);
  const re = new RegExp(ITEM_ID_SOURCE);
  const order = ['yours', 'not-yours', 'sent'];
  for (let i = 0; i < 400; i++) {
    const inp = generated(rng);
    const items = itemsOf(inp);
    const ids = items.map((x) => x.id);
    assert.equal(new Set(ids).size, ids.length, `unique: ${ids.join(' ')}`);
    for (const id of ids) assert.ok(re.test(id) && id.length <= SLOT_CAP, id);
    assert.deepEqual(itemsOf(inp), items, 'deterministic');
    if (inp.git !== null) {
      const shuffled = { ...inp, git: { ...inp.git, branches: [...inp.git.branches].reverse() } };
      assert.deepEqual(itemsOf(shuffled), items, 'branch order does not matter');
    }
    for (let k = 1; k < items.length; k++) assert.ok(order.indexOf((items[k] as Item).group) >= order.indexOf((items[k - 1] as Item).group), 'groups ordered');
    const c = countsOf(items);
    assert.equal(c.yours + c.notYours + c.sent, items.length);
  }
});

test('generated views: every item names a branch the view lists, or a key the router set; no router item while the router is not heard; marks move only markable items', () => {
  const rng = seeded(0x51e7);
  for (let i = 0; i < 400; i++) {
    const inp = generated(rng);
    const first = itemsOf(inp);
    const marked = new Set(first.filter(() => rng() < 0.5).map((x) => x.id));
    const items = itemsOf({ ...inp, sent: marked });
    const names = new Set((inp.git?.branches ?? []).map((b) => b.name));
    const workerBranches = new Set(Object.entries(inp.router.values).filter(([k]) => k.startsWith('worker/')).map(([, v]) => (v as { branch?: unknown }).branch));
    for (const x of items) {
      if (x.kind === 'plan' || x.kind === 'question' || x.kind === 'worker' || x.kind === 'motion') {
        assert.ok(inp.router.heard && inp.router.run !== null, `${x.id} drawn while the router is not heard`);
      }
      if (x.branch !== null) assert.ok(names.has(x.branch) || (x.kind === 'worker' && workerBranches.has(x.branch)), `${x.id} names ${x.branch}`);
      if (x.kind === 'question') assert.ok(Object.hasOwn(inp.router.values, `question/${x.id.slice('question/'.length).split('@')[0]}`), x.id);
      assert.equal(x.group === 'sent', x.markable && marked.has(x.id), x.id);
    }
    // Marks change groups only: the same items, the same ids.
    assert.deepEqual(items.map((x) => x.id).sort(), first.map((x) => x.id).sort());
  }
});

test('W6 review round 1 (items.ts:82): the list draws only the reader\'s open sets; a branch they leave out makes no item, whatever the merged reads say', () => {
  const g = rawGit({ notes: { mr: [], cr: [W1, EPIC], dod: [] }, reviews: { [W1]: { verdict: 'PASS', blockers: 0, importants: 0 }, [EPIC]: { verdict: 'PASS', blockers: 0, importants: 0 } } });
  g.branches.push({ name: 'wu/E-A/w1-reader_probe', tip: TASK, upstream: null, ahead: 0, behind: 0, gone: false });
  assert.deepEqual(itemsOf(input({ git: { ...g, open: { epics: [], units: [], tasks: [] } } })), []);
  const all = itemsOf(input({ git: { ...g, open: { epics: ['epic/E-A'], units: ['wu/E-A/w1-reader'], tasks: ['wu/E-A/w1-reader_probe'] } } }));
  assert.deepEqual(all.map((i) => i.kind), ['result', 'epic-result', 'task']);
});

/** What a POSIX shell makes of one word: plain characters as they are, '...' literally, \\x as x. */
function shellWord(word: string): string | null {
  let out = '';
  for (let i = 0; i < word.length; i++) {
    const c = word[i] as string;
    if (c === "'") {
      const end = word.indexOf("'", i + 1);
      if (end < 0) return null;
      out += word.slice(i + 1, end);
      i = end;
    } else if (c === '\\') {
      out += word[i + 1] ?? '';
      i += 1;
    } else if (/[A-Za-z0-9._\/@%+=:,-]/.test(c)) out += c;
    else return null;
  }
  return out;
}

test('W6 review round 1 (items.ts:167): a name in a line the owner pastes is one shell word that means exactly the name; a plain name stays as it is', () => {
  assert.equal(shQuote('wu/E-A/w1-reader_probe'), 'wu/E-A/w1-reader_probe');
  assert.equal(shQuote('wu/E-X/w1-a_$(touch${IFS}p)'), "'wu/E-X/w1-a_$(touch${IFS}p)'");
  assert.equal(shQuote("it's"), "'it'\\''s'");
});

test('W6 review round 1: generated names: shQuote always gives one shell word holding the name', () => {
  const rng = seeded(0x7a11);
  const alphabet = ['a', 'Z', '0', '/', '_', '-', '.', '$', '(', ')', '{', '}', ';', '|', '&', '`', ' ', "'", '"', '\\', '*', '?', '~', '!', '#', '<', '>', '\n', 'é'];
  for (let i = 0; i < 3000; i++) {
    const n = 1 + Math.floor(rng() * 24);
    let name = '';
    for (let k = 0; k < n; k++) name += pick(rng, alphabet);
    assert.equal(shellWord(shQuote(name)), name, JSON.stringify(name));
  }
});

test('W6 review round 1 (items.ts:167): a task or result line quotes its branch, and the push line its remote, should the facts ever carry a name the shell reads', () => {
  const evil = 'wu/E-A/w1-reader_$(touch${IFS}p)';
  const g = rawGit({ merged: { main: [], 'epic/E-A': [], 'wu/E-A/w1-reader': [] } });
  g.branches.push({ name: evil, tip: TASK, upstream: null, ahead: 0, behind: 0, gone: false });
  g.branches[0] = { name: 'main', tip: MAIN, upstream: 'o$(x)/main', ahead: 1, behind: 0, gone: false };
  const items = itemsOf(input({ git: { ...g, open: { epics: ['epic/E-A'], units: ['wu/E-A/w1-reader'], tasks: [evil] } } }));
  assert.equal(items.find((i) => i.kind === 'task')?.line, `git merge --no-ff '${evil}'`);
  assert.equal(items.find((i) => i.kind === 'push')?.line, "git push 'o$(x)' main");
  for (const i of items) if (i.line !== null) for (const word of i.line.split(/ (?=(?:[^']*'[^']*')*[^']*$)/)) assert.notEqual(shellWord(word), null, i.line);
});

test('W6 review round 1 (items.ts:115): two branches of one work unit on the same reviewed tip are two items with two ids', () => {
  const g = rawGit({ notes: { mr: [], cr: [W1], dod: [] }, reviews: { [W1]: { verdict: 'PASS', blockers: 0, importants: 0 } } });
  g.branches.push({ name: 'wu/E-A/w1-other', tip: W1, upstream: null, ahead: 0, behind: 0, gone: false });
  const items = itemsOf(input({ git: withOpen(g) }));
  assert.deepEqual(items.map((i) => i.branch).sort(), ['wu/E-A/w1-other', 'wu/E-A/w1-reader']);
  assert.equal(new Set(items.map((i) => i.id)).size, 2);
  for (const i of items) assert.match(i.id, /^result\/E-A\.W1-[0-9a-f]{8}@111111111111$/);
});

test('W6 review round 1 (open.ts:148): a plan staged with no tip is listed, markable, but offers no Open: there is no tip to open it at', () => {
  const plan = only(itemsOf(input({ router: router({ 'wu/E-A.W1/stage': { stage: 'plan to read' } }) })));
  assert.deepEqual([plan.kind, plan.tip, plan.open, plan.markable], ['plan', null, 'none', true]);
  assert.equal(plan.why, 'The router waits for your word on the plan. It named no tip, so there is no page to open: ask it for the plan.');
});

test('W6 review round 2 (items.ts:103): an open unit or epic whose name carries no keel id (wu/E-A/reader, epic/foo) is listed with a true reason and no Open, never an Open that fails', () => {
  const OTHER = T('4');
  const g = rawGit({ notes: { mr: [], cr: [OTHER, T('5')], dod: [] }, reviews: { [OTHER]: { verdict: 'PASS', blockers: 0, importants: 0 }, [T('5')]: { verdict: 'PASS', blockers: 0, importants: 0 } } });
  g.branches.push({ name: 'wu/E-A/reader', tip: OTHER, upstream: null, ahead: 0, behind: 0, gone: false });
  g.branches.push({ name: 'epic/foo', tip: T('5'), upstream: null, ahead: 0, behind: 0, gone: false });
  g.merged = { main: [], 'epic/E-A': [], 'epic/foo': [] };
  const items = itemsOf(input({ git: withOpen(g) }));
  const unit = items.find((i) => i.branch === 'wu/E-A/reader') as Item;
  assert.deepEqual([unit.kind, unit.wu, unit.open, unit.title], ['result', null, 'none', 'Result to sign']);
  assert.equal(unit.why, 'The review passed. Your sign-off is missing. Its branch name carries no work unit number (wu/<epic>/w<n>-...), so Keel cannot find its result page: open it from its folder.');
  const epic = items.find((i) => i.branch === 'epic/foo') as Item;
  assert.deepEqual([epic.kind, epic.epic, epic.open], ['epic-result', null, 'none']);
  assert.match(epic.why, /Its branch name carries no epic id \(epic\/E-<CODE>\), so Keel cannot find its result page/);
});
