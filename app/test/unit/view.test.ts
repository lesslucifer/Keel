// The view's starting point: what waiting.get answers before the reader has read anything.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { emptyView } from '../../src/main/reader/view.ts';
import { plainDeep } from '../../src/main/bridge.ts';

test('the empty view has no project, no git, no router run, is not heard, holds no alert, focus or item, and is plain data', () => {
  const v = emptyView();
  assert.deepEqual(v, {
    project: null,
    git: null,
    router: { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null },
    alerts: [],
    focus: null,
    scheme: null,
    readAt: null,
    changedAt: null,
    items: [],
    counts: { yours: 0, notYours: 0, sent: 0 },
  });
  assert.deepEqual(plainDeep(v), v);
});

test('each empty view is a fresh object: changing one leaves the next as it was', () => {
  const a = emptyView();
  a.router.values.router = { name: 'x' };
  a.alerts.push({ n: 1, at: '2026-11-22T09:00:00+08:00', headline: 'h', lines: ['l'], wu: null, link: null, shown: false });
  assert.deepEqual(emptyView().router.values, {});
  assert.deepEqual(emptyView().alerts, []);
  a.counts.yours = 3;
  assert.deepEqual(emptyView().counts, { yours: 0, notYours: 0, sent: 0 });
});
