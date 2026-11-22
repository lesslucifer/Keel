// The browser mode's entry against the fake: ?state= picks the scenario, the page says it runs on
// fixtures, and the screen works end to end (Add in no-project shows the project).
import { render, screen, within } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { expect, test } from 'vitest';
import { BrowserApp } from '../../src/window/browser.tsx';
import { SCENARIO_NAMES } from '../../src/window/fake/fixtures.ts';

test('?state=no-project draws the screen with no project, and Add then shows the fixture project', async () => {
  render(<BrowserApp search="?state=no-project" />);
  const add = await screen.findByRole('button', { name: /add a project/i });
  expect(screen.queryByText(/\/Users\/you\/code\/keel/)).not.toBeInTheDocument();
  await userEvent.click(add);
  expect(await screen.findByText(/\/Users\/you\/code\/keel/)).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /add a project/i })).not.toBeInTheDocument();
});

test('no ?state= draws the default, one project: it opens on Waits on you, and the Projects tab shows the project', async () => {
  render(<BrowserApp search="" />);
  expect(await screen.findByRole('heading', { level: 1, name: 'Waits on you' })).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Projects' }));
  expect(await screen.findByText(/\/Users\/you\/code\/keel/)).toBeInTheDocument();
});

test('the page says it runs on fixtures, names the scenario and links each one', async () => {
  render(<BrowserApp search="?state=tool-missing" />);
  expect(screen.getByText('Browser mode: fixtures, not your Mac')).toBeInTheDocument();
  const nav = screen.getByRole('navigation', { name: 'Fixture states' });
  for (const name of SCENARIO_NAMES) {
    const link = within(nav).getByRole('link', { name });
    expect(link).toHaveAttribute('href', `?state=${name}`);
  }
  expect(within(nav).getByRole('link', { name: 'tool-missing' })).toHaveAttribute('aria-current', 'page');
});

test('?scheme=dark and ?scheme=light force one world on the page\'s root; no ?scheme= follows the Mac', () => {
  const { unmount } = render(<BrowserApp search="?state=tool-missing&scheme=dark" />);
  expect(document.documentElement.dataset.scheme).toBe('dark');
  unmount();
  render(<BrowserApp search="?scheme=light" />).unmount();
  expect(document.documentElement.dataset.scheme).toBe('light');
  render(<BrowserApp search="?scheme=sepia" />);
  expect(document.documentElement.dataset.scheme).toBeUndefined();
});

test('the appearance links keep the scenario, and the scenario links keep the forced world', () => {
  render(<BrowserApp search="?state=tool-missing&scheme=dark" />);
  const looks = screen.getByRole('navigation', { name: 'Appearance' });
  expect(within(looks).getByRole('link', { name: 'Follow the Mac' })).toHaveAttribute('href', '?state=tool-missing');
  expect(within(looks).getByRole('link', { name: 'Light' })).toHaveAttribute('href', '?state=tool-missing&scheme=light');
  expect(within(looks).getByRole('link', { name: 'Dark' })).toHaveAttribute('aria-current', 'page');
  const states = screen.getByRole('navigation', { name: 'Fixture states' });
  expect(within(states).getByRole('link', { name: 'no-project' })).toHaveAttribute('href', '?state=no-project&scheme=dark');
});

test('the view links pick a waiting view and keep the scenario and the forced world; every other link keeps the view', () => {
  render(<BrowserApp search="?state=tool-missing&view=every-kind&scheme=dark" />);
  const views = screen.getByRole('navigation', { name: 'Fixture views' });
  expect(within(views).getByRole('link', { name: 'every-kind' })).toHaveAttribute('aria-current', 'page');
  expect(within(views).getByRole('link', { name: 'its own view' })).toHaveAttribute('href', '?state=tool-missing&scheme=dark');
  expect(within(views).getByRole('link', { name: 'not-heard' })).toHaveAttribute('href', '?state=tool-missing&view=not-heard&scheme=dark');
  const states = screen.getByRole('navigation', { name: 'Fixture states' });
  expect(within(states).getByRole('link', { name: 'no-project' })).toHaveAttribute('href', '?state=no-project&view=every-kind&scheme=dark');
  const looks = screen.getByRole('navigation', { name: 'Appearance' });
  expect(within(looks).getByRole('link', { name: 'Light' })).toHaveAttribute('href', '?state=tool-missing&view=every-kind&scheme=light');
});
