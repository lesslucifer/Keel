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

export type RequestName = 'state.get' | 'projects.add' | 'projects.remove' | 'settings.choose' | 'settings.reset' | 'alert.test' | 'waiting.get';

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
  'waiting.get': frozenTable<Slot>({}),
}) as Readonly<Record<RequestName, Readonly<Record<string, Slot>>>>;

export type PushEvent = 'state.changed' | 'waiting.changed';

/** The push events, each with its payload's shape in words. W7 listed state.changed; W5 adds waiting.changed. */
export const PUSH_EVENTS: Readonly<Record<PushEvent, 'none'>> = frozenTable({
  // A project or a setting changed: the window calls state.get again. An event carries no data a request could not fetch.
  'state.changed': 'none',
  // What git or the router says changed, an alert was shown or clicked: the window calls waiting.get again.
  'waiting.changed': 'none',
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

/** The note kinds the view reads: which tips carry a sign-off (mr), a review (cr), a gate receipt (dod). */
export const NOTE_KINDS = Object.freeze(['mr', 'cr', 'dod'] as const);
export type NoteKind = (typeof NOTE_KINDS)[number];

/** A local branch: its short name (main, epic/E-X, wu/E-X/w1), its tip, and where it stands against its upstream (no fetch). */
export interface GitBranch {
  name: string;
  tip: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  /** The upstream is configured but its branch is gone. */
  gone: boolean;
}

export interface GitWorktree {
  path: string;
  /** Its branch's short name, or null when detached or bare. */
  branch: string | null;
  tip: string | null;
}

/** What git says, read with the fixed commands only. */
export interface GitFacts {
  branches: GitBranch[];
  /** Per kind, the open branches' tips (full ids: epics not in main, their work units not yet in them) that carry a note of that kind. */
  notes: Record<NoteKind, string[]>;
  /** Per target branch (main, each epic not in main, each open work unit with a task), the branches whose tips it holds. */
  merged: Record<string, string[]>;
  worktrees: GitWorktree[];
  /** Why git could not be read, or null. */
  problem: string | null;
}

/** One alert line the router wrote, as the update log's state keeps it, and whether this app has shown it. */
export interface RouterAlert {
  n: number;
  at: string;
  headline: string;
  lines: string[];
  wu: string | null;
  link: string | null;
  shown: boolean;
}

/** What the router said, folded from its update log; a key it has not written is at its default. */
export interface RouterView {
  /** The run the values belong to; null when no run could be read (every router field at its default). */
  run: string | null;
  /** The number of the last line folded in; 0 with no run. */
  n: number;
  /** Each key the router set (router, facts, wu/<id>/stage, question/<id>, worker/<name>, ...), its JSON value as written. */
  values: Record<string, unknown>;
  /** A line (any line: the watchdog writes a heartbeat each tick) within the silence limit, 15 minutes. */
  heard: boolean;
  /** The newest line's time, as the router wrote it. */
  lastAt: string | null;
  /** Why the log could not be read, or null. */
  problem: string | null;
}

/** Where a click on an alert or a keel:// link points the window: a work unit, or none (the list); seq grows per click. */
export interface Focus {
  wu: string | null;
  seq: number;
}

/** waiting.get's value: the one listed project's facts. W6 adds the items drawn from them. */
export interface View {
  project: { id: string; name: string } | null;
  git: GitFacts | null;
  router: RouterView;
  /** The router's last alerts (at most 20), oldest first. */
  alerts: RouterAlert[];
  focus: Focus | null;
  /** The app macOS opens keel:// links with, or null when unknown or none. */
  scheme: string | null;
  /** When the view was last read. */
  readAt: string | null;
}

export interface Replies {
  'state.get': AppState;
  'projects.add': Change;
  'projects.remove': Change;
  'settings.choose': Change;
  'settings.reset': Change;
  'alert.test': AlertOutcome;
  'waiting.get': View;
}
