// The window's wire contract: what the window may ask the main part, and what the main part may
// push to the window. Types and plain data only - no electron, no react, no import - so the main
// part, the preload, the window and W8's fake bridge all read the same table. Every name, slot and
// event here is snapshot-locked by app/test/integration/bridge.test.ts: a change here is a change
// to what the window may do.

/** The channel the preload invokes for a request ({name, args}); the main part checks each call. */
export const ASK_CHANNEL = 'keel:ask';

/** The channel the main part sends push events on ({event, payload}), to the app's window only. */
export const PUSH_CHANNEL = 'keel:push';

export const TOOL_NAMES = Object.freeze(['keel', 'git', 'claude'] as const);
export type Tool = (typeof TOOL_NAMES)[number];

/** A project's id, minted in the main part: 'p' and 8 lowercase hex digits. */
export const PROJECT_ID_SOURCE = '^p[0-9a-f]{8}$';

/** No slot value is longer than this; checked before any pattern. */
export const SLOT_CAP = 64;

/** An argument slot: one of a fixed list of values, or a project id. No slot takes a path, a command or free text. */
export type Slot = { readonly kind: 'enum'; readonly values: readonly string[] } | { readonly kind: 'project-id' };

const TOOL_SLOT: Slot = Object.freeze({ kind: 'enum', values: TOOL_NAMES });
const PROJECT_SLOT: Slot = Object.freeze({ kind: 'project-id' });

export type RequestName = 'state.get' | 'projects.add' | 'projects.remove' | 'settings.choose' | 'settings.reset' | 'alert.test';

function frozenTable<V>(entries: Record<string, V>): Readonly<Record<string, V>> {
  return Object.freeze(Object.assign(Object.create(null) as Record<string, V>, entries));
}

/** The request table: each name the window may send, and its argument slots (an empty table: no arguments). */
export const REQUESTS: Readonly<Record<RequestName, Readonly<Record<string, Slot>>>> = frozenTable({
  'state.get': frozenTable<Slot>({}),
  'projects.add': frozenTable<Slot>({}),
  'projects.remove': frozenTable<Slot>({ project: PROJECT_SLOT }),
  'settings.choose': frozenTable<Slot>({ tool: TOOL_SLOT }),
  'settings.reset': frozenTable<Slot>({ tool: TOOL_SLOT }),
  'alert.test': frozenTable<Slot>({}),
}) as Readonly<Record<RequestName, Readonly<Record<string, Slot>>>>;

export type PushEvent = 'state.changed';

/** The push events, each with its payload's shape in words. W7 lists one; W5 adds the waiting list's. */
export const PUSH_EVENTS: Readonly<Record<PushEvent, 'none'>> = frozenTable({
  // A project or a setting changed: the window calls state.get again. An event carries no data a request could not fetch.
  'state.changed': 'none',
}) as Readonly<Record<PushEvent, 'none'>>;

/** What travels on the push channel. */
export interface PushMessage {
  readonly event: PushEvent;
  readonly payload: null;
}

// ---- Replies (types only) ----

/** Every answer to a request: its value, a refusal by the bridge (nothing ran), or a handler's failure. */
export type Answer<T = unknown> = { ok: true; value: T } | { ok: false; refused: string } | { ok: false; failed: string };

export interface LoginPathState {
  entries: string[];
  source: 'login-shell' | 'fallback';
  /** Why the fallback was taken; null for the login shell's PATH. */
  reason: string | null;
}

export interface ToolState {
  path: string | null;
  source: 'set' | 'path' | 'missing';
  /** Why the tool is not usable, or null. */
  problem: string | null;
}

export interface ProjectEntry {
  id: string;
  path: string;
  name: string;
}

/** state.get's value. */
export interface AppState {
  version: string;
  path: LoginPathState;
  tools: Record<Tool, ToolState>;
  projects: ProjectEntry[];
}

/** projects.add, projects.remove, settings.choose and settings.reset: done, or why not (nothing saved). */
export type Change = { done: true } | { done: false; why: string };

/** alert.test's value. */
export type AlertOutcome = 'shown' | 'not-shown' | 'not-supported';

export interface Replies {
  'state.get': AppState;
  'projects.add': Change;
  'projects.remove': Change;
  'settings.choose': Change;
  'settings.reset': Change;
  'alert.test': AlertOutcome;
}
