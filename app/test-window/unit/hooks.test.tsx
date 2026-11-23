// The window's hooks and texts against the stub bridge: useAppState asks state.get on mount and on each
// state.changed, unsubscribes on unmount and keeps an error answer as text; useAction is busy while
// its one request runs and says the outcome in a sentence.
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { expect, test } from 'vitest';
import type { Answer, Change, View } from '../../src/shared/contract.ts';
import { BridgeProvider } from '../../src/window/bridge.ts';
import { alertText, answerText, changeText, sinceText, useAction, useAppState, useWaiting } from '../../src/window/hooks.ts';
import { deferred, makeStub, ok, sampleState, type Stub } from '../support/stub-bridge.ts';

const within = (stub: Stub) => ({ wrapper: ({ children }: { children: ReactNode }) => <BridgeProvider bridge={stub.bridge}>{children}</BridgeProvider> });

test('answerText: nothing for a value, the refusal and the failure each in words', () => {
  expect(answerText({ ok: true, value: 1 })).toBe('');
  expect(answerText({ ok: false, refused: 'bad-slot:tool' })).toBe('Keel refused the request (bad-slot:tool).');
  expect(answerText({ ok: false, failed: 'disk full' })).toBe('The request failed: disk full.');
});

test('changeText: done says the done sentence, not done says why, a refusal or failure says so', () => {
  const done: Answer<Change> = ok({ done: true });
  expect(changeText(done)).toBe('Done.');
  expect(changeText(done, 'Project removed from the list.')).toBe('Project removed from the list.');
  expect(changeText(ok({ done: false, why: 'a second project is not supported' }))).toBe('Not done: a second project is not supported.');
  expect(changeText(ok({ done: false, why: 'cancelled.' }))).toBe('Not done: cancelled.');
  expect(changeText({ ok: false, failed: 'boom' })).toBe('The request failed: boom.');
});

test('alertText: each of the three outcomes in words, and a refusal', () => {
  expect(alertText(ok('shown'))).toBe('The test alert was shown.');
  expect(alertText(ok('not-shown'))).toMatch(/^The test alert was not shown within 5 s\. Check System Settings > Notifications > Keel\.$/);
  expect(alertText(ok('not-supported'))).toBe('This Mac does not support alerts from Keel.');
  expect(alertText({ ok: false, refused: 'busy' })).toBe('Keel refused the request (busy).');
});

test('alertText (W9): not shown names the place macOS files the alerts - Notifications > Keel when packaged, Notifications > Electron under pnpm start', () => {
  expect(alertText(ok('not-shown'), true)).toBe('The test alert was not shown within 5 s. Check System Settings > Notifications > Keel.');
  expect(alertText(ok('not-shown'), false)).toBe('The test alert was not shown within 5 s. Check System Settings > Notifications > Electron.');
  expect(alertText(ok('shown'), false)).toBe('The test alert was shown.');
  expect(alertText(ok('not-supported'), false)).toBe('This Mac does not support alerts from Keel.');
});

test('useAppState asks state.get once on mount, loading until the answer, then holds the state', async () => {
  const stub = makeStub();
  const { result } = renderHook(() => useAppState(), within(stub));
  expect(result.current.loading).toBe(true);
  await waitFor(() => expect(result.current.state).toEqual(sampleState()));
  expect(result.current).toMatchObject({ loading: false, error: '' });
  expect(stub.names()).toEqual(['state.get']);
});

test('useAppState asks state.get again on each state.changed push and draws the new state', async () => {
  const stub = makeStub();
  const { result } = renderHook(() => useAppState(), within(stub));
  await waitFor(() => expect(result.current.loading).toBe(false));
  stub.answer('state.get', () => ok(sampleState({ projects: [] })));
  act(() => {
    expect(stub.push('state.changed')).toBe(1);
  });
  await waitFor(() => expect(result.current.state?.projects).toEqual([]));
  expect(stub.names()).toEqual(['state.get', 'state.get']);
});

test('useAppState unsubscribes from state.changed on unmount', async () => {
  const stub = makeStub();
  const { result, unmount } = renderHook(() => useAppState(), within(stub));
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(stub.subscribers('state.changed')).toBe(1);
  unmount();
  expect(stub.subscribers('state.changed')).toBe(0);
});

test('useAppState keeps a refused or failed answer as text, and keeps the last good state beside it', async () => {
  const stub = makeStub({ 'state.get': () => ({ ok: false, failed: '<b>x</b>' }) });
  const { result } = renderHook(() => useAppState(), within(stub));
  await waitFor(() => expect(result.current.error).toBe('The request failed: <b>x</b>.'));
  expect(result.current).toMatchObject({ state: null, loading: false });
  stub.answer('state.get', () => ok(sampleState()));
  act(() => void stub.push('state.changed'));
  await waitFor(() => expect(result.current.state).not.toBeNull());
  expect(result.current.error).toBe('');
  stub.answer('state.get', () => ({ ok: false, refused: 'bad-call' }));
  act(() => void stub.push('state.changed'));
  await waitFor(() => expect(result.current.error).toBe('Keel refused the request (bad-call).'));
  expect(result.current.state).toEqual(sampleState());
});

test('useAppState draws the newest answer when an older state.get answers last', async () => {
  const first = deferred<Answer>();
  const stub = makeStub({ 'state.get': () => first.promise });
  const { result } = renderHook(() => useAppState(), within(stub));
  stub.answer('state.get', () => ok(sampleState({ version: 'new' })));
  act(() => void stub.push('state.changed'));
  await waitFor(() => expect(result.current.state?.version).toBe('new'));
  await act(async () => first.resolve(ok(sampleState({ version: 'old' }))));
  expect(result.current.state?.version).toBe('new');
});

test('useAction is busy while its request runs, then says the outcome; the request went out exactly', async () => {
  const held = deferred<Answer>();
  const stub = makeStub({ 'settings.choose': () => held.promise });
  const { result } = renderHook(() => useAction(), within(stub));
  expect(result.current).toMatchObject({ busy: false, note: '' });
  act(() => void result.current.run({ name: 'settings.choose', args: { tool: 'git' } }));
  expect(result.current.busy).toBe(true);
  expect(stub.asks).toEqual([{ name: 'settings.choose', args: { tool: 'git' } }]);
  await act(async () => held.resolve(ok({ done: true })));
  expect(result.current).toMatchObject({ busy: false, note: 'Saved: Keel uses the chosen file.' });
});

test('useAction runs one request at a time: a second run while busy sends nothing', async () => {
  const held = deferred<Answer>();
  const stub = makeStub({ 'alert.test': () => held.promise, 'projects.add': () => ok({ done: true }) });
  const { result } = renderHook(() => useAction(), within(stub));
  act(() => {
    void result.current.run({ name: 'alert.test', args: {} });
    void result.current.run({ name: 'projects.add', args: {} });
  });
  expect(stub.names()).toEqual(['alert.test']);
  await act(async () => held.resolve(ok('shown')));
  expect(result.current.note).toBe('The test alert was shown.');
});

test('useAction says each request\'s outcome in its own words, and a bridge that throws as a failure', async () => {
  const stub = makeStub({
    'projects.remove': () => ok({ done: true }),
    'projects.add': () => ok({ done: false, why: 'cancelled' }),
    'settings.reset': () => ({ ok: false, refused: 'bad-slot:tool' }),
  });
  const { result } = renderHook(() => useAction(), within(stub));
  await act(() => result.current.run({ name: 'projects.remove', args: { project: 'p0a1b2c3d' } }));
  expect(result.current.note).toBe('Project removed from the list. Add brings it back.');
  await act(() => result.current.run({ name: 'projects.add', args: {} }));
  expect(result.current.note).toBe('Not done: cancelled.');
  await act(() => result.current.run({ name: 'settings.reset', args: { tool: 'keel' } }));
  expect(result.current.note).toBe('Keel refused the request (bad-slot:tool).');
  stub.answer('alert.test', () => Promise.reject(new Error('the bridge is gone')));
  await act(() => result.current.run({ name: 'alert.test', args: {} }));
  expect(result.current).toMatchObject({ busy: false, note: 'The request failed: the bridge is gone.' });
});

test('sinceText: the time today, the day and time on another day, nothing for none or nonsense', () => {
  const now = new Date(2026, 10, 22, 12, 0, 0);
  const today = sinceText(new Date(2026, 10, 22, 8, 41, 0).toISOString(), now);
  expect(today).toMatch(/^since \S*08.41/);
  const earlier = sinceText(new Date(2026, 10, 20, 9, 5, 0).toISOString(), now);
  expect(earlier).toMatch(/^since .+, \S*09.05/);
  expect(earlier).not.toBe(today);
  expect(sinceText(null, now)).toBe('');
  expect(sinceText('yesterday', now)).toBe('');
});

test('useWaiting: one waiting.get on mount, one more per waiting.changed; a failure keeps the last view and says why', async () => {
  const view = { project: null } as unknown as View;
  let answer: Answer<View> = ok(view);
  const stub = makeStub({ 'waiting.get': () => answer });
  const { result } = renderHook(() => useWaiting(), { wrapper: ({ children }: { children: ReactNode }) => <BridgeProvider bridge={stub.bridge}>{children}</BridgeProvider> });
  await waitFor(() => expect(result.current.view).toEqual(view));
  expect(stub.names()).toEqual(['waiting.get']);
  answer = { ok: false, failed: 'gone' };
  act(() => void stub.push('waiting.changed'));
  await waitFor(() => expect(result.current.error).toBe('The request failed: gone.'));
  expect(result.current.view).toEqual(view);
  expect(stub.names()).toEqual(['waiting.get', 'waiting.get']);
});
