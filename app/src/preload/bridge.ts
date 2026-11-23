// The preload's bridge factory, pure: given ipcRenderer (injected, so a node test can build it),
// it makes the one object the window gets as window.keel. ask() invokes the request channel with
// {name, args} and nothing else; the main part checks every call. on() listens only for a push
// event in the contract's list, hands its callback a copy of the payload alone - never Electron's
// IpcRendererEvent, which holds ipcRenderer itself - and returns an unsubscribe.

import { ASK_CHANNEL, PUSH_CHANNEL, PUSH_EVENTS } from '../shared/contract.ts';

type Listener = (event: unknown, message: unknown) => void;

/** The part of Electron's ipcRenderer the bridge uses. */
export interface IpcLike {
  invoke(channel: string, payload: unknown): Promise<unknown>;
  on(channel: string, listener: Listener): unknown;
  removeListener(channel: string, listener: Listener): unknown;
}

export interface KeelBridge {
  ask(name: string, args: Record<string, string>): Promise<unknown>;
  on(event: string, callback: (payload: unknown) => void): () => void;
}

export function makeBridge(ipc: IpcLike): KeelBridge {
  return Object.freeze({
    ask: (name: string, args: Record<string, string>): Promise<unknown> => ipc.invoke(ASK_CHANNEL, { name, args }),
    on: (event: string, callback: (payload: unknown) => void): (() => void) => {
      if (typeof event !== 'string' || !Object.hasOwn(PUSH_EVENTS, event)) throw new Error(`not a push event: ${String(event)}`);
      if (typeof callback !== 'function') throw new Error('the callback must be a function');
      const listener: Listener = (_ipcEvent, message) => {
        if (typeof message !== 'object' || message === null || (message as { event?: unknown }).event !== event) return;
        callback(structuredClone((message as { payload?: unknown }).payload ?? null));
      };
      ipc.on(PUSH_CHANNEL, listener);
      let subscribed = true;
      return () => {
        if (!subscribed) return;
        subscribed = false;
        ipc.removeListener(PUSH_CHANNEL, listener);
      };
    },
  });
}
