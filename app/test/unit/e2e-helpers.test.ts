// The epic test's pure helpers (app/e2e/world.ts, E-APPLIST.W9): the sink line reader the walk checks
// the alert and open sinks with, and the wait it looks for each step's outcome with.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseLines, waitFor, onPath } from '../../e2e/world.ts';
import { seeded, pick } from '../support/seeded.ts';

test('a sink\'s whole lines are read in order; blank lines are skipped; a last line not yet ended is not read', () => {
  assert.deepEqual(parseLines(''), []);
  assert.deepEqual(parseLines('{"a":1}\n\n{"b":2}\n'), [{ a: 1 }, { b: 2 }]);
  assert.deepEqual(parseLines('{"a":1}\n{"b":'), [{ a: 1 }]);
  assert.deepEqual(parseLines('{"b":2'), []);
});

test('a whole line that is not one JSON object fails the read, naming it', () => {
  assert.throws(() => parseLines('[1,2]\n'), /a sink line that is not a JSON object: \[1,2\]/);
  assert.throws(() => parseLines('null\n'), /not a JSON object: null/);
  assert.throws(() => parseLines('{"a":\n'), SyntaxError);
});

test('property: however a run of appended lines is cut, the read gives exactly the lines written whole before the cut', () => {
  const rand = seeded(0xe2e1);
  for (let round = 0; round < 300; round += 1) {
    const n = Math.floor(rand() * 6);
    const lines = Array.from({ length: n }, (_, i) => ({ n: i, title: pick(rand, ['Plan to read: E-A.W1', 'Which unit merges first?', 'é"\\\u2028']) }));
    const text = lines.map((l) => `${JSON.stringify(l)}\n`).join('');
    const cut = Math.floor(rand() * (text.length + 1));
    const whole = text.slice(0, cut).split('\n').length - 1;
    assert.deepEqual(parseLines(text.slice(0, cut)), lines.slice(0, whole), `round ${round}, cut ${cut}`);
  }
});

test('a wait answers the first value its probe gives, and fails naming what it waited for when the time runs out', async () => {
  let calls = 0;
  assert.equal(await waitFor('the third look', 1000, () => (++calls >= 3 ? 'seen' : undefined), 1), 'seen');
  assert.equal(calls, 3);
  assert.equal(await waitFor('an async probe', 1000, async () => 7, 1), 7);
  for (const nothing of [undefined, null, false]) {
    const t0 = Date.now();
    await assert.rejects(waitFor('the plan to read', 30, () => nothing, 5), /^Error: the plan to read: not seen within 30 ms$/);
    assert.ok(Date.now() - t0 >= 30);
  }
  // 0 and '' are answers, not nothing.
  assert.equal(await waitFor('zero', 100, () => 0, 1), 0);
  assert.equal(await waitFor('empty', 100, () => '', 1), '');
});

test('a program is found on the PATH given, by absolute folders only; none says which', () => {
  assert.equal(onPath('sh', '/bin'), '/bin/sh');
  assert.equal(onPath('sh', 'bin:/nowhere:/bin'), '/bin/sh');
  assert.throws(() => onPath('sh', 'bin:/nowhere'), /no sh on PATH/);
});
