// pnpm build:e2e (E-APPLIST.W9): Vite's three bundles of this app into app/.vite/, from its own
// forge.config.ts, through build.ts. Kept apart from build.ts so no node test reaches forge.config.ts
// and the packages it imports.

import config from '../forge.config.ts';
import { APP_ROOT, buildBundles } from './build.ts';

try {
  await buildBundles(config, APP_ROOT);
} catch (e) {
  console.error(`build:e2e failed: ${e instanceof Error ? e.message : String(e)}`);
  process.exitCode = 1;
}
