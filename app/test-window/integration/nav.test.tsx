// The frame's tabs (E-APPLIST.W6): Waits on you, with how many wait on you, and Projects; the screen
// the app opens on; and a screen kept once chosen, whatever changes under it.
import { render, screen, within } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { expect, test } from 'vitest';
import { App } from '../../src/window/App.tsx';
import { BridgeProvider } from '../../src/window/bridge.ts';
import { BrowserApp } from '../../src/window/browser.tsx';
import { VIEWS, noProjectView } from '../../src/window/fake/fixtures.ts';
import { makeStub, ok, sampleState } from '../support/stub-bridge.ts';

test('the tabs move between Waits on you and Projects, the current one marked, its title the screen\'s', async () => {
  const stub = makeStub({ 'waiting.get': () => ok(structuredClone(VIEWS.live)) });
  render(
    <BridgeProvider bridge={stub.bridge}>
      <App />
    </BridgeProvider>,
  );
  const tabs = await screen.findByRole('navigation', { name: 'Screens' });
  const waits = within(tabs).getByRole('button', { name: `Waits on you (${VIEWS.live.counts.yours})` });
  const projects = within(tabs).getByRole('button', { name: 'Projects' });
  expect(waits).toHaveAttribute('aria-current', 'page');
  await userEvent.click(projects);
  expect(projects).toHaveAttribute('aria-current', 'page');
  expect(waits).not.toHaveAttribute('aria-current');
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Projects');
  await userEvent.click(waits);
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Waits on you');
});

test('with no project listed the app opens on Projects and the tab shows no count; Add keeps it there', async () => {
  const state = sampleState({ projects: [] });
  const stub = makeStub({ 'state.get': () => ok(state), 'waiting.get': () => ok(noProjectView()), 'projects.add': () => ok({ done: true }) });
  render(
    <BridgeProvider bridge={stub.bridge}>
      <App />
    </BridgeProvider>,
  );
  expect(await screen.findByRole('heading', { level: 1, name: 'Projects' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Waits on you' })).toBeInTheDocument();
});

test('a screen once chosen stays: Add on Projects lists the project and the screen is still Projects', async () => {
  render(<BrowserApp search="?state=no-project" />);
  await userEvent.click(await screen.findByRole('button', { name: /add a project/i }));
  expect(await screen.findByText(/\/Users\/you\/code\/keel/)).toBeInTheDocument();
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Projects');
});
