// The epic test's build helper (app/e2e/build.ts, E-APPLIST.W9): Vite's three bundles made by Forge's
// own Vite plugin from forge.config.ts, through its prePackage hook only, so no packager, maker, fuse
// or signing step is ever reached; and the outputs it names are the ones the shell reads. No node test
// may import forge.config.ts (it imports Forge's packages: guards.test.ts), so its text is read here.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { OUTPUTS, vitePluginOf, buildBundles, task, type Step } from '../../e2e/build.ts';

const APP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

test('pnpm build:e2e hands forge.config.ts, with its one Vite plugin (the main part, the preload, the window), to the helper, and nothing else', () => {
  const pkg = JSON.parse(readFileSync(path.join(APP, 'package.json'), 'utf8')) as { scripts: Record<string, string> };
  assert.equal(pkg.scripts['build:e2e'], 'node e2e/build-bundles.ts');
  assert.equal(pkg.scripts.e2e, 'pnpm build:e2e && playwright test');
  const entry = readFileSync(path.join(APP, 'e2e', 'build-bundles.ts'), 'utf8');
  // Built from pieces, so this file's text holds no import guards.test.ts's walk would follow.
  assert.ok(entry.split('\n').includes(`${'imp' + 'ort'} config from '../forge.config.ts';`), 'build-bundles.ts takes the config from forge.config.ts');
  assert.match(entry, /^\s*await buildBundles\(config, APP_ROOT\);$/m);
  const forge = readFileSync(path.join(APP, 'forge.config.ts'), 'utf8');
  assert.equal(forge.match(/new VitePlugin\(/g)?.length, 1);
  assert.match(forge, /\{ entry: \{ main: 'src\/main\/index\.ts' \}, config: 'vite\.main\.config\.ts', target: 'main' \}/);
  assert.match(forge, /\{ entry: \{ preload: 'src\/preload\/index\.ts' \}, config: 'vite\.preload\.config\.ts', target: 'preload' \}/);
  assert.match(forge, /renderer: \[\{ name: 'main_window', config: 'vite\.renderer\.config\.ts' \}\]/);
});

test('the outputs it names are the ones the shell reads: package.json main, the preload index.ts loads, the window page under the renderer\'s name', () => {
  assert.deepEqual([...OUTPUTS], ['.vite/build/main.cjs', '.vite/build/preload.cjs', '.vite/renderer/main_window/index.html']);
  assert.ok(Object.isFrozen(OUTPUTS));
  const pkg = JSON.parse(readFileSync(path.join(APP, 'package.json'), 'utf8')) as { main: string };
  assert.equal(pkg.main, OUTPUTS[0]);
  const shell = readFileSync(path.join(APP, 'src', 'main', 'index.ts'), 'utf8');
  assert.match(shell, /path\.join\(__dirname, 'preload\.cjs'\)/);
  assert.match(shell, /path\.join\(__dirname, '\.\.', 'renderer', MAIN_WINDOW_VITE_NAME\)/);
});

test('no Vite plugin, or two, is refused by name', () => {
  assert.throws(() => vitePluginOf({ plugins: [] }), /forge\.config\.ts has no Vite plugin/);
  const fake = { name: 'vite', setDirectories() {}, getHooks: () => ({}) };
  assert.throws(() => vitePluginOf({ plugins: [fake, fake] }), /forge\.config\.ts has 2 Vite plugins/);
});

/** A Forge config whose every step but the Vite plugin's prePackage throws, and a record of what ran. */
function fakeForge(outputs: ReadonlySet<string>) {
  const ran: string[] = [];
  const boom = (what: string) => () => {
    throw new Error(`${what} was reached`);
  };
  const vite = {
    name: 'vite',
    setDirectories: (dir: string) => ran.push(`setDirectories ${dir}`),
    getHooks: () => ({
      preStart: [boom('preStart')],
      packageAfterCopy: boom('packageAfterCopy'),
      prePackage: [
        function (this: { newListr(steps: Step[], opts?: { concurrent?: boolean | number }): unknown }) {
          ran.push('prePackage');
          return this.newListr(
            [
              {
                title: 'main and preload',
                task: (_ctx: unknown, sub: { newListr(steps: Step[], opts?: { concurrent?: boolean | number }): unknown }) =>
                  sub.newListr(
                    [
                      { title: 'main', task: async () => void ran.push('build main') },
                      { title: 'preload', task: async () => void ran.push('build preload') },
                    ],
                    { concurrent: true },
                  ),
              },
              { title: 'renderer', task: async () => void ran.push('build renderer') },
            ],
            { concurrent: false },
          );
        },
      ],
    }),
  };
  const fuses = { name: 'fuses', getHooks: () => ({ packageAfterExtract: boom('fuses'), prePackage: [boom('fuses prePackage')] }) };
  const config = {
    packagerConfig: { osxSign: { optionsForFile: boom('signing') } },
    makers: [{ name: 'zip', make: boom('maker') }],
    plugins: [vite, fuses],
    hooks: { prePackage: boom('config hook'), postPackage: boom('config hook') },
  };
  const exists = (file: string) => outputs.has(file);
  return { config, ran, exists };
}

test('a build runs only the Vite plugin\'s prePackage steps, nested lists whole and in order; no packager, maker, fuse or signing step is reached', async () => {
  const dir = '/tmp/app';
  const f = fakeForge(new Set(OUTPUTS.map((o) => path.join(dir, o))));
  const said: string[] = [];
  await buildBundles(f.config, dir, { exists: f.exists, log: (s) => said.push(s) });
  assert.deepEqual(f.ran.slice(0, 2), ['setDirectories /tmp/app', 'prePackage']);
  assert.deepEqual(f.ran.slice(2, 4).sort(), ['build main', 'build preload']);
  assert.deepEqual(f.ran.slice(4), ['build renderer']);
  assert.deepEqual(said, OUTPUTS.map((o) => `built ${o}`));
});

test('a build whose outputs are not all there fails, naming each one missing', async () => {
  const dir = '/tmp/app';
  const f = fakeForge(new Set([path.join(dir, OUTPUTS[0] as string)]));
  await assert.rejects(buildBundles(f.config, dir, { exists: f.exists, log: () => undefined }), /the build left no \.vite\/build\/preload\.cjs, \.vite\/renderer\/main_window\/index\.html/);
});

test('the task stand-in runs a list in order, or at once when concurrent, runs a returned list as its subtasks, and fails on the first failing step', async () => {
  const order: string[] = [];
  const wait = (ms: number, s: string) => async () => {
    await new Promise((r) => setTimeout(r, ms));
    order.push(s);
  };
  await task().newListr([{ task: wait(15, 'a') }, { task: wait(1, 'b') }], { concurrent: false }).run();
  assert.deepEqual(order, ['a', 'b']);
  order.length = 0;
  await task().newListr([{ task: wait(15, 'a') }, { task: wait(1, 'b') }], { concurrent: true }).run();
  assert.deepEqual(order, ['b', 'a']);
  order.length = 0;
  await task().newListr([{ task: (_c, sub) => sub.newListr([{ task: wait(1, 'inner') }]) }, { task: wait(1, 'after') }]).run();
  assert.deepEqual(order, ['inner', 'after']);
  await assert.rejects(
    task()
      .newListr([
        {
          task: async () => {
            throw new Error('vite failed');
          },
        },
        { task: wait(1, 'never') },
      ])
      .run(),
    /vite failed/,
  );
  assert.ok(!order.includes('never'));
});
