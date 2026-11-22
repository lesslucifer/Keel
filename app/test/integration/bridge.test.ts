// The window's wire contract at its real boundary: the whole request table through the dispatcher
// the shell hands to ipcMain.handle, against senders shaped as Electron records them; and the push
// side through the pusher, against recorded windows.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dispatcher, pusher, contract, type EventLike, type Expected, type Handlers, type WindowLike } from '../../src/main/bridge.ts';
import { REQUESTS, PUSH_EVENTS, ASK_CHANNEL, PUSH_CHANNEL } from '../../src/shared/contract.ts';

const PAGE = 'app://keel/index.html';
const ITEM = '^(?:result|epic-result|plan|question|worker|task|push|motion)/[A-Za-z0-9._-]{1,41}@[0-9a-f]{7,12}$';
const expected: Expected = { contentsId: 3, pages: [PAGE] };
const own: EventLike = { sender: { id: 3 }, senderFrame: { url: PAGE, parent: null } };

function recorded() {
  const calls: Array<[string, Record<string, string>]> = [];
  const h = (name: string) => (args: Record<string, string>) => {
    calls.push([name, args]);
    return { did: name };
  };
  const handlers: Handlers = {
    'state.get': h('state.get'),
    'projects.add': h('projects.add'),
    'projects.remove': h('projects.remove'),
    'settings.choose': h('settings.choose'),
    'settings.reset': h('settings.reset'),
    'alert.test': h('alert.test'),
    'waiting.get': h('waiting.get'),
    'item.open': h('item.open'),
    'item.mark': h('item.mark'),
  };
  return { calls, ask: dispatcher(() => expected, handlers) };
}

const GOOD: Array<[string, Record<string, string>]> = [
  ['state.get', {}],
  ['projects.add', {}],
  ['projects.remove', { project: 'p00c0ffee' }],
  ['settings.choose', { tool: 'keel' }],
  ['settings.choose', { tool: 'git' }],
  ['settings.choose', { tool: 'claude' }],
  ['settings.reset', { tool: 'keel' }],
  ['settings.reset', { tool: 'git' }],
  ['settings.reset', { tool: 'claude' }],
  ['item.open', { item: 'result/E-APPLIST.W6@0f1e2d3' }],
  ['item.open', { item: 'question/merge-order@a1b2c3d4' }],
  ['item.mark', { item: 'plan/E-APPLIST.W6@0f1e2d3c4b5a', sent: 'yes' }],
  ['item.mark', { item: 'plan/E-APPLIST.W6@0f1e2d3c4b5a', sent: 'no' }],
  ['alert.test', {}],
  ['waiting.get', {}],
];

test('the wire contract is locked: two channels, nine request names with their slots, two push events (a change here changes what the window may do)', () => {
  assert.equal(ASK_CHANNEL, 'keel:ask');
  assert.equal(PUSH_CHANNEL, 'keel:push');
  assert.deepEqual(contract(), {
    requests: {
      'alert.test': {},
      'item.mark': { item: `item-id ${ITEM}`, sent: 'one of yes|no' },
      'item.open': { item: `item-id ${ITEM}` },
      'projects.add': {},
      'projects.remove': { project: 'project-id ^p[0-9a-f]{8}$' },
      'settings.choose': { tool: 'one of keel|git|claude' },
      'settings.reset': { tool: 'one of keel|git|claude' },
      'state.get': {},
      'waiting.get': {},
    },
    push: { 'state.changed': 'none', 'waiting.changed': 'none' },
  });
  assert.deepEqual(Object.keys(REQUESTS).sort(), Object.keys(contract().requests));
  assert.deepEqual(Object.keys(PUSH_EVENTS).sort(), Object.keys(contract().push));
});

test('every name with every good slot value reaches its handler once, with exactly its args', async () => {
  const { calls, ask } = recorded();
  for (const [name, args] of GOOD) {
    assert.deepEqual(await ask(own, structuredClone({ name, args })), { ok: true, value: { did: name } });
  }
  assert.deepEqual(calls, GOOD);
});

test('refused calls run nothing: an unknown name, a foreign page, a subframe, another window, a missing frame, free text, an extra key', async () => {
  const { calls, ask } = recorded();
  const cases: Array<[EventLike, unknown, string]> = [
    [own, { name: 'shell.open', args: {} }, 'unknown-name'],
    [{ sender: { id: 3 }, senderFrame: { url: 'https://evil.example/', parent: null } }, { name: 'state.get', args: {} }, 'other-page'],
    [{ sender: { id: 3 }, senderFrame: { url: PAGE, parent: { url: PAGE } } }, { name: 'state.get', args: {} }, 'not-main-frame'],
    [{ sender: { id: 4 }, senderFrame: { url: PAGE, parent: null } }, { name: 'state.get', args: {} }, 'other-window'],
    [{ sender: { id: 3 }, senderFrame: null }, { name: 'state.get', args: {} }, 'no-frame'],
    [own, { name: 'settings.choose', args: { tool: '/opt/homebrew/bin/git' } }, 'bad-slot:tool'],
    [own, { name: 'settings.choose', args: { tool: 7 } }, 'bad-slot:tool'],
    [own, { name: 'projects.remove', args: { project: 'p00c0ffee', path: '/' } }, 'bad-args'],
    [own, { name: 'alert.test', args: { body: 'hi' } }, 'bad-args'],
    [own, { name: 'item.open', args: { item: '/Users/owner/code/keel/docs/x.html' } }, 'bad-slot:item'],
    [own, { name: 'item.open', args: { item: 'result/../../etc@0f1e2d3' } }, 'bad-slot:item'],
    [own, { name: 'item.open', args: { item: 'shell/E-APPLIST.W6@0f1e2d3' } }, 'bad-slot:item'],
    [own, { name: 'item.mark', args: { item: 'plan/E-APPLIST.W6@0f1e2d3', sent: 'maybe' } }, 'bad-slot:sent'],
    [own, { name: 'item.mark', args: { item: 'plan/E-APPLIST.W6@0f1e2d3' } }, 'bad-args'],
  ];
  for (const [event, call, refused] of cases) {
    assert.deepEqual(await ask(event, structuredClone(call)), { ok: false, refused }, refused);
  }
  assert.deepEqual(calls, []);
});

test('with no window open every call is refused and nothing runs', async () => {
  let ran = false;
  const handlers = Object.fromEntries(Object.keys(REQUESTS).map((n) => [n, () => { ran = true; }])) as unknown as Handlers;
  const ask = dispatcher(() => null, handlers);
  assert.deepEqual(await ask(own, { name: 'state.get', args: {} }), { ok: false, refused: 'no-window' });
  assert.equal(ran, false);
});

test('every string a handler answers reaches the window through plain()', async () => {
  const handlers = Object.fromEntries(Object.keys(REQUESTS).map((n) => [n, () => ({ why: 'bad\u202e name\n', list: ['a\u0007'] })])) as unknown as Handlers;
  const ask = dispatcher(() => expected, handlers);
  assert.deepEqual(await ask(own, { name: 'state.get', args: {} }), { ok: true, value: { why: 'bad  name ', list: ['a '] } });
});

test('a handler that throws answers failed with its message made plain, and never throws to the window', async () => {
  const handlers = Object.fromEntries(Object.keys(REQUESTS).map((n) => [n, () => { throw new Error('disk\u0007 full\n'); }])) as unknown as Handlers;
  const ask = dispatcher(() => expected, handlers);
  assert.deepEqual(await ask(own, { name: 'alert.test', args: {} }), { ok: false, failed: 'disk  full ' });
});

test('a dispatcher must be given exactly one handler per request name', () => {
  assert.throws(() => dispatcher(() => expected, { 'state.get': () => 1 } as unknown as Handlers), /handlers/);
  const extra = Object.fromEntries([...Object.keys(REQUESTS), 'shell.run'].map((n) => [n, () => 1])) as unknown as Handlers;
  assert.throws(() => dispatcher(() => expected, extra), /handlers/);
});

/** A window as the pusher sees it, recording what it is sent. */
function windowRecorder(destroyed = false) {
  const sent: Array<[string, unknown]> = [];
  const win: WindowLike & { gone: boolean } = {
    gone: destroyed,
    isDestroyed() { return this.gone; },
    webContents: { isDestroyed: () => win.gone, send: (channel: string, msg: unknown) => { sent.push([channel, msg]); } },
  };
  return { win, sent };
}

test('a listed push event reaches only the app window, on the push channel, as {event, payload: null}', () => {
  const app = windowRecorder();
  const other = windowRecorder();
  const push = pusher(() => app.win);
  assert.equal(push('state.changed'), 'sent');
  assert.equal(push('waiting.changed'), 'sent');
  assert.deepEqual(app.sent, [['keel:push', { event: 'state.changed', payload: null }], ['keel:push', { event: 'waiting.changed', payload: null }]]);
  assert.deepEqual(other.sent, []);
});

test('an unlisted push event, or a payload on an event that carries none, is refused and nothing is sent', () => {
  const app = windowRecorder();
  const push = pusher(() => app.win);
  for (const e of ['state.get', 'projects.changed', '__proto__', '', 7]) assert.equal(push(e), 'refused', String(e));
  assert.equal(push('state.changed', { path: '/x' }), 'refused');
  assert.equal(push('waiting.changed', { wu: 'E-X.W1' }), 'refused');
  assert.deepEqual(app.sent, []);
});

test('with no window, or a destroyed one, a push is dropped and nothing throws', () => {
  assert.equal(pusher(() => null)('state.changed'), 'no-window');
  const gone = windowRecorder(true);
  assert.equal(pusher(() => gone.win)('state.changed'), 'no-window');
  assert.deepEqual(gone.sent, []);
  const half = windowRecorder();
  half.win.webContents.isDestroyed = () => true;
  assert.equal(pusher(() => half.win)('state.changed'), 'no-window');
  assert.deepEqual(half.sent, []);
});

test('the pusher reads the app window per push, so a window opened again from the Dock gets later events', () => {
  let current: WindowLike | null = null;
  const push = pusher(() => current);
  assert.equal(push('state.changed'), 'no-window');
  const reopened = windowRecorder();
  current = reopened.win;
  assert.equal(push('state.changed'), 'sent');
  assert.equal(reopened.sent.length, 1);
});
