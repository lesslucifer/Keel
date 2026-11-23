// The sent marks' rules (src/main/items/sent.ts) on fabricated records: the file's shape, a mark set and
// taken back, the marks of one project, the bound, and the clean-up of marks whose item has left.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseSent, emptySent, marked, withMark, SENT_CAP } from '../../src/main/items/sent.ts';
import { seeded, pick } from '../support/seeded.ts';

const P = 'p0a1b2c3d';
const Q = 'p11111111';

test('the record reads back only in its own shape: version 1, marks of a project id and an item id', () => {
  assert.deepEqual(parseSent({ version: 1, marks: [{ project: P, item: 'plan/E-A.W1@abcdef0' }] }), { version: 1, marks: [{ project: P, item: 'plan/E-A.W1@abcdef0' }] });
  for (const v of [null, [], 'x', { version: 2, marks: [] }, { version: 1 }, { version: 1, marks: [{ project: P }] }, { version: 1, marks: [{ project: 'x', item: 'plan/E-A.W1@abcdef0' }] }, { version: 1, marks: [{ project: P, item: '../../etc' }] }]) {
    assert.equal(parseSent(v), null, JSON.stringify(v));
  }
  assert.deepEqual(emptySent(), { version: 1, marks: [] });
});

test('a mark is set once and taken back; marks are per project', () => {
  let r = withMark(emptySent(), P, 'plan/E-A.W1@abcdef0', true, new Set(['plan/E-A.W1@abcdef0']));
  r = withMark(r, P, 'plan/E-A.W1@abcdef0', true, new Set(['plan/E-A.W1@abcdef0']));
  r = withMark(r, Q, 'question/q1@a1b2c3d4', true, new Set(['question/q1@a1b2c3d4']));
  assert.deepEqual([...marked(r, P)], ['plan/E-A.W1@abcdef0']);
  assert.deepEqual([...marked(r, Q)], ['question/q1@a1b2c3d4']);
  r = withMark(r, P, 'plan/E-A.W1@abcdef0', false, new Set(['plan/E-A.W1@abcdef0']));
  assert.deepEqual([...marked(r, P)], []);
  assert.deepEqual([...marked(r, Q)], ['question/q1@a1b2c3d4'], 'the other project keeps its marks');
});

test('a write drops this project\'s marks whose item is no longer listed; another project\'s stay', () => {
  let r = withMark(emptySent(), P, 'plan/E-A.W1@abcdef0', true, new Set(['plan/E-A.W1@abcdef0']));
  r = withMark(r, Q, 'plan/E-B.W1@abcdef0', true, new Set(['plan/E-B.W1@abcdef0']));
  r = withMark(r, P, 'question/q1@a1b2c3d4', true, new Set(['question/q1@a1b2c3d4']));
  assert.deepEqual([...marked(r, P)], ['question/q1@a1b2c3d4']);
  assert.deepEqual([...marked(r, Q)], ['plan/E-B.W1@abcdef0']);
});

test('generated marks: the record never holds more than SENT_CAP marks, never a mark twice, and keeps the newest', () => {
  const rng = seeded(0x5e17);
  let r = emptySent();
  const projects = [P, Q, 'p22222222'];
  for (let i = 0; i < 2000; i++) {
    const project = pick(rng, projects);
    const item = `plan/E-A.W${Math.floor(rng() * 400)}@abcdef0`;
    const live = new Set([...marked(r, project), item]);
    r = withMark(r, project, item, rng() < 0.85, live);
    assert.ok(r.marks.length <= SENT_CAP);
    const keys = r.marks.map((m) => `${m.project} ${m.item}`);
    assert.equal(new Set(keys).size, keys.length);
  }
  const last = withMark(r, P, 'question/newest@a1b2c3d4', true, new Set([...marked(r, P), 'question/newest@a1b2c3d4']));
  assert.ok(marked(last, P).has('question/newest@a1b2c3d4'));
});
