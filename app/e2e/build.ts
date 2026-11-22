// The epic test's build (E-APPLIST.W9, owner answer 2a): Vite's three bundles into app/.vite/, made by
// Forge's own Vite plugin from forge.config.ts, as `electron-forge package` makes them before it packs,
// and nothing more: no package, no fuse, no signature, no Electron copy. The plugin's config generator is
// not exported (its package exports '.' only), so the helper runs the plugin's public prePackage hook,
// which builds the main part and the preload, then the window, with the production defines
// (MAIN_WINDOW_VITE_DEV_SERVER_URL undefined, so the shell serves app://keel/ under the packaged policy).
// Forge hands its hooks a Listr task; `task()` is a stand-in with the one method the hook uses, so the
// helper needs no package beyond the app's own. It imports no package (keel check's node tests reach
// it); e2e/build-bundles.ts, run by `pnpm build:e2e`, hands it forge.config.ts.

import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** The app folder: package.json, forge.config.ts and .vite/ live here. */
export const APP_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** What the shell reads: package.json's main, the preload index.ts loads, the window's page (renderer main_window). */
export const OUTPUTS: readonly string[] = Object.freeze(['.vite/build/main.cjs', '.vite/build/preload.cjs', '.vite/renderer/main_window/index.html']);

/** One step of a task list, as Listr takes it. */
export interface Step {
  title?: string;
  task: (ctx: unknown, sub: Task) => unknown;
}

/** The part of Listr's task wrapper Forge's Vite plugin uses: a title it may set, and a list of subtasks. */
export interface Task {
  title: string;
  newListr(steps: Step[], opts?: { concurrent?: boolean | number }): { run(): Promise<void> };
}

const isList = (v: unknown): v is { run(): Promise<void> } => typeof v === 'object' && v !== null && typeof (v as { run?: unknown }).run === 'function';

/** A Listr task stand-in: a list runs its steps in order (all at once when concurrent); a step that returns a list runs it as its subtasks; the first failure fails the list. */
export function task(): Task {
  return {
    title: '',
    newListr(steps, opts = {}) {
      const one = async (s: Step) => {
        const out = await s.task(undefined, task());
        if (isList(out)) await out.run();
      };
      return {
        async run() {
          if (opts.concurrent === true || (typeof opts.concurrent === 'number' && opts.concurrent > 1)) {
            await Promise.all(steps.map(one));
            return;
          }
          for (const s of steps) await one(s);
        },
      };
    },
  };
}

/** The part of a Forge plugin the helper reads. */
interface PluginLike {
  name?: unknown;
  setDirectories?: unknown;
  getHooks?: unknown;
}

/** The one Vite plugin in a Forge config: named 'vite', with setDirectories and getHooks; refused by name otherwise. */
export function vitePluginOf(config: { plugins?: readonly unknown[] }): { setDirectories(dir: string): void; getHooks(): Record<string, unknown> } {
  const found = (config.plugins ?? []).filter((p): p is PluginLike => typeof p === 'object' && p !== null && (p as PluginLike).name === 'vite');
  if (found.length === 0) throw new Error('forge.config.ts has no Vite plugin');
  if (found.length > 1) throw new Error(`forge.config.ts has ${found.length} Vite plugins`);
  const p = found[0] as PluginLike;
  if (typeof p.setDirectories !== 'function' || typeof p.getHooks !== 'function') throw new Error('the Vite plugin in forge.config.ts has no setDirectories or getHooks');
  return p as { setDirectories(dir: string): void; getHooks(): Record<string, unknown> };
}

/** The Vite plugin's prePackage hook (Forge keeps a hook as one function or a list of them): exactly one. */
function prePackageOf(hooks: Record<string, unknown>): (this: Task, ...args: unknown[]) => unknown {
  const h = hooks.prePackage;
  const list = Array.isArray(h) ? h : h === undefined ? [] : [h];
  if (list.length !== 1 || typeof list[0] !== 'function') throw new Error('the Vite plugin has no single prePackage hook');
  return list[0] as (this: Task, ...args: unknown[]) => unknown;
}

/** Build the bundles into `dir`/.vite through the config's Vite plugin alone; fails naming each output the build did not leave. */
export async function buildBundles(
  config: { plugins?: readonly unknown[] },
  dir: string,
  io: { exists?: (file: string) => boolean; log?: (line: string) => void } = {},
): Promise<void> {
  const exists = io.exists ?? existsSync;
  const log = io.log ?? ((line: string) => console.log(line));
  const plugin = vitePluginOf(config);
  plugin.setDirectories(dir);
  const root = task();
  const out = await prePackageOf(plugin.getHooks()).call(root, config);
  if (isList(out)) await out.run();
  const missing = OUTPUTS.filter((o) => !exists(path.join(dir, o)));
  if (missing.length > 0) throw new Error(`the build left no ${missing.join(', ')}`);
  for (const o of OUTPUTS) log(`built ${o}`);
}
