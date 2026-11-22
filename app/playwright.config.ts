// The epic test's runner (E-APPLIST.W9, DEC-75): Playwright on the unpackaged app, run by hand with
// `pnpm e2e` (the bundles first, then this), never by keel check (DEC-73). One file, one worker, no
// retries; each named step and its outcome printed as it runs, so a run's output says which step failed
// and where. Traces and other output go under the OS temp folder (no new ignore entry), kept on failure only.

import os from 'node:os';
import path from 'node:path';
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e',
  testMatch: '*.e2e.spec.ts',
  workers: 1,
  fullyParallel: false,
  retries: 0,
  // The whole walk; each step waits at most expect's timeout for what it looks for.
  timeout: 6 * 60 * 1000,
  expect: { timeout: 20_000 },
  outputDir: path.join(os.tmpdir(), 'keel-app-e2e-results'),
  reporter: [['list', { printSteps: true }]],
  use: { trace: 'retain-on-failure' },
});
