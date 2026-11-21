import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { ALERT_TITLE, ALERT_BODY, ALERT_WAIT_MS, testAlert } from '../../src/main/alert.ts';

/** A Notification stand-in: show() emits 'show' at once, later, or never. */
function fake(mode: 'at-once' | 'never' | 'throws') {
  const n = new EventEmitter() as EventEmitter & { show(): void; shown: number };
  n.shown = 0;
  n.show = () => {
    n.shown++;
    if (mode === 'throws') throw new Error('no notification centre');
    if (mode === 'at-once') queueMicrotask(() => n.emit('show'));
  };
  return n;
}

test('the test alert is titled Keel and says alerts work; it waits 5 s to be shown', () => {
  assert.equal(ALERT_TITLE, 'Keel');
  assert.equal(ALERT_BODY, 'Test alert: alerts work on this Mac.');
  assert.equal(ALERT_WAIT_MS, 5_000);
});

test('an alert macOS shows answers shown', async () => {
  const n = fake('at-once');
  assert.equal(await testAlert(true, () => n), 'shown');
  assert.equal(n.shown, 1);
});

test('an alert not shown within the wait answers not-shown', async () => {
  assert.equal(await testAlert(true, () => fake('never'), 50), 'not-shown');
});

test('an alert whose show() throws answers not-shown', async () => {
  assert.equal(await testAlert(true, () => fake('throws'), 50), 'not-shown');
});

test('where notifications are not supported nothing is made and the answer is not-supported', async () => {
  let made = false;
  assert.equal(await testAlert(false, () => { made = true; return fake('at-once'); }), 'not-supported');
  assert.equal(made, false);
});
