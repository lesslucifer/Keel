// The stub bridge the window's tests hand to BridgeProvider: it records every ask, answers from a
// per-request handler (a handler may return a promise the test settles), and lets the test push an
// event to whatever the window subscribed.
import type { KeelBridge } from '../../src/preload/bridge.ts';
import type { Answer, AppState, RequestName } from '../../src/shared/contract.ts';

export type Handler = (args: Record<string, string>) => Answer | Promise<Answer>;

export interface Stub {
  bridge: KeelBridge;
  /** Every ask, in order. */
  asks: Array<{ name: string; args: Record<string, string> }>;
  /** The names asked, in order. */
  names(): string[];
  /** Push `event` to every subscriber; the number reached. */
  push(event: string): number;
  /** How many subscribers `event` has now. */
  subscribers(event: string): number;
  /** Replace the handler of `name`. */
  answer(name: RequestName, handler: Handler): void;
}

export const ok = <T>(value: T): Answer<T> => ({ ok: true, value });

/** A promise and the functions that settle it, for an answer the test releases. */
export function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

export function sampleState(over: Partial<AppState> = {}): AppState {
  return {
    version: '0.1.0',
    path: { entries: ['/opt/homebrew/bin', '/usr/bin', '/bin'], source: 'login-shell', reason: null },
    tools: {
      keel: { path: '/opt/homebrew/bin/keel', source: 'path', problem: null },
      git: { path: '/usr/bin/git', source: 'set', problem: null },
      claude: { path: null, source: 'missing', problem: 'not found on the PATH' },
    },
    projects: [{ id: 'p0a1b2c3d', path: '/Users/owner/code/keel', name: 'keel' }],
    ...over,
  };
}

export function makeStub(handlers: Partial<Record<RequestName, Handler>> = {}): Stub {
  const table: Partial<Record<string, Handler>> = { 'state.get': () => ok(sampleState()), ...handlers };
  const asks: Stub['asks'] = [];
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const bridge: KeelBridge = {
    ask(name, args) {
      asks.push({ name, args: { ...args } });
      const handler = table[name];
      return Promise.resolve(handler === undefined ? { ok: false, refused: 'unknown-name' } : handler(args));
    },
    on(event, callback) {
      const set = listeners.get(event) ?? new Set();
      listeners.set(event, set);
      set.add(callback);
      return () => {
        set.delete(callback);
      };
    },
  };
  return {
    bridge,
    asks,
    names: () => asks.map((a) => a.name),
    push(event) {
      const set = [...(listeners.get(event) ?? [])];
      for (const cb of set) cb(null);
      return set.length;
    },
    subscribers: (event) => listeners.get(event)?.size ?? 0,
    answer(name, handler) {
      table[name] = handler;
    },
  };
}
