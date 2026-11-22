// Screen 6 at its boundary, the bridge: drawn in jsdom inside the real App and BridgeProvider against
// the stub bridge, which records each ask and lets the test push. Each control sends exactly its
// request and slot; every control is off while one runs; a push redraws from a new state.get; a
// refusal, a failure and any markup in a value are shown as text.
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { expect, test } from 'vitest';
import { TOOL_NAMES, type Answer } from '../../src/shared/contract.ts';
import { App } from '../../src/window/App.tsx';
import { BridgeProvider } from '../../src/window/bridge.ts';
import { deferred, makeStub, ok, sampleState, type Handler, type Stub } from '../support/stub-bridge.ts';

function draw(handlers: Parameters<typeof makeStub>[0] = {}): Stub {
  const stub = makeStub(handlers);
  render(
    <BridgeProvider bridge={stub.bridge}>
      <App start="projects" />
    </BridgeProvider>,
  );
  return stub;
}

/** The asks screen 6 made: the frame's own waiting.get (W6, for the tab's count) left out. */
const own = (stub: Stub) => stub.asks.filter((a) => a.name !== 'waiting.get');
const names = (stub: Stub) => own(stub).map((a) => a.name);

const ready = () => screen.findByRole('heading', { name: 'Project' });
const tool = (name: string) => screen.getByRole('group', { name });
/** The screen's own buttons, in <main>: the frame's tabs (W6) are not the screen's controls. */
const buttons = () => within(screen.getByRole('main')).getAllByRole('button');

test('on mount: one state.get, and the fixture drawn - the project, each tool with its path, source and problem, the PATH\'s source, the version', async () => {
  const stub = draw();
  await ready();
  expect(names(stub)).toEqual(['state.get']);
  expect(screen.getByRole('banner')).toHaveTextContent('Keel');
  expect(screen.getByRole('banner')).toHaveTextContent('keel');
  expect(screen.getByText('/Users/owner/code/keel')).toBeInTheDocument();
  expect(within(tool('keel')).getByText('/opt/homebrew/bin/keel')).toBeInTheDocument();
  expect(within(tool('keel')).getByText('found on the PATH')).toBeInTheDocument();
  expect(within(tool('git')).getByText('/usr/bin/git')).toBeInTheDocument();
  expect(within(tool('git')).getByText('set in Keel')).toBeInTheDocument();
  expect(within(tool('claude')).getByText('missing')).toBeInTheDocument();
  expect(within(tool('claude')).getByText('not found on the PATH')).toBeInTheDocument();
  expect(screen.getByText('From your login shell.')).toBeInTheDocument();
  expect(screen.getByRole('contentinfo')).toHaveTextContent('Keel 0.1.0');
});

test('the PATH\'s fallback is shown with why', async () => {
  draw({ 'state.get': () => ok(sampleState({ path: { entries: ['/usr/bin'], source: 'fallback', reason: 'the login shell did not answer within 5 s' } })) });
  expect(await screen.findByText('A fallback: the login shell did not answer within 5 s.')).toBeInTheDocument();
});

test('Remove sends projects.remove with the project\'s id, and says what happened', async () => {
  const stub = draw({ 'projects.remove': () => ok({ done: true }) });
  await ready();
  await userEvent.click(screen.getByRole('button', { name: 'Remove from the list' }));
  expect(own(stub).slice(1)).toEqual([{ name: 'projects.remove', args: { project: 'p0a1b2c3d' } }]);
  expect(await screen.findByRole('status')).toHaveTextContent('Project removed from the list. Add brings it back.');
});

test('with no project, the screen says so and Add sends projects.add with no arguments', async () => {
  const stub = draw({ 'state.get': () => ok(sampleState({ projects: [] })), 'projects.add': () => ok({ done: false, why: 'cancelled' }) });
  await ready();
  expect(screen.getByRole('banner')).toHaveTextContent('No project');
  expect(screen.queryByRole('button', { name: 'Remove from the list' })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Add a project…' }));
  expect(own(stub).slice(1)).toEqual([{ name: 'projects.add', args: {} }]);
  expect(await screen.findByRole('status')).toHaveTextContent('Not done: cancelled.');
});

test('Choose sends settings.choose with its tool, for each of the three tools', async () => {
  const stub = draw({ 'settings.choose': () => ok({ done: true }) });
  await ready();
  for (const name of TOOL_NAMES) await userEvent.click(within(tool(name)).getByRole('button', { name: 'Choose…' }));
  expect(own(stub).slice(1)).toEqual(TOOL_NAMES.map((t) => ({ name: 'settings.choose', args: { tool: t } })));
});

test('Use the PATH shows only for a tool set in Keel and sends settings.reset with its tool, for each of the three tools', async () => {
  const allSet = sampleState();
  for (const name of TOOL_NAMES) allSet.tools[name] = { path: `/Users/owner/bin/${name}`, source: 'set', problem: null };
  const stub = draw({ 'state.get': () => ok(allSet), 'settings.reset': () => ok({ done: true }) });
  await ready();
  for (const name of TOOL_NAMES) await userEvent.click(within(tool(name)).getByRole('button', { name: 'Use the PATH' }));
  expect(own(stub).slice(1)).toEqual(TOOL_NAMES.map((t) => ({ name: 'settings.reset', args: { tool: t } })));
});

test('Use the PATH is absent for a tool found on the PATH or missing', async () => {
  draw();
  await ready();
  expect(within(tool('keel')).queryByRole('button', { name: 'Use the PATH' })).not.toBeInTheDocument();
  expect(within(tool('claude')).queryByRole('button', { name: 'Use the PATH' })).not.toBeInTheDocument();
  expect(within(tool('git')).getByRole('button', { name: 'Use the PATH' })).toBeInTheDocument();
});

test('Send a test alert sends alert.test and shows each of the three outcomes in words', async () => {
  const outcomes = { shown: 'The test alert was shown.', 'not-shown': 'The test alert was not shown within 5 s. Check System Settings > Notifications > Keel.', 'not-supported': 'This Mac does not support alerts from Keel.' } as const;
  for (const [outcome, words] of Object.entries(outcomes)) {
    const stub = draw({ 'alert.test': () => ok(outcome) });
    await ready();
    await userEvent.click(screen.getByRole('button', { name: 'Send a test alert' }));
    expect(names(stub)).toEqual(['state.get', 'alert.test']);
    expect(await screen.findByRole('status')).toHaveTextContent(words);
    expect(screen.getByRole('status').textContent).toBe(words);
    cleanup();
  }
});

test('every control is off while a request is pending, and on again after its answer', async () => {
  const held = deferred<Answer>();
  draw({ 'settings.choose': () => held.promise });
  await ready();
  expect(buttons().length).toBeGreaterThanOrEqual(6);
  for (const b of buttons()) expect(b).toBeEnabled();
  await userEvent.click(within(tool('keel')).getByRole('button', { name: 'Choose…' }));
  for (const b of buttons()) expect(b, b.textContent ?? '').toBeDisabled();
  await act(async () => held.resolve(ok({ done: true })));
  for (const b of buttons()) expect(b, b.textContent ?? '').toBeEnabled();
});

test('a state.changed push causes exactly one new state.get, and the new state is drawn', async () => {
  const stub = draw();
  await ready();
  stub.answer('state.get', () => ok(sampleState({ projects: [{ id: 'p11111111', path: '/Users/owner/code/other', name: 'other' }] })));
  act(() => void stub.push('state.changed'));
  expect(await screen.findByText('/Users/owner/code/other')).toBeInTheDocument();
  expect(names(stub)).toEqual(['state.get', 'state.get']);
  expect(screen.queryByText('/Users/owner/code/keel')).not.toBeInTheDocument();
});

test('a refused and a failed answer each show as text', async () => {
  const stub = draw({ 'settings.reset': () => ({ ok: false, refused: 'bad-slot:tool' }), 'alert.test': () => ({ ok: false, failed: 'no Notification' }) });
  await ready();
  await userEvent.click(within(tool('git')).getByRole('button', { name: 'Use the PATH' }));
  expect(await screen.findByRole('status')).toHaveTextContent('Keel refused the request (bad-slot:tool).');
  await userEvent.click(screen.getByRole('button', { name: 'Send a test alert' }));
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('The request failed: no Notification.'));
  expect(names(stub)).toEqual(['state.get', 'settings.reset', 'alert.test']);
});

test('a later state.get that fails keeps the screen and shows the error as text', async () => {
  const stub = draw();
  await ready();
  stub.answer('state.get', () => ({ ok: false, failed: 'the store is unreadable' }));
  act(() => void stub.push('state.changed'));
  expect(await screen.findByRole('alert')).toHaveTextContent('The request failed: the store is unreadable.');
  expect(screen.getByText('/Users/owner/code/keel')).toBeInTheDocument();
});

test('a value holding markup is shown as its own characters, never as markup', async () => {
  const markup = '<b>x</b>';
  const s = sampleState({ projects: [{ id: 'p0a1b2c3d', path: `/Users/owner/${markup}`, name: markup }] });
  s.tools.git = { path: `/opt/${markup}/git`, source: 'set', problem: markup };
  draw({ 'state.get': () => ok(s), 'projects.remove': () => ok({ done: false, why: markup }) });
  await ready();
  expect(screen.getByText(`/Users/owner/${markup}`)).toBeInTheDocument();
  expect(within(tool('git')).getByText(`/opt/${markup}/git`)).toBeInTheDocument();
  expect(within(tool('git')).getByText(markup)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Remove from the list' }));
  expect(await screen.findByRole('status')).toHaveTextContent(`Not done: ${markup}.`);
  expect(document.querySelector('b')).toBeNull();
});

test('loading: the screen says so until state.get answers', async () => {
  const held = deferred<Answer>();
  draw({ 'state.get': () => held.promise as ReturnType<Handler> });
  expect(screen.getByText('Loading…')).toBeInTheDocument();
  await act(async () => held.resolve(ok(sampleState())));
  expect(await ready()).toBeInTheDocument();
});

test('could not read: a first state.get that is refused says so, as text', async () => {
  draw({ 'state.get': () => ({ ok: false, refused: 'other-page' }) });
  expect(await screen.findByRole('alert')).toHaveTextContent('Keel could not read its state. Keel refused the request (other-page).');
  expect(screen.queryByRole('button')).not.toBeInTheDocument();
});

test('mx3a\'s button rules: at most one solid black primary pill per section, every button a pill with a 1px edge', async () => {
  for (const state of [sampleState(), sampleState({ projects: [] })]) {
    draw({ 'state.get': () => ok(state) });
    await ready();
    const sections = [...document.querySelectorAll('section')];
    expect(sections.length).toBe(4);
    for (const section of sections) {
      const solid = [...section.querySelectorAll('button')].filter((b) => b.className.split(/\s+/).includes('bg-primary'));
      expect(solid.length, section.textContent ?? '').toBeLessThanOrEqual(1);
    }
    for (const b of buttons()) expect(b.className.split(/\s+/), b.textContent ?? '').toEqual(expect.arrayContaining(['rounded-pill', 'border']));
    cleanup();
  }
});

test('with no project, Add a project is the screen\'s one solid primary pill; every other button is an outline pill with no fill', async () => {
  draw({ 'state.get': () => ok(sampleState({ projects: [] })) });
  await ready();
  const solid = buttons().filter((b) => b.className.split(/\s+/).includes('bg-primary'));
  expect(solid.map((b) => b.textContent)).toEqual(['Add a project…']);
  for (const b of buttons().filter((x) => !solid.includes(x))) {
    const cls = b.className.split(/\s+/);
    expect(cls, b.textContent ?? '').toContain('border-outline');
    expect(cls.filter((c) => c.startsWith('bg-')), b.textContent ?? '').toEqual([]);
  }
});

test('Mistral\'s type: the screen title and every section head in New York, in sentence case, never uppercase', async () => {
  draw();
  await ready();
  const heads = [screen.getByRole('heading', { level: 1 }), ...screen.getAllByRole('heading', { level: 2 })];
  expect(heads.map((h) => h.textContent)).toEqual(['Projects', 'Project', 'Tools', 'PATH', 'Alerts']);
  for (const h of heads) {
    expect(h.className.split(/\s+/), h.textContent ?? '').toContain('font-serif');
    expect(h.className.split(/\s+/), h.textContent ?? '').not.toContain('uppercase');
  }
});

test('a plain white title bar over a 1px hairline: the canvas, not a coloured band', async () => {
  draw();
  await ready();
  const bar = screen.getByRole('banner').className.split(/\s+/);
  expect(bar).toEqual(expect.arrayContaining(['bg-canvas', 'border-b', 'border-edge']));
});

test('every card is white on a hairline: the card ground, a 1px edge, 12px corners', async () => {
  draw();
  await ready();
  for (const section of document.querySelectorAll('section')) {
    const card = section.querySelector('h2 + div');
    expect(card?.className.split(/\s+/), section.textContent ?? '').toEqual(expect.arrayContaining(['bg-card', 'border', 'border-edge', 'rounded-card']));
  }
});

test('no state by colour alone: each source badge says its state in words, each in its own pair of colours', async () => {
  draw();
  await ready();
  const badge = (t: string, words: string) => within(tool(t)).getByText(words).className.split(/\s+/);
  expect(badge('git', 'set in Keel')).toEqual(expect.arrayContaining(['bg-set', 'text-on-set']));
  expect(badge('keel', 'found on the PATH')).toEqual(expect.arrayContaining(['bg-found', 'text-on-found']));
  expect(badge('claude', 'missing')).toEqual(expect.arrayContaining(['bg-missing', 'text-on-missing']));
});

test('a problem, a fallback and an error are words in the problem colour', async () => {
  draw({ 'state.get': () => ok(sampleState({ path: { entries: ['/usr/bin'], source: 'fallback', reason: 'no answer' } })) });
  expect((await screen.findByText('A fallback: no answer.')).className.split(/\s+/)).toContain('text-problem');
  expect(within(tool('claude')).getByText('not found on the PATH').className.split(/\s+/)).toContain('text-problem');
});
