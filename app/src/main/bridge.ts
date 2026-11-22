// The bridge: the only door between the window and the main part, both ways.
// Requests: the window sends {name, args}; the main part answers only a name in the contract's
// table, only from the app window's own main frame on the app's own page, and only with args that
// are exactly the name's slots, each value one its slot allows. A refused call runs nothing.
// Push: the main part sends only an event name in the contract's list, only to the app's window,
// with every string passed through plain(). Pure: the shell (index.ts) hands Electron's objects in.

import { REQUESTS, PUSH_EVENTS, PUSH_CHANNEL, PROJECT_ID_SOURCE, ITEM_ID_SOURCE, SLOT_CAP, type Answer, type RequestName, type Slot, type PushEvent, type PushMessage } from '../shared/contract.ts';
import { plain } from './text.ts';

export const PROJECT_ID = new RegExp(PROJECT_ID_SOURCE);
export const ITEM_ID = new RegExp(ITEM_ID_SOURCE);

/** A frame as Electron's WebFrameMain shows it: its URL, and its parent (null for a main frame). */
export interface FrameLike {
  readonly url: string;
  readonly parent: unknown;
}

/** The part of an IpcMainInvokeEvent the check reads. */
export interface EventLike {
  readonly sender: { readonly id: number };
  readonly senderFrame: FrameLike | null;
}

/** What a sender must be: the app window's webContents id, and the page(s) it may show. */
export interface Expected {
  readonly contentsId: number;
  readonly pages: readonly string[];
}

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** Whether `v` is a value `slot` allows: a string, not empty, within the cap, no control character, then the slot's own rule. */
export function slotAccepts(slot: Slot, v: unknown): boolean {
  if (typeof v !== 'string' || v.length === 0 || v.length > SLOT_CAP || CONTROL.test(v)) return false;
  if (slot.kind === 'enum') return slot.values.includes(v);
  return slot.kind === 'project-id' ? PROJECT_ID.test(v) : ITEM_ID.test(v);
}

/** Why the sender is refused, or null when it is the app window's main frame on the app's page. */
export function senderRefusal(event: EventLike, expected: Expected | null): string | null {
  if (expected === null) return 'no-window';
  const frame = event.senderFrame;
  if (frame === null || frame === undefined) return 'no-frame';
  if (frame.parent !== null) return 'not-main-frame';
  if (event.sender.id !== expected.contentsId) return 'other-window';
  const url = typeof frame.url === 'string' ? frame.url.split('#')[0] : undefined;
  if (url === undefined || !expected.pages.includes(url)) return 'other-page';
  return null;
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v) && Object.getPrototypeOf(v) === Object.prototype;
}

/** Exactly these string keys, own and enumerable, and no symbol key. */
function hasExactly(v: Record<string, unknown>, keys: readonly string[]): boolean {
  if (Object.getOwnPropertySymbols(v).length > 0) return false;
  const own = Object.getOwnPropertyNames(v);
  return own.length === keys.length && keys.every((k) => Object.prototype.propertyIsEnumerable.call(v, k));
}

export type Checked = { ok: true; name: RequestName; args: Record<string, string> } | { ok: false; refused: string };

/** Judge one call: the sender first, then the call's shape, its name, its args and each slot. */
export function check(call: unknown, event: EventLike, expected: Expected | null): Checked {
  const who = senderRefusal(event, expected);
  if (who !== null) return { ok: false, refused: who };
  if (!isPlainObject(call) || !hasExactly(call, ['name', 'args'])) return { ok: false, refused: 'bad-call' };
  const { name, args } = call;
  if (typeof name !== 'string' || !Object.hasOwn(REQUESTS, name)) return { ok: false, refused: 'unknown-name' };
  const table = REQUESTS[name as RequestName];
  const keys = Object.keys(table);
  if (!isPlainObject(args) || !hasExactly(args, keys)) return { ok: false, refused: 'bad-args' };
  const clean: Record<string, string> = {};
  for (const k of keys) {
    const v = args[k];
    if (!slotAccepts(table[k] as Slot, v)) return { ok: false, refused: `bad-slot:${k}` };
    clean[k] = v as string;
  }
  return { ok: true, name: name as RequestName, args: clean };
}

/**
 * `v` as plain data with every string (object keys too) passed through plain(); undefined becomes
 * null. Throws on anything else - a function, a symbol, a big int, a non-finite number, a class
 * instance - so nothing but plain data ever crosses to the window.
 */
export function plainDeep(v: unknown): unknown {
  if (v === undefined || v === null) return null;
  if (typeof v === 'string') return plain(v);
  if (typeof v === 'boolean') return v;
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (Array.isArray(v)) return v.map(plainDeep);
  if (isPlainObject(v)) {
    const out: Record<string, unknown> = {};
    // defineProperty, not assignment: an own '__proto__' key stays a key and never sets the prototype.
    for (const [k, x] of Object.entries(v)) Object.defineProperty(out, plain(k), { value: plainDeep(x), enumerable: true, writable: true, configurable: true });
    return out;
  }
  throw new Error(`not plain data: ${typeof v}`);
}

export type Handlers = Record<RequestName, (args: Record<string, string>) => unknown>;

/**
 * The function the shell hands to ipcMain.handle(ASK_CHANNEL): each call checked, then its handler
 * run, its value passed through plainDeep(). `expected` is read per call (null while no window is
 * open). A refused call runs nothing; a handler that throws answers `failed`, its message plain.
 */
export function dispatcher(expected: () => Expected | null, handlers: Handlers): (event: EventLike, call: unknown) => Promise<Answer> {
  const names = Object.keys(REQUESTS).sort();
  const given = Object.keys(handlers).sort();
  if (names.length !== given.length || names.some((n, i) => n !== given[i])) {
    throw new Error(`the dispatcher needs handlers for exactly: ${names.join(', ')}`);
  }
  const table = new Map(Object.entries(handlers));
  return async (event, call) => {
    const c = check(call, event, expected());
    if (!c.ok) return c;
    const handler = table.get(c.name) as (args: Record<string, string>) => unknown;
    try {
      return { ok: true, value: plainDeep(await handler(c.args)) };
    } catch (e) {
      return { ok: false, failed: plain(e instanceof Error ? e.message : String(e), 500) };
    }
  };
}

/** Whether `event` is a push event name in the contract's list. */
export function pushable(event: unknown): event is PushEvent {
  return typeof event === 'string' && Object.hasOwn(PUSH_EVENTS, event);
}

/** The app window as the pusher reads it (Electron's BrowserWindow). */
export interface WindowLike {
  isDestroyed(): boolean;
  readonly webContents: { isDestroyed(): boolean; send(channel: string, message: PushMessage): void };
}

export type Pushed = 'sent' | 'refused' | 'no-window';

/**
 * The shell's push(event, payload): only a listed event, only to the app window `appWindow()`
 * returns at that moment, on PUSH_CHANNEL. An event whose payload is 'none' takes none. No window,
 * or a destroyed one, drops the push.
 */
export function pusher(appWindow: () => WindowLike | null): (event: unknown, payload?: unknown) => Pushed {
  return (event, payload) => {
    if (!pushable(event)) return 'refused';
    if (PUSH_EVENTS[event] === 'none' && payload !== undefined && payload !== null) return 'refused';
    const win = appWindow();
    if (win === null || win.isDestroyed() || win.webContents.isDestroyed()) return 'no-window';
    win.webContents.send(PUSH_CHANNEL, { event, payload: plainDeep(payload) as null });
    return 'sent';
  };
}

/** The contract in words, sorted: each request's slots and each push event's payload. */
export function contract(): { requests: Record<string, Record<string, string>>; push: Record<string, string> } {
  const requests: Record<string, Record<string, string>> = {};
  for (const name of Object.keys(REQUESTS).sort()) {
    const desc: Record<string, string> = {};
    for (const [k, s] of Object.entries(REQUESTS[name as RequestName])) {
      desc[k] = s.kind === 'enum' ? `one of ${s.values.join('|')}` : s.kind === 'project-id' ? `project-id ${PROJECT_ID_SOURCE}` : `item-id ${ITEM_ID_SOURCE}`;
    }
    requests[name] = desc;
  }
  const push: Record<string, string> = {};
  for (const e of Object.keys(PUSH_EVENTS).sort()) push[e] = PUSH_EVENTS[e as PushEvent];
  return { requests, push };
}
