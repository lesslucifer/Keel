// The preload's bridge factory, built from an injected ipc object: what the window gets as window.keel.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeBridge, type IpcLike } from '../../src/preload/bridge.ts';

type Listener = (event: unknown, message: unknown) => void;

/** An ipcRenderer stand-in: records invokes, holds listeners per channel, and can emit as the main part would. */
function fakeIpc() {
  const invoked: Array<[string, unknown]> = [];
  const listeners = new Map<string, Listener[]>();
  const ipc: IpcLike = {
    invoke: async (channel, payload) => {
      invoked.push([channel, payload]);
      return { ok: true, value: 'answer' };
    },
    on: (channel, listener) => {
      listeners.set(channel, [...(listeners.get(channel) ?? []), listener]);
    },
    removeListener: (channel, listener) => {
      listeners.set(channel, (listeners.get(channel) ?? []).filter((l) => l !== listener));
    },
  };
  // The first argument stands for Electron's IpcRendererEvent, which holds ipcRenderer itself: it must never reach the window.
  const emit = (channel: string, message: unknown) => {
    for (const l of listeners.get(channel) ?? []) l({ sender: 'ipcRenderer' }, message);
  };
  const count = (channel: string) => (listeners.get(channel) ?? []).length;
  return { ipc, invoked, emit, count };
}

test('the bridge exposes exactly ask and on, frozen', () => {
  const keel = makeBridge(fakeIpc().ipc);
  assert.deepEqual(Object.keys(keel).sort(), ['ask', 'on']);
  assert.ok(Object.isFrozen(keel));
});

test('ask(name, args) sends {name, args} on the request channel and hands back the main part\'s answer', async () => {
  const { ipc, invoked } = fakeIpc();
  const keel = makeBridge(ipc);
  assert.deepEqual(await keel.ask('settings.choose', { tool: 'git' }), { ok: true, value: 'answer' });
  assert.deepEqual(await keel.ask('state.get', {}), { ok: true, value: 'answer' });
  assert.deepEqual(invoked, [
    ['keel:ask', { name: 'settings.choose', args: { tool: 'git' } }],
    ['keel:ask', { name: 'state.get', args: {} }],
  ]);
});

test('ask() leaves the checking to the main part: whatever the window passes goes as {name, args} and nothing else', async () => {
  const { ipc, invoked } = fakeIpc();
  await makeBridge(ipc).ask('shell.run', { cmd: 'rm' });
  assert.deepEqual(invoked, [['keel:ask', { name: 'shell.run', args: { cmd: 'rm' } }]]);
});

test('on() refuses an event name not in the push list, and listens to nothing', () => {
  const { ipc, count } = fakeIpc();
  const keel = makeBridge(ipc);
  for (const name of ['state.get', 'keel:push', '__proto__', 'toString', '', 'State.changed']) {
    assert.throws(() => keel.on(name, () => undefined), /push event/, name);
  }
  assert.throws(() => keel.on('state.changed', 'not a function' as never), /function/);
  assert.equal(count('keel:push'), 0);
});

test('on() calls back only for its event, with the payload alone (never the ipc event), and returns an unsubscribe', () => {
  const { ipc, emit, count } = fakeIpc();
  const keel = makeBridge(ipc);
  const got: unknown[][] = [];
  const off = keel.on('state.changed', (...args: unknown[]) => got.push(args));
  assert.equal(typeof off, 'function');
  emit('keel:push', { event: 'state.changed', payload: null });
  emit('keel:push', { event: 'other.event', payload: null });
  emit('keel:push', 'garbage');
  emit('keel:push', null);
  emit('keel:other', { event: 'state.changed', payload: null });
  assert.deepEqual(got, [[null]]);
  off();
  off();
  assert.equal(count('keel:push'), 0);
  emit('keel:push', { event: 'state.changed', payload: null });
  assert.deepEqual(got, [[null]]);
});

test('on() hands each callback its own copy of the payload', () => {
  const { ipc, emit } = fakeIpc();
  const keel = makeBridge(ipc);
  const seen: unknown[] = [];
  keel.on('state.changed', (p: unknown) => seen.push(p));
  keel.on('state.changed', (p: unknown) => seen.push(p));
  const payload = { list: ['a'] };
  emit('keel:push', { event: 'state.changed', payload });
  assert.equal(seen.length, 2);
  assert.deepEqual(seen[0], payload);
  assert.notEqual(seen[0], payload);
  assert.notEqual(seen[0], seen[1]);
});

test('two subscriptions unsubscribe separately', () => {
  const { ipc, emit, count } = fakeIpc();
  const keel = makeBridge(ipc);
  let a = 0;
  let b = 0;
  const offA = keel.on('state.changed', () => { a++; });
  keel.on('state.changed', () => { b++; });
  offA();
  emit('keel:push', { event: 'state.changed', payload: null });
  assert.deepEqual([a, b, count('keel:push')], [0, 1, 1]);
});
