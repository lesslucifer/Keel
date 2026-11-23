// The harness itself: jsdom draws React, Testing Library finds by role, jest-dom's matchers are
// wired by test-window/support/setup.ts, and each test starts from an empty document.
import { expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';

test('the harness draws a React element in jsdom and jest-dom matchers read it', () => {
  render(<p role="status">ready</p>);
  expect(screen.getByRole('status')).toHaveTextContent('ready');
});

test('each test starts from an empty document: the previous test\'s element was cleaned up', () => {
  expect(screen.queryByRole('status')).not.toBeInTheDocument();
});
