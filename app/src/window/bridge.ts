// The window's one way to the main part, and the one reader of window.keel. A screen never touches
// the bridge itself: it gets it from useBridge(), which answers the bridge a BridgeProvider was
// given (a test's stub, the browser mode's fake) or else the preload's window.keel. ask and onPush
// take that bridge. Typed from the wire contract.

import { createContext, createElement, useContext, type ReactNode } from 'react';
import type { Answer, PushEvent, Replies, RequestName, Tool } from '../shared/contract.ts';
import type { KeelBridge } from '../preload/bridge.ts';

export type { KeelBridge };

/** Each request's arguments, as the contract's slots take them. */
export interface Args {
  'state.get': Record<string, never>;
  'projects.add': Record<string, never>;
  'projects.remove': { project: string };
  'settings.choose': { tool: Tool };
  'settings.reset': { tool: Tool };
  'alert.test': Record<string, never>;
}

/** One request with its arguments, as a value a button can hold. */
export type Request = { [N in RequestName]: { name: N; args: Args[N] } }[RequestName];

const BridgeContext = createContext<KeelBridge | null>(null);

/** Hands `bridge` to every screen below it; with none, the screens use the preload's window.keel. */
export function BridgeProvider({ bridge, children }: { bridge?: KeelBridge; children?: ReactNode }) {
  return createElement(BridgeContext.Provider, { value: bridge ?? null }, children);
}

/** The bridge the screens ask through: the provided one, else window.keel; neither is an error. */
export function useBridge(): KeelBridge {
  const provided = useContext(BridgeContext);
  if (provided !== null) return provided;
  const exposed = (globalThis as { keel?: KeelBridge }).keel;
  if (exposed === undefined) throw new Error('no bridge: no BridgeProvider was given one and there is no window.keel (the page runs outside Keel)');
  return exposed;
}

/** Ask the main part; the answer is its value, a refusal (nothing ran) or a failure. */
export function ask<N extends RequestName>(bridge: KeelBridge, name: N, args: Args[N]): Promise<Answer<Replies[N]>> {
  return bridge.ask(name, args as Record<string, string>) as Promise<Answer<Replies[N]>>;
}

/** Call `callback` on each `event` the main part pushes; returns the unsubscribe. */
export function onPush(bridge: KeelBridge, event: PushEvent, callback: () => void): () => void {
  return bridge.on(event, () => callback());
}
