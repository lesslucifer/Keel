// The fake bridge: a KeelBridge that answers from an in-memory AppState, so the window runs in a
// browser with no Electron (DEC-74) and the tests draw screens from fixtures. It checks each call as
// the main part does (src/main/bridge.ts check: the name in the contract's table, args exactly its
// slots, each value one its slot allows), answers every request with a handler from a table typed
// by RequestName (a name the contract gains fails tsc here too), and pushes state.changed once per
// change. waiting.get answers the scenario's view (fixtures.ts VIEWS), or the view ?view= names; item.open
// answers as the main part would (an item listed now, with a page) and opens nothing; item.mark moves an
// item to Sent and back (the list drawn again by src/shared/items.ts) and pushes waiting.changed. Never part of the shipped page: only browser.tsx and the window's tests import fake/.

import { ITEM_ID_SOURCE, PROJECT_ID_SOURCE, PUSH_EVENTS, REQUESTS, SLOT_CAP, type AlertOutcome, type Answer, type AppState, type Change, type Item, type Replies, type RequestName, type Slot, type Tool, type View } from '../../shared/contract.ts';
import type { KeelBridge } from '../bridge.ts';
import { itemsOf, countsOf } from '../../shared/items.ts';
import { CHOSEN, NEXT_PROJECT, ON_PATH, SCENARIOS, VIEWS, type ScenarioName, type ViewName } from './fixtures.ts';

export interface FakeOptions {
  /** How the fake waits before each answer; a test passes one that does not. */
  wait?: (ms: number) => Promise<void>;
  /** The waiting view to answer instead of the scenario's own (browser mode's ?view=). */
  view?: ViewName | null;
}

const PROJECT_ID = new RegExp(PROJECT_ID_SOURCE);
const ITEM_ID = new RegExp(ITEM_ID_SOURCE);
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

function slotAccepts(slot: Slot, v: unknown): boolean {
  if (typeof v !== 'string' || v.length === 0 || v.length > SLOT_CAP || CONTROL.test(v)) return false;
  if (slot.kind === 'enum') return slot.values.includes(v);
  return slot.kind === 'project-id' ? PROJECT_ID.test(v) : ITEM_ID.test(v);
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v) && Object.getPrototypeOf(v) === Object.prototype;
}

/** Why the main part would refuse this call, or null. */
function refusal(name: unknown, args: unknown): string | null {
  if (typeof name !== 'string' || !Object.hasOwn(REQUESTS, name)) return 'unknown-name';
  const table = REQUESTS[name as RequestName];
  const keys = Object.keys(table);
  if (!isPlainObject(args) || Object.getOwnPropertySymbols(args).length > 0) return 'bad-args';
  const own = Object.getOwnPropertyNames(args);
  if (own.length !== keys.length || !keys.every((k) => Object.prototype.propertyIsEnumerable.call(args, k))) return 'bad-args';
  for (const k of keys) if (!slotAccepts(table[k] as Slot, args[k])) return `bad-slot:${k}`;
  return null;
}

const DONE: Change = { done: true };
const notDone = (why: string): Change => ({ done: false, why });
const realWait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export function makeFakeBridge(name: ScenarioName, options: FakeOptions = {}): KeelBridge {
  const scenario = SCENARIOS[name];
  const wait = options.wait ?? realWait;
  const state: AppState = structuredClone(scenario.state);
  const listeners = new Set<(payload: unknown) => void>();
  const changed = () => {
    for (const cb of [...listeners]) cb(null);
  };
  const view: View = structuredClone(options.view === undefined || options.view === null ? scenario.view : VIEWS[options.view]);
  const waitingListeners = new Set<(payload: unknown) => void>();
  const waitingChanged = () => {
    for (const cb of [...waitingListeners]) cb(null);
  };
  const listed = (id: string): Item | undefined => view.items.find((i) => i.id === id);
  // The marks the fixture view starts with, then the owner's; the list is drawn again by the main part's rules.
  const sent = new Set(view.items.filter((i) => i.group === 'sent').map((i) => i.id));

  type Handlers = { [N in RequestName]: (args: Record<string, string>) => Replies[N] };
  const handlers: Handlers = {
    'state.get': () => structuredClone(state),
    'projects.add': () => {
      if (state.projects.length > 0) return notDone('Keel holds one project; remove it first');
      state.projects.push({ ...NEXT_PROJECT });
      return DONE;
    },
    'projects.remove': ({ project }) => {
      const at = state.projects.findIndex((p) => p.id === project);
      if (at < 0) return notDone('that project is not in the list');
      state.projects.splice(at, 1);
      return DONE;
    },
    'settings.choose': ({ tool }) => {
      state.tools[tool as Tool] = { path: CHOSEN[tool as Tool], source: 'set', problem: null };
      return DONE;
    },
    'settings.reset': ({ tool }) => {
      state.tools[tool as Tool] = { path: ON_PATH[tool as Tool], source: 'path', problem: null };
      return DONE;
    },
    'alert.test': (): AlertOutcome => 'shown',
    'waiting.get': () => structuredClone(view),
    'item.open': ({ item }) => {
      const i = listed(item as string);
      if (i === undefined) return notDone('that item has left the list or moved on: the list shows it as it is now');
      if (i.open === 'text') return notDone('a question shows its text in the list itself');
      if (i.open === 'none') return notDone('this item has nothing to open');
      return DONE;
    },
    'item.mark': ({ item, sent: mark }) => {
      const i = listed(item as string);
      if (i === undefined) return notDone('that item has left the list or moved on: the list shows it as it is now');
      if (!i.markable) return notDone('only a plan to read or a question can be marked sent');
      if (mark === 'yes') sent.add(i.id);
      else sent.delete(i.id);
      view.items = itemsOf({ git: view.git, router: view.router, sent });
      view.counts = countsOf(view.items);
      return DONE;
    },
  };

  const answer = (request: string, args: Record<string, string>): Answer => {
    const why = refusal(request, args);
    if (why !== null) return { ok: false, refused: why };
    const n = request as RequestName;
    if (scenario.answers === 'refuse') return { ok: false, refused: 'fixture: the refusing scenario refuses every request' };
    if (scenario.answers === 'fail' && n !== 'state.get') return { ok: false, failed: 'fixture: the failing scenario fails every change' };
    const value = handlers[n](args);
    if (n === 'item.mark' && (value as Change).done) waitingChanged();
    else if (n !== 'state.get' && n !== 'alert.test' && n !== 'waiting.get' && n !== 'item.open' && (value as Change).done) changed();
    return { ok: true, value };
  };

  return Object.freeze({
    async ask(request: string, args: Record<string, string>): Promise<unknown> {
      await wait(scenario.delayMs);
      return answer(request, args);
    },
    on(event: string, callback: (payload: unknown) => void): () => void {
      if (typeof event !== 'string' || !Object.hasOwn(PUSH_EVENTS, event)) throw new Error(`not a push event: ${String(event)}`);
      const set = event === 'waiting.changed' ? waitingListeners : listeners;
      set.add(callback);
      return () => {
        set.delete(callback);
      };
    },
  });
}
