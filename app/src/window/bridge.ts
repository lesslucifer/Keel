// The window's one way to the main part. Nothing else in the window reads window.keel: every screen
// asks through this module, so W8's tests and its browser mode hand in a fake bridge with
// provideBridge() and the screens never know. Typed from the wire contract.

import type { Answer, PushEvent, Replies, RequestName, Tool } from '../shared/contract.ts';
import type { KeelBridge } from '../preload/bridge.ts';

/** Each request's arguments, as the contract's slots take them. */
export interface Args {
  'state.get': Record<string, never>;
  'projects.add': Record<string, never>;
  'projects.remove': { project: string };
  'settings.choose': { tool: Tool };
  'settings.reset': { tool: Tool };
  'alert.test': Record<string, never>;
}

let injected: KeelBridge | null = null;

/** Use `bridge` in place of the preload's window.keel (a test's or the browser mode's fake). */
export function provideBridge(bridge: KeelBridge | null): void {
  injected = bridge;
}

function current(): KeelBridge {
  if (injected !== null) return injected;
  const exposed = (globalThis as { keel?: KeelBridge }).keel;
  if (exposed === undefined) throw new Error('no bridge: the page runs outside Keel');
  return exposed;
}

/** Ask the main part; the answer is its value, a refusal (nothing ran) or a failure. */
export function ask<N extends RequestName>(name: N, args: Args[N]): Promise<Answer<Replies[N]>> {
  return current().ask(name, args as Record<string, string>) as Promise<Answer<Replies[N]>>;
}

/** Call `callback` on each `event` the main part pushes; returns the unsubscribe. */
export function onPush(event: PushEvent, callback: () => void): () => void {
  return current().on(event, () => callback());
}
