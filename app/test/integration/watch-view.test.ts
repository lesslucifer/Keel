// The view reader and the watcher at their real boundary: a temp keel project with real git and the
// real router-app.sh, watched by real fs.watch; each change reaches the view and pushes
// waiting.changed, within a bounded wait (never a sleep). The shell's parts are faked as W7's are.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdirSync, watch } from 'node:fs';
import path from 'node:path';
import { ViewReader } from '../../src/main/reader/view.ts';
import { Watcher, nodeWatch, type WatchFn } from '../../src/main/watcher/watch.ts';
import { realGit, sh, within, tempDir } from '../support/fs.ts';
import { routerProject, routerApp, routerState } from '../support/router.ts';

const GIT = realGit();

/**
 * Whether this machine delivers file events to fs.watch: a sandbox may refuse them (macOS answers
 * EMFILE when the file-event service is closed to the process). Where it does not, the real-watch
 * test is skipped and says so; the minute's re-read, tested below with the watch off, still holds.
 */
async function fileEventsWork(t: { after: (fn: () => void) => void }): Promise<boolean> {
  const dir = tempDir(t, 'fs-events');
  let seen = false;
  let failed = false;
  try {
    const w = watch(dir, { persistent: false }, () => (seen = true));
    w.on('error', () => (failed = true));
    t.after(() => w.close());
  } catch {
    return false;
  }
  writeFileSync(path.join(dir, 'probe'), 'x');
  await within(2_000, () => seen || failed);
  return seen && !failed;
}

function keelProject(t: { after: (fn: () => void) => void }): string {
  const dir = routerProject(t, 'watch-view');
  mkdirSync(path.join(dir, '.keel'));
  writeFileSync(path.join(dir, '.keel', 'config.toml'), 'code_paths = ["src/"]\n');
  sh(GIT, dir, ['add', '.keel']);
  sh(GIT, dir, ['commit', '-q', '-m', 'init']);
  sh(GIT, dir, ['branch', 'epic/E-A']);
  return dir;
}

function reader(dir: string, over: Partial<ConstructorParameters<typeof ViewReader>[0]> = {}) {
  return new ViewReader({
    project: async () => ({ id: 'p0a1b2c3d', name: 'proj', path: dir }),
    git: async () => GIT,
    runOptions: { path: [path.dirname(GIT), '/usr/bin', '/bin'], source: { HOME: dir, LANG: 'C' } },
    ...over,
  });
}

/** A watcher on `r` with real timers: a short debounce, and a re-read that never comes within a test unless asked. */
function watching(t: { after: (fn: () => void) => void }, r: ViewReader, opts: { watch?: WatchFn; rereadMs?: number } = {}) {
  let pushes = 0;
  const w = new Watcher({
    targets: () => r.targets(),
    read: async () => {
      if (await r.read()) pushes++;
    },
    watch: opts.watch ?? nodeWatch,
    debounceMs: 50,
    rereadMs: opts.rereadMs ?? 3_600_000,
  });
  t.after(() => w.stop());
  return { w, pushes: () => pushes };
}

test('the view holds the project, its git facts and the router\'s values; a read with nothing new is no change', async (t) => {
  const dir = keelProject(t);
  routerApp(dir, 'start', 'keel-1c');
  routerApp(dir, 'stage', 'E-A.W1', 'building');
  const r = reader(dir);
  assert.equal(await r.read(), true);
  const v = r.view();
  assert.deepEqual(v.project, { id: 'p0a1b2c3d', name: 'proj' });
  assert.deepEqual(v.git?.branches.map((b) => b.name).sort(), ['epic/E-A', 'main']);
  assert.deepEqual(v.router.values, { 'wu/E-A.W1/stage': { stage: 'building' } });
  assert.equal(v.router.run, routerState(dir).run);
  assert.equal(typeof v.readAt, 'string');
  assert.equal(await r.read(), false);
});

test('no project listed: the view is empty; git not found: the view says so and still reads the router', async (t) => {
  const dir = keelProject(t);
  routerApp(dir, 'start', 'keel-1c');
  const none = reader(dir, { project: async () => null });
  await none.read();
  assert.deepEqual([none.view().project, none.view().git, none.view().router.run], [null, null, null]);
  assert.deepEqual(none.targets(), []);
  const noGit = reader(dir, { git: async () => null });
  await noGit.read();
  assert.match(noGit.view().git?.problem ?? '', /git was not found/);
  assert.equal(noGit.view().router.run, routerState(dir).run);
});

test('the router falls silent: the minute\'s read after 15 minutes turns heard off, and that is a change', async (t) => {
  const dir = keelProject(t);
  routerApp(dir, 'start', 'keel-1c');
  let now = Date.now();
  const r = reader(dir, { now: () => now });
  await r.read();
  assert.equal(r.view().router.heard, true);
  now = Date.parse(routerState(dir).at) + 15 * 60 * 1000;
  assert.equal(await r.read(), true);
  assert.equal(r.view().router.heard, false);
});

test('on real fs.watch, a commit, a note, a merge, a new worktree and a router line each reach the view and push', async (t) => {
  if (!(await fileEventsWork(t))) return t.skip('fs.watch gets no file events on this machine (a sandbox refuses them): run on the owner\'s Mac');
  const dir = keelProject(t);
  routerApp(dir, 'start', 'keel-1c');
  const r = reader(dir);
  const { w, pushes } = watching(t, r);
  await w.start();
  const step = async (what: string, change: () => void, seen: () => boolean) => {
    const before = pushes();
    change();
    assert.ok(await within(5_000, () => pushes() > before && seen()), `${what}: no push within 5 s`);
  };
  const tip = (b: string) => r.view().git?.branches.find((x) => x.name === b)?.tip;
  await step('a commit on a branch', () => {
    sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w1']);
    sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'w1']);
  }, () => tip('wu/E-A/w1') !== undefined);
  await step('a sign-off note', () => sh(GIT, dir, ['notes', '--ref=mr', 'add', '-m', '{}', 'wu/E-A/w1']), () => (r.view().git?.notes.mr.length ?? 0) === 1);
  await step('a merge into the epic', () => {
    sh(GIT, dir, ['switch', '-q', 'epic/E-A']);
    sh(GIT, dir, ['merge', '-q', '--no-ff', '-m', 'w1 in', 'wu/E-A/w1']);
  }, () => (r.view().git?.merged['epic/E-A'] ?? []).includes('wu/E-A/w1'));
  await step('a new worktree', () => sh(GIT, dir, ['worktree', 'add', '-q', '-b', 'wu/E-A/w2', path.join(path.dirname(dir), 'wt2'), 'epic/E-A']), () => (r.view().git?.worktrees.length ?? 0) === 2);
  await step('a router line', () => routerApp(dir, 'stage', 'E-A.W2', 'plan-to-read'), () => 'wu/E-A.W2/stage' in r.view().router.values);
});

test('with the file watch off, the minute\'s re-read still finds the change', async (t) => {
  const dir = keelProject(t);
  routerApp(dir, 'start', 'keel-1c');
  const r = reader(dir);
  const deaf: WatchFn = () => ({ close: () => undefined });
  const { w, pushes } = watching(t, r, { watch: deaf, rereadMs: 200 });
  await w.start();
  routerApp(dir, 'stage', 'E-A.W1', 'building');
  assert.ok(await within(5_000, () => pushes() > 0 && 'wu/E-A.W1/stage' in r.view().router.values));
});
