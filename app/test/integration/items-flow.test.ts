// E-APPLIST.W6's contract at its real boundary: each kind of item enters and leaves the list through
// waiting.get on the real dispatcher, as real git (a temp keel project: main with an upstream, an epic,
// work units, a task, review and sign-off notes) and the real scripts/router-app.sh change under it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { dispatcher, type EventLike } from '../../src/main/bridge.ts';
import { makeHandlers } from '../../src/main/handlers.ts';
import { ViewReader } from '../../src/main/reader/view.ts';
import type { Item, View } from '../../src/shared/contract.ts';
import { realGit, sh, tempDir, snapshot } from '../support/fs.ts';
import { routerProject, routerApp } from '../support/router.ts';

const GIT = realGit();
const PAGE = 'app://keel/index.html';
const own: EventLike = { sender: { id: 1 }, senderFrame: { url: PAGE, parent: null } };
const rev = (dir: string, ref: string) => sh(GIT, dir, ['rev-parse', ref]).trim();
const cr = (verdict: string, blockers = 0, importants = 0) => JSON.stringify({ schema: 'cr/v1', verdict, blockers, importants, code_diff_hash: 'x' });

/** A keel project the router writes into: main pushed then one merge ahead later; epic/E-A; wu/E-A/w1-reader and wu/E-A/w2-writer with a task. */
function world(t: { after: (fn: () => void) => void }) {
  const dir = routerProject(t, 'items-flow');
  const root = path.dirname(dir);
  mkdirSync(path.join(dir, '.keel'));
  writeFileSync(path.join(dir, '.keel', 'config.toml'), 'code_paths = ["src/"]\n');
  writeFileSync(path.join(dir, '.gitignore'), '.router/\n');
  sh(GIT, dir, ['add', '.']);
  sh(GIT, dir, ['commit', '-q', '-m', 'init']);
  sh(GIT, dir, ['branch', '-M', 'main']);
  sh(GIT, root, ['init', '-q', '--bare', 'up.git']);
  sh(GIT, dir, ['remote', 'add', 'origin', path.join(root, 'up.git')]);
  sh(GIT, dir, ['push', '-q', '-u', 'origin', 'main']);
  sh(GIT, dir, ['branch', 'epic/E-A']);
  for (const unit of ['wu/E-A/w1-reader', 'wu/E-A/w2-writer']) {
    sh(GIT, dir, ['switch', '-q', '-c', unit, 'epic/E-A']);
    sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', `${unit} work`]);
  }
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w2-writer_probe', 'wu/E-A/w2-writer']);
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'task']);
  sh(GIT, dir, ['switch', '-q', 'main']);
  routerApp(dir, 'start', 'keel-1c');
  const sent = new Set<string>();
  const reader = new ViewReader({
    project: async () => ({ id: 'p0a1b2c3d', name: 'proj', path: dir }),
    git: async () => GIT,
    runOptions: { path: [path.dirname(GIT), '/usr/bin', '/bin'], source: { HOME: root, LANG: 'C' } },
    sent: async () => sent,
  });
  const ask = dispatcher(
    () => ({ contentsId: 1, pages: [PAGE] }),
    makeHandlers({
      appDir: path.join(tempDir(t, 'items-flow-app'), 'Keel'),
      version: '0.1.0',
      loginPath: { entries: [path.dirname(GIT)], source: 'login-shell', reason: null },
      chooseFolder: async () => null,
      chooseFile: async () => null,
      testAlert: async () => 'shown',
      push: () => undefined,
      view: () => reader.view(),
      open: async () => ({ done: true }),
      mark: async () => ({ done: true }),
    }),
  );
  /** Read the project again, then the list as waiting.get answers it. */
  const list = async (): Promise<Item[]> => {
    await reader.read();
    const a = (await ask(own, { name: 'waiting.get', args: {} })) as { ok: true; value: View };
    assert.equal(a.ok, true);
    assert.equal(a.value.git?.problem ?? null, null);
    return a.value.items;
  };
  const kinds = async () => (await list()).map((i) => `${i.kind}:${i.title}`);
  const git = (...args: string[]) => sh(GIT, dir, args);
  const worktree = (branch: string) => {
    const wt = path.join(root, branch.replaceAll('/', '-'));
    git('worktree', 'add', '-q', wt, branch);
    return wt;
  };
  return { dir, root, git, list, kinds, worktree, sent, reader };
}

test('a work unit\'s result enters on a PASS review note on its tip, turns to merge on the sign-off note, and leaves with the merge into its epic', async (t) => {
  const w = world(t);
  assert.deepEqual(await w.kinds(), ['task:Task into its work unit']);
  w.git('notes', '--ref=cr', 'add', '-m', cr('FAIL', 2, 1), 'wu/E-A/w1-reader');
  let items = await w.list();
  assert.deepEqual(items.map((i) => [i.group, i.title, i.why]).filter(([, t]) => t !== 'Task into its work unit'), [['not-yours', 'Review failed', '2 blockers and 1 important open: its worker fixes them.']]);
  // A fix round moves the tip; its new note passes.
  w.git('switch', '-q', 'wu/E-A/w1-reader');
  w.git('commit', '-q', '--allow-empty', '-m', 'fix');
  w.git('notes', '--ref=cr', 'add', '-m', cr('PASS'), 'HEAD');
  w.git('switch', '-q', 'main');
  items = await w.list();
  const sign = items.find((i) => i.kind === 'result') as Item;
  assert.deepEqual([sign.title, sign.group, sign.wu, sign.tip, sign.open], ['Result to sign', 'yours', 'E-A.W1', rev(w.dir, 'wu/E-A/w1-reader').slice(0, 7), 'page']);
  assert.match(sign.since ?? '', /^\d{4}-\d\d-\d\dT/);
  w.git('notes', '--ref=mr', 'add', '-m', '{"schema":"mr/v1"}', 'wu/E-A/w1-reader');
  const merge = (await w.list()).find((i) => i.kind === 'result') as Item;
  assert.deepEqual([merge.title, merge.id, merge.line], ['Result to merge', sign.id, 'git merge --no-ff --no-edit wu/E-A/w1-reader']);
  w.git('switch', '-q', 'epic/E-A');
  w.git('merge', '-q', '--no-ff', '-m', 'merge w1', 'wu/E-A/w1-reader');
  w.git('switch', '-q', 'main');
  assert.deepEqual((await w.list()).filter((i) => i.kind === 'result'), []);
});

test('an epic\'s result enters on a PASS note on its tip, turns to merge into main on the sign-off, and leaves with the merge, when main not pushed appears and then leaves with the push', async (t) => {
  const w = world(t);
  // The epic gets a commit of its own (its first unit merged in): an epic with none stays open in main (W5).
  w.git('switch', '-q', 'epic/E-A');
  w.git('merge', '-q', '--no-ff', '-m', 'merge w1', 'wu/E-A/w1-reader');
  w.git('switch', '-q', 'main');
  w.git('notes', '--ref=cr', 'add', '-m', cr('WAIVED', 1, 0), 'epic/E-A');
  let epic = (await w.list()).find((i) => i.kind === 'epic-result') as Item;
  assert.deepEqual([epic.title, epic.epic, epic.open, epic.why], ['Epic result to sign', 'E-A', 'archive', 'The review was waived with 1 blocker and 0 importants open. Your sign-off is missing.']);
  w.git('notes', '--ref=mr', 'add', '-m', '{"schema":"mr/v1"}', 'epic/E-A');
  epic = (await w.list()).find((i) => i.kind === 'epic-result') as Item;
  assert.equal(epic.title, 'Epic result to merge');
  w.git('merge', '-q', '--no-ff', '-m', 'merge E-A', 'epic/E-A');
  let items = await w.list();
  assert.deepEqual(items.map((i) => i.kind), ['push'], 'the epic left; its units and task closed with it');
  assert.match(items[0]?.why ?? '', /^main is [2-9] commits ahead of origin\/main\.$/);
  w.git('push', '-q', 'origin', 'main');
  items = await w.list();
  assert.deepEqual(items, []);
});

test('a plan to read enters on the router\'s stage and leaves on plan approved; a plan whose branch moves past it leaves too; marked sent it sits under Sent', async (t) => {
  const w = world(t);
  const tip = rev(w.dir, 'wu/E-A/w2-writer').slice(0, 7);
  routerApp(w.dir, 'stage', 'E-A.W2', 'plan-to-read', tip);
  let plan = (await w.list()).find((i) => i.kind === 'plan') as Item;
  assert.deepEqual([plan.title, plan.group, plan.wu, plan.branch, plan.tip, plan.markable], ['Plan to read', 'yours', 'E-A.W2', 'wu/E-A/w2-writer', tip, true]);
  w.sent.add(plan.id);
  plan = (await w.list()).find((i) => i.kind === 'plan') as Item;
  assert.equal(plan.group, 'sent');
  routerApp(w.dir, 'stage', 'E-A.W2', 'plan-approved', tip);
  assert.deepEqual((await w.list()).filter((i) => i.kind === 'plan'), []);
  routerApp(w.dir, 'stage', 'E-A.W2', 'plan-to-read', tip);
  assert.equal((await w.list()).filter((i) => i.kind === 'plan').length, 1);
  w.git('switch', '-q', 'wu/E-A/w2-writer');
  w.git('commit', '-q', '--allow-empty', '-m', 'build');
  w.git('switch', '-q', 'main');
  assert.deepEqual((await w.list()).filter((i) => i.kind === 'plan'), [], 'the branch moved past the plan');
  routerApp(w.dir, 'stage', 'E-A.W2', 'building');
  const motion = (await w.list()).find((i) => i.kind === 'motion') as Item;
  assert.deepEqual([motion.group, motion.title], ['not-yours', 'Building']);
});

test('a question and a worker waiting enter on their router lines and leave when the router removes or changes them; a router restart drops every router item and keeps what git proves', async (t) => {
  const w = world(t);
  routerApp(w.dir, 'set', 'question/merge-order', JSON.stringify({ headline: 'Merge W1 first?', lines: ['W2 builds on it.'], wu: 'E-A.W1' }));
  routerApp(w.dir, 'set', 'worker/e-a-w2', JSON.stringify({ state: 'waiting', waiting: 'a permission prompt', session: 'abc-123', branch: 'wu/E-A/w2-writer' }));
  let items = await w.list();
  assert.deepEqual(items.map((i) => `${i.kind}:${i.group}`), ['question:yours', 'worker:yours', 'task:yours']);
  const q = items[0] as Item;
  assert.deepEqual([q.why, q.text, q.wu, q.open], ['Merge W1 first?', ['W2 builds on it.'], 'E-A.W1', 'text']);
  assert.equal(items[1]?.line, 'claude attach abc-123');
  routerApp(w.dir, 'del', 'question/merge-order');
  routerApp(w.dir, 'set', 'worker/e-a-w2', JSON.stringify({ state: 'busy', branch: 'wu/E-A/w2-writer' }));
  items = await w.list();
  assert.deepEqual(items.map((i) => i.kind), ['task']);
  routerApp(w.dir, 'set', 'question/q2', JSON.stringify({ headline: 'Go?' }));
  w.git('notes', '--ref=cr', 'add', '-m', cr('PASS'), 'wu/E-A/w1-reader');
  assert.deepEqual((await w.list()).map((i) => i.kind), ['result', 'question', 'task']);
  routerApp(w.dir, 'start', 'keel-1c');
  assert.deepEqual((await w.list()).map((i) => i.kind), ['result', 'task'], 'a new run: the router has said nothing yet');
});

test('a task waits on the owner while its tip is outside its work unit and no busy worker is on it, and leaves once merged into it', async (t) => {
  const w = world(t);
  let task = (await w.list()).find((i) => i.kind === 'task') as Item;
  assert.deepEqual([task.wu, task.branch, task.line], ['E-A.W2', 'wu/E-A/w2-writer_probe', 'git merge --no-ff wu/E-A/w2-writer_probe']);
  routerApp(w.dir, 'set', 'worker/e-a-probe', JSON.stringify({ state: 'busy', branch: 'wu/E-A/w2-writer_probe' }));
  assert.deepEqual((await w.list()).filter((i) => i.kind === 'task'), [], 'a busy worker is on it');
  routerApp(w.dir, 'set', 'worker/e-a-probe', JSON.stringify({ state: 'idle', branch: 'wu/E-A/w2-writer_probe' }));
  task = (await w.list()).find((i) => i.kind === 'task') as Item;
  assert.equal(task.why, 'No worker is busy on it. The merge into wu/E-A/w2-writer is yours, in its folder.');
  const wt = w.worktree('wu/E-A/w2-writer');
  sh(GIT, wt, ['merge', '-q', '--no-ff', '-m', 'task in', 'wu/E-A/w2-writer_probe']);
  assert.deepEqual((await w.list()).filter((i) => i.kind === 'task'), []);
});

test('drawing the list writes nothing into the project', async (t) => {
  const w = world(t);
  w.git('notes', '--ref=cr', 'add', '-m', cr('PASS'), 'wu/E-A/w1-reader');
  routerApp(w.dir, 'stage', 'E-A.W2', 'plan-to-read', rev(w.dir, 'wu/E-A/w2-writer').slice(0, 7));
  const before = snapshot(w.dir);
  await w.list();
  await w.list();
  assert.deepEqual(snapshot(w.dir), before);
});

test('the view says when a read last found a change; a read that finds the same keeps it', async (t) => {
  const w = world(t);
  await w.reader.read();
  const first = w.reader.view().changedAt;
  assert.match(first ?? '', /^\d{4}-\d\d-\d\dT/);
  assert.equal(await w.reader.read(), false);
  assert.equal(w.reader.view().changedAt, first);
  routerApp(w.dir, 'set', 'question/q1', JSON.stringify({ headline: 'Go?' }));
  assert.equal(await w.reader.read(), true);
  assert.ok((w.reader.view().changedAt ?? '') >= (first ?? ''));
});
