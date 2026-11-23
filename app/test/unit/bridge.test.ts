import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PROJECT_ID, slotAccepts, senderRefusal, check, plainDeep, pushable, type EventLike, type Expected } from '../../src/main/bridge.ts';
import { REQUESTS, PUSH_EVENTS, SLOT_CAP, TOOL_NAMES, type Slot } from '../../src/shared/contract.ts';
import { seeded, hostileString, pick } from '../support/seeded.ts';

const PAGE = 'app://keel/index.html';
const expected: Expected = { contentsId: 7, pages: [PAGE] };
const own = (url = PAGE): EventLike => ({ sender: { id: 7 }, senderFrame: { url, parent: null } });

const TOOL: Slot = { kind: 'enum', values: TOOL_NAMES };
const ID: Slot = { kind: 'project-id' };

test('an enum slot takes exactly its values and nothing near them', () => {
  for (const v of ['keel', 'git', 'claude']) assert.ok(slotAccepts(TOOL, v), v);
  for (const v of ['Keel', 'git ', ' git', 'gi', 'gitx', 'git\n', 'git\u0000', '', 'claude;', 'keel/../git', '__proto__', 'constructor', 'toString']) {
    assert.ok(!slotAccepts(TOOL, v), JSON.stringify(v));
  }
});

test('a project-id slot takes p and 8 lowercase hex digits and nothing else', () => {
  assert.ok(slotAccepts(ID, 'p0123abcd'));
  assert.ok(slotAccepts(ID, 'pffffffff'));
  for (const v of ['P0123abcd', 'p0123ABCD', 'p0123abc', 'p0123abcde', 'p0123abcg', 'q0123abcd', ' p0123abcd', 'p0123abcd\n', '/Users/x', '$(id)', '']) {
    assert.ok(!slotAccepts(ID, v), JSON.stringify(v));
  }
});

const ITEM: Slot = { kind: 'item-id' };

test('an item-id slot takes kind/subject@version and nothing else: no path, no dot-dot segment, no other kind, nothing over the cap', () => {
  for (const v of ['result/E-APPLIST.W6@0f1e2d3', 'epic-result/E-APPLIST@0123456789ab', 'plan/E-X.W1@abcdef0', 'question/merge-order@a1b2c3d4', 'worker/e-applist-w6@deadbeef', 'push/main@9a8b7c6', 'task/1a2b3c4d@0f1e2d3', 'motion/E-X.W2@a1b2c3d4']) {
    assert.ok(slotAccepts(ITEM, v), v);
  }
  for (const v of ['', 'result/E-X.W1', 'result/E-X.W1@', 'result/@0f1e2d3', 'result/E-X.W1@0F1E2D3', 'result/E-X.W1@0f1e2d', 'result/E-X.W1@0f1e2d3c4b5a6', 'result/a/b@0f1e2d3', 'result/../x@0f1e2d3', '/etc/passwd', 'shell/E-X.W1@0f1e2d3', 'RESULT/E-X.W1@0f1e2d3', 'result/E X@0f1e2d3', `result/${'a'.repeat(42)}@0f1e2d3`, `epic-result/${'a'.repeat(41)}@0f1e2d3c4b5a`, ' result/E-X.W1@0f1e2d3', 'result/E-X.W1@0f1e2d3\n']) {
    assert.ok(!slotAccepts(ITEM, v), JSON.stringify(v));
  }
});

test('generated inputs: no random string passes the item-id slot unless it matches the item id and fits the cap', () => {
  const rng = seeded(0x17e3);
  const pattern = /^(?:result|epic-result|plan|question|worker|task|push|motion)\/[A-Za-z0-9._-]{1,41}@[0-9a-f]{7,12}$/;
  const parts = ['result/', 'plan/', 'question/', '@', '/', '.', '..', '-', '_', 'E-X.W1', '0f1e2d3', 'a', 'F', ' ', '%2e', '\u0000'];
  for (let i = 0; i < 3000; i++) {
    const v = rng() < 0.5 ? hostileString(rng, 70) : Array.from({ length: 1 + Math.floor(rng() * 6) }, () => pick(rng, parts)).join('');
    assert.equal(slotAccepts(ITEM, v), v.length <= SLOT_CAP && pattern.test(v), JSON.stringify(v));
  }
});

test('a slot refuses every value that is not a string', () => {
  for (const v of [undefined, null, 1, 0, true, {}, [], ['git'], { toString: () => 'git' }, new String('git'), Symbol('git'), 1n]) {
    assert.ok(!slotAccepts(TOOL, v));
    assert.ok(!slotAccepts(ID, v));
  }
});

test('a slot refuses a value over the length cap and any value with a control character, before its pattern', () => {
  assert.equal(SLOT_CAP, 64);
  assert.ok(!slotAccepts({ kind: 'enum', values: ['a'.repeat(SLOT_CAP + 1)] }, 'a'.repeat(SLOT_CAP + 1)));
  assert.ok(slotAccepts({ kind: 'enum', values: ['a'.repeat(SLOT_CAP)] }, 'a'.repeat(SLOT_CAP)));
  assert.ok(!slotAccepts({ kind: 'enum', values: ['a\u0007b'] }, 'a\u0007b'));
  assert.ok(!slotAccepts({ kind: 'enum', values: ['a\u0085b'] }, 'a\u0085b'));
});

test('generated inputs: no random string passes a slot unless it is one of its values or matches the project id', () => {
  const rng = seeded(0xb41d07);
  const near = ['p', 'k', 'e', 'l', 'g', 'i', 't', 'c', 'a', 'u', 'd', '0', '9', 'f', 'A', ' ', '\n', '/', '$'];
  for (let i = 0; i < 5000; i++) {
    const v = i % 2 === 0 ? hostileString(rng, 12) : hostileString(rng, 10, near);
    assert.equal(slotAccepts(TOOL, v), (TOOL_NAMES as readonly string[]).includes(v), JSON.stringify(v));
    assert.equal(slotAccepts(ID, v), /^p[0-9a-f]{8}$/.test(v), JSON.stringify(v));
  }
  for (let i = 0; i < 500; i++) {
    const v = 'p' + Array.from({ length: 8 }, () => pick(rng, [...'0123456789abcdef'])).join('');
    assert.ok(slotAccepts(ID, v) && PROJECT_ID.test(v), v);
  }
});

test('the sender check takes only the main frame of the app window showing the app page', () => {
  assert.equal(senderRefusal(own(), expected), null);
  assert.equal(senderRefusal(own(`${PAGE}#tools`), expected), null);
  assert.equal(senderRefusal({ sender: { id: 7 }, senderFrame: null }, expected), 'no-frame');
  assert.equal(senderRefusal({ sender: { id: 7 }, senderFrame: { url: PAGE, parent: {} } }, expected), 'not-main-frame');
  assert.equal(senderRefusal({ sender: { id: 8 }, senderFrame: { url: PAGE, parent: null } }, expected), 'other-window');
  for (const url of ['https://example.com/', 'app://keel/other.html', `${PAGE}?x=1`, 'app://evil/index.html', 'file:///index.html', 'about:blank', '']) {
    assert.equal(senderRefusal(own(url), expected), 'other-page', url);
  }
});

test('the sender check refuses everything when no window is expected', () => {
  assert.equal(senderRefusal(own(), null), 'no-window');
});

test('check refuses a call that is not a plain {name, args} object', () => {
  const calls: unknown[] = [null, undefined, 'state.get', ['state.get', {}], { name: 'state.get' }, { args: {} }, { name: 'state.get', args: {}, extra: 1 }, new (class { name = 'state.get'; args = {}; })()];
  for (const call of calls) assert.deepEqual(check(call, own(), expected), { ok: false, refused: 'bad-call' }, String(call));
});

test('check refuses an unknown name, including the names every object inherits', () => {
  for (const name of ['state.set', 'shell.run', '__proto__', 'constructor', 'toString', 'hasOwnProperty', '', 'STATE.GET', 1, null]) {
    assert.deepEqual(check({ name, args: {} }, own(), expected), { ok: false, refused: 'unknown-name' }, String(name));
  }
});

test('check refuses args that are not a plain object with exactly the slot keys', () => {
  const bad: unknown[] = [undefined, null, [], 'git', { tool: 'git', more: 'x' }, {}, Object.create({ tool: 'git' }), Object.assign(Object.create(null), { tool: 'git' }), { [Symbol('s')]: 1, tool: 'git' }];
  for (const args of bad) assert.deepEqual(check({ name: 'settings.reset', args }, own(), expected), { ok: false, refused: 'bad-args' });
  assert.deepEqual(check({ name: 'state.get', args: { x: 1 } }, own(), expected), { ok: false, refused: 'bad-args' });
});

test('check refuses a slot value that is free text, a path or a command, with a wrong type', () => {
  for (const tool of ['/usr/bin/git', 'git; rm -rf ~', '$(id)', 'git\u0000', 'GIT', 1, true, ['git']]) {
    assert.deepEqual(check({ name: 'settings.choose', args: { tool } }, own(), expected), { ok: false, refused: 'bad-slot:tool' }, String(tool));
  }
  assert.deepEqual(check({ name: 'projects.remove', args: { project: '/Users/me/proj' } }, own(), expected), { ok: false, refused: 'bad-slot:project' });
});

test('check passes a good call through with a fresh args object holding only the slot values', () => {
  const args = { tool: 'claude' };
  const r = check({ name: 'settings.choose', args }, own(), expected);
  assert.deepEqual(r, { ok: true, name: 'settings.choose', args: { tool: 'claude' } });
  assert.ok(r.ok && r.args !== args);
});

test('check judges the sender before anything else', () => {
  assert.deepEqual(check('garbage', { sender: { id: 9 }, senderFrame: { url: PAGE, parent: null } }, expected), { ok: false, refused: 'other-window' });
});

test('the request and push tables are frozen and inherit no names', () => {
  for (const table of [REQUESTS, PUSH_EVENTS]) {
    assert.ok(Object.isFrozen(table));
    assert.equal(Object.getPrototypeOf(table), null);
  }
  for (const slots of Object.values(REQUESTS)) {
    assert.ok(Object.isFrozen(slots));
    assert.equal(Object.getPrototypeOf(slots), null);
  }
});

test('pushable() takes only an event name in the fixed list', () => {
  assert.equal(pushable('state.changed'), true);
  for (const e of ['state.get', 'State.changed', 'state.changed ', '__proto__', 'constructor', 'toString', '', null, undefined, 1, {}]) {
    assert.equal(pushable(e), false, String(e));
  }
});

test('plainDeep() passes every string of a payload through plain(), at any depth, keys included', () => {
  assert.deepEqual(plainDeep('a\u202eb'), 'a b');
  assert.deepEqual(plainDeep({ 'k\u0007': ['x\ny', { z: 'q\u2028r', n: 3, b: true, none: null }] }), { 'k ': ['x y', { z: 'q r', n: 3, b: true, none: null }] });
  assert.equal(plainDeep(null), null);
  assert.equal(plainDeep(undefined), null);
});

test('plainDeep() keeps an own __proto__ key as a key and never sets the prototype', () => {
  const out = plainDeep(JSON.parse('{"__proto__": {"polluted": "yes"}}')) as Record<string, unknown>;
  assert.equal(Object.getPrototypeOf(out), Object.prototype);
  assert.deepEqual(Object.keys(out), ['__proto__']);
  assert.equal((out as { polluted?: string }).polluted, undefined);
});

test('plainDeep() refuses what is not plain data: functions, symbols, big ints, non-finite numbers, class instances', () => {
  for (const v of [() => 1, Symbol('s'), 1n, NaN, Infinity, new Date(0), new Map(), [1, () => 2], { a: { b: Symbol('x') } }]) {
    assert.throws(() => plainDeep(v), /plain data/, String(typeof v));
  }
});

test('generated payloads: no string plainDeep() returns holds a control character', () => {
  const rng = seeded(0x9a7);
  const unsafe = /[\u0000-\u001f\u007f-\u009f\u200e\u200f\u202a-\u202e\u2028\u2029\u2066-\u2069]/u;
  const strings = (v: unknown): string[] => (typeof v === 'string' ? [v] : Array.isArray(v) ? v.flatMap(strings) : v !== null && typeof v === 'object' ? Object.entries(v).flatMap(([k, x]) => [k, ...strings(x)]) : []);
  for (let i = 0; i < 500; i++) {
    const payload = { [hostileString(rng, 6)]: [hostileString(rng, 20), { [hostileString(rng, 4)]: hostileString(rng, 20) }] };
    const out = strings(plainDeep(payload));
    assert.equal(out.length, strings(payload).length);
    for (const s of out) assert.ok(!unsafe.test(s), JSON.stringify(s));
  }
});
