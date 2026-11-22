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

test('no ?state= draws the default, one project', async () => {
  render(<BrowserApp search="" />);
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
