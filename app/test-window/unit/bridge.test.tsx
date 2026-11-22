// BridgeProvider and useBridge: the provided bridge wins; with none, the preload's window.keel; with
// neither, a thrown error that says so. ask and onPush go through the bridge they are given.
import { renderHook } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, expect, test } from 'vitest';
import { BridgeProvider, ask, onPush, useBridge } from '../../src/window/bridge.ts';
import { makeStub } from '../support/stub-bridge.ts';

const g = globalThis as { keel?: unknown };
afterEach(() => {
  delete g.keel;
});

test('the bridge a BridgeProvider is given wins over window.keel', () => {
  const given = makeStub().bridge;
  g.keel = makeStub().bridge;
  const { result } = renderHook(() => useBridge(), { wrapper: ({ children }: { children: ReactNode }) => <BridgeProvider bridge={given}>{children}</BridgeProvider> });
  expect(result.current).toBe(given);
});

test('with no bridge given, useBridge answers the preload\'s window.keel, inside a provider or not', () => {
  const exposed = makeStub().bridge;
  g.keel = exposed;
  expect(renderHook(() => useBridge()).result.current).toBe(exposed);
  const { result } = renderHook(() => useBridge(), { wrapper: ({ children }: { children: ReactNode }) => <BridgeProvider>{children}</BridgeProvider> });
  expect(result.current).toBe(exposed);
});

test('with neither, useBridge throws an error naming the missing bridge', () => {
  expect(() => renderHook(() => useBridge())).toThrow(/no bridge.*window\.keel/);
});

test('ask sends exactly the name and the args through the bridge and hands back its answer', async () => {
  const stub = makeStub({ 'projects.remove': () => ({ ok: true, value: { done: true } }) });
  expect(await ask(stub.bridge, 'projects.remove', { project: 'p0a1b2c3d' })).toEqual({ ok: true, value: { done: true } });
  expect(stub.asks).toEqual([{ name: 'projects.remove', args: { project: 'p0a1b2c3d' } }]);
});

test('onPush calls back on each push of its event and stops after the unsubscribe', () => {
  const stub = makeStub();
  let calls = 0;
  const off = onPush(stub.bridge, 'state.changed', () => {
    calls += 1;
  });
  stub.push('state.changed');
  stub.push('state.changed');
  off();
  stub.push('state.changed');
  expect(calls).toBe(2);
  expect(stub.subscribers('state.changed')).toBe(0);
});
