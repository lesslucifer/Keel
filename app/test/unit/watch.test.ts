// The watcher's timing, on fake timers and a fake file watch: a burst is one read after the debounce,
// the minute's re-read comes anyway, reads never overlap, a watch that fails is armed again, stop ends it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Watcher, gitRelevant, DEBOUNCE_MS, REREAD_MS, type Timers, type WatchFn, type WatchTarget } from '../../src/main/watcher/watch.ts';

/** Timers driven by advance(), never by the clock. */
function fakeTimers() {
  let now = 0;
  let seq = 0;
  const jobs = new Map<number, { at: number; fn: () => void; every: number | null }>();
  const timers: Timers = {
    setTimeout: (fn, ms) => (jobs.set(++seq, { at: now + ms, fn, every: null }), seq),
    clearTimeout: (h) => void jobs.delete(h as number),
    setInterval: (fn, ms) => (jobs.set(++seq, { at: now + ms, fn, every: ms }), seq),
    clearInterval: (h) => void jobs.delete(h as number),
  };
  const advance = async (ms: number) => {
    const end = now + ms;
    for (;;) {
      const due = [...jobs.entries()].filter(([, j]) => j.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
      if (due === undefined) break;
      const [id, job] = due;
      now = job.at;
      if (job.every === null) jobs.delete(id);
      else job.at += job.every;
      job.fn();
      await settle();
    }
    now = end;
    await settle();
  };
  return { timers, advance, pending: () => jobs.size };
}

const settle = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

/** A file watch the test fires by hand; it records what is armed and closed, and may refuse a folder. */
function fakeWatch(refuse: Set<string> = new Set()) {
  const armed = new Map<string, (file: string | null) => void>();
  const closed: string[] = [];
  const watch: WatchFn = (dir, _recursive, onEvent) => {
    if (refuse.has(dir)) throw new Error(`ENOENT: ${dir}`);
    armed.set(dir, onEvent);
    return { close: () => (closed.push(dir), armed.delete(dir)) };
  };
  const fire = (dir: string, file: string | null) => armed.get(dir)?.(file);
  return { watch, armed, closed, fire };
}

const GIT: WatchTarget = { dir: '/p/.git', recursive: true, relevant: gitRelevant };
const LOG: WatchTarget = { dir: '/p/.router/app', recursive: false, relevant: () => true };

function world(opts: { refuse?: Set<string> } = {}) {
  const t = fakeTimers();
  const w = fakeWatch(opts.refuse);
  let reads = 0;
  let running = 0;
  let maxRunning = 0;
  let slow = false;
  let release: (() => void) | null = null;
  const read = async () => {
    reads++;
    running++;
    maxRunning = Math.max(maxRunning, running);
    if (slow) await new Promise<void>((r) => (release = r));
    running--;
  };
  const watcher = new Watcher({ targets: () => [GIT, LOG], read, watch: w.watch, timers: t.timers });
  return { t, w, watcher, reads: () => reads, maxRunning: () => maxRunning, slow: () => (slow = true), release: () => release?.() };
}

test('the debounce is one second and the re-read one minute', () => {
  assert.equal(DEBOUNCE_MS, 1000);
  assert.equal(REREAD_MS, 60_000);
});

test('start reads once, then arms both folders; a burst of changes is one read, a second after the last', async () => {
  const x = world();
  await x.watcher.start();
  assert.equal(x.reads(), 1);
  assert.deepEqual([...x.w.armed.keys()].sort(), ['/p/.git', '/p/.router/app']);
  for (let i = 0; i < 5; i++) {
    x.w.fire('/p/.git', 'refs/heads/main');
    await x.t.advance(300);
  }
  assert.equal(x.reads(), 1, 'still within the debounce of the last event');
  await x.t.advance(DEBOUNCE_MS);
  assert.equal(x.reads(), 2);
  x.w.fire('/p/.router/app', 'log.jsonl');
  await x.t.advance(DEBOUNCE_MS);
  assert.equal(x.reads(), 3);
});

test('a change git makes outside refs (objects, the index, logs) is not a reason to read', async () => {
  const x = world();
  await x.watcher.start();
  for (const f of ['objects/ab/cdef', 'index', 'index.lock', 'logs/HEAD', 'COMMIT_EDITMSG', 'FETCH_HEAD']) x.w.fire('/p/.git', f);
  await x.t.advance(DEBOUNCE_MS * 2);
  assert.equal(x.reads(), 1);
});

test('the git files that are reasons: a branch or notes ref, packed refs, HEAD, a worktree\'s HEAD, or a change with no name', () => {
  for (const f of ['refs/heads/wu/E-A/w1', 'refs/notes/mr', 'refs/heads/main.lock', 'packed-refs', 'HEAD', 'worktrees/w2/HEAD', null]) assert.equal(gitRelevant(f), true, String(f));
  for (const f of ['objects/12/34', 'index', 'logs/refs/heads/main', 'worktrees/w2/index', 'hooks/pre-commit', 'config']) assert.equal(gitRelevant(f), false, String(f));
});

test('with no event at all, the minute\'s re-read reads anyway', async () => {
  const x = world();
  await x.watcher.start();
  await x.t.advance(REREAD_MS - 1);
  assert.equal(x.reads(), 1);
  await x.t.advance(1);
  assert.equal(x.reads(), 2);
  await x.t.advance(REREAD_MS);
  assert.equal(x.reads(), 3);
});

test('reads never overlap: changes during a read make one more read after it, not two at once', async () => {
  const x = world();
  await x.watcher.start();
  x.slow();
  x.w.fire('/p/.git', 'refs/heads/main');
  await x.t.advance(DEBOUNCE_MS);
  assert.equal(x.reads(), 2, 'the change started a read, which waits');
  x.w.fire('/p/.git', 'refs/notes/cr');
  await x.t.advance(DEBOUNCE_MS);
  x.w.fire('/p/.git', 'refs/notes/mr');
  await x.t.advance(DEBOUNCE_MS);
  assert.equal(x.reads(), 2, 'no second read while one runs');
  x.release();
  await settle();
  assert.equal(x.reads(), 3, 'the changes during it make exactly one more');
  x.release();
  await settle();
  assert.equal(x.maxRunning(), 1);
  assert.equal(x.reads(), 3);
});

test('a folder that cannot be watched yet (no .router/app) is armed at the next minute, once it can be', async () => {
  const refuse = new Set(['/p/.router/app']);
  const x = world({ refuse });
  await x.watcher.start();
  assert.deepEqual([...x.w.armed.keys()], ['/p/.git']);
  refuse.clear();
  await x.t.advance(REREAD_MS);
  assert.deepEqual([...x.w.armed.keys()].sort(), ['/p/.git', '/p/.router/app']);
});

test('when the targets change (a project removed or added), the old watches close and the new ones arm', async () => {
  const t = fakeTimers();
  const w = fakeWatch();
  let targets: WatchTarget[] = [GIT];
  const watcher = new Watcher({ targets: () => targets, read: async () => undefined, watch: w.watch, timers: t.timers });
  await watcher.start();
  targets = [];
  await watcher.kick();
  assert.deepEqual([...w.armed.keys()], []);
  assert.deepEqual(w.closed, ['/p/.git']);
  targets = [{ ...GIT, dir: '/q/.git' }];
  await watcher.kick();
  assert.deepEqual([...w.armed.keys()], ['/q/.git']);
});

test('stop closes every watch and clears every timer: no read after it', async () => {
  const x = world();
  await x.watcher.start();
  x.w.fire('/p/.git', 'refs/heads/main');
  x.watcher.stop();
  assert.deepEqual(x.w.closed.sort(), ['/p/.git', '/p/.router/app']);
  assert.equal(x.t.pending(), 0);
  await x.t.advance(REREAD_MS * 2);
  assert.equal(x.reads(), 1);
});

test('a read that throws is not the end: the next change reads again', async () => {
  const t = fakeTimers();
  const w = fakeWatch();
  let reads = 0;
  const watcher = new Watcher({ targets: () => [GIT], read: async () => { reads++; throw new Error('boom'); }, watch: w.watch, timers: t.timers });
  await watcher.start();
  w.fire('/p/.git', 'refs/heads/main');
  await t.advance(DEBOUNCE_MS);
  assert.equal(reads, 2);
});
