// Open at its real boundary: a temp keel project with committed docs, real git through the fixed reads,
// a temp app folder, and a recording opener in place of the browser. The page opened is the committed
// one, copied with the docs beside it, even while the working file differs; an epic result opens in place.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, readFileSync, existsSync, readdirSync, symlinkSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { openItem, KEPT_COPIES } from '../../src/main/open/open.ts';
import { openFileSink } from '../../src/main/alerter/seams.ts';
import { GitReader } from '../../src/main/reader/git.ts';
import { itemsOf } from '../../src/shared/items.ts';
import type { GitFacts, Item, RouterView } from '../../src/shared/contract.ts';
import { realGit, sh, tempDir, mk, snapshot } from '../support/fs.ts';

const GIT = realGit();
const rev = (dir: string, ref: string) => sh(GIT, dir, ['rev-parse', ref]).trim();

const PLAN_YAML = 'artifact: wu-plan\nstatus: ready\nid: E-A.W1\nepic: E-A\npage: 2026-11-22-e-a-w1-plan.html\n';
const PLAN_PAGE = '<!DOCTYPE html><html><body><p>The committed plan.</p><a href="../other.html">other</a></body></html>\n';
const RESULT_YAML = 'artifact: wu-result\nstatus: ready\nid: E-A.W1\nepic: E-A\npage: 2026-11-22-e-a-w1-result.html\n';

function world(t: { after: (fn: () => void) => void }) {
  const root = tempDir(t, 'open');
  const dir = mk(path.join(root, 'proj'));
  mkdirSync(path.join(dir, '.keel'));
  writeFileSync(path.join(dir, '.keel', 'config.toml'), 'code_paths = ["src/"]\n');
  mkdirSync(path.join(dir, 'docs', 'work'), { recursive: true });
  writeFileSync(path.join(dir, 'docs', 'other.html'), '<p>Another doc, linked from the plan.</p>\n');
  writeFileSync(path.join(dir, 'README.md'), 'not a doc\n');
  sh(GIT, dir, ['init', '-q']);
  sh(GIT, dir, ['add', '.']);
  sh(GIT, dir, ['commit', '-q', '-m', 'init']);
  sh(GIT, dir, ['branch', '-M', 'main']);
  sh(GIT, dir, ['branch', 'epic/E-A']);
  sh(GIT, dir, ['switch', '-q', '-c', 'wu/E-A/w1-reader', 'epic/E-A']);
  writeFileSync(path.join(dir, 'docs', 'work', '2026-11-22-e-a-w1-plan.yaml'), PLAN_YAML);
  writeFileSync(path.join(dir, 'docs', 'work', '2026-11-22-e-a-w1-plan.html'), PLAN_PAGE);
  symlinkSync('/etc/hosts', path.join(dir, 'docs', 'hosts.html'));
  sh(GIT, dir, ['add', '.']);
  sh(GIT, dir, ['commit', '-q', '-m', 'the plan']);
  const appDir = path.join(root, 'Keel');
  const opened: string[] = [];
  const parts = {
    appDir,
    project: { id: 'p0a1b2c3d', path: dir },
    git: GIT,
    runOptions: { path: [path.dirname(GIT), '/usr/bin', '/bin'], source: { HOME: root, LANG: 'C' } },
    openUrl: async (url: string) => {
      opened.push(url);
    },
  };
  const facts = async (): Promise<GitFacts> => new GitReader(dir, parts.runOptions).read(GIT);
  return { root, dir, appDir, opened, parts, facts };
}

const heard = (values: Record<string, unknown>): RouterView => ({ run: 'keel-1c.20261122-080000.1', n: 3, values, heard: true, lastAt: new Date().toISOString(), problem: null });

test('Open on a plan copies the tip\'s docs into the app folder and opens the committed page, even while the working file differs; the links between docs resolve', async (t) => {
  const w = world(t);
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  writeFileSync(path.join(w.dir, 'docs', 'work', '2026-11-22-e-a-w1-plan.html'), '<p>An edit nobody committed.</p>\n');
  const facts = await w.facts();
  const plan = itemsOf({ git: facts, router: heard({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: tip.slice(0, 7) } }), sent: new Set() }).find((i) => i.kind === 'plan') as Item;
  const before = snapshot(w.dir);
  assert.deepEqual(await openItem(plan, facts, w.parts), { done: true });
  assert.equal(w.opened.length, 1);
  const page = fileURLToPath(w.opened[0] as string);
  assert.ok(page.startsWith(path.join(w.appDir, 'open', 'p0a1b2c3d', tip) + path.sep), page);
  assert.equal(readFileSync(page, 'utf8'), PLAN_PAGE, 'the committed bytes, not the working file');
  assert.equal(readFileSync(page, 'utf8'), sh(GIT, w.dir, ['show', `${tip}:docs/work/2026-11-22-e-a-w1-plan.html`]));
  assert.equal(readFileSync(path.resolve(path.dirname(page), '../other.html'), 'utf8'), '<p>Another doc, linked from the plan.</p>\n', 'a relative link resolves in the copy');
  const copy = path.join(w.appDir, 'open', 'p0a1b2c3d', tip);
  assert.equal(existsSync(path.join(copy, 'docs', 'hosts.html')), false, 'a link in the tree is not copied');
  assert.equal(existsSync(path.join(copy, 'README.md')), false, 'only docs/');
  assert.equal(statSync(page).mode & 0o777, 0o600);
  assert.deepEqual(snapshot(w.dir), before, 'nothing written into the project');
  // A second Open of the same tip opens the same copy.
  assert.deepEqual(await openItem(plan, facts, w.parts), { done: true });
  assert.equal(w.opened[1], w.opened[0]);
});

test('Open on a result finds the result page through its data file at the tip; with none it says so and opens nothing', async (t) => {
  const w = world(t);
  sh(GIT, w.dir, ['notes', '--ref=cr', 'add', '-m', '{"schema":"cr/v1","verdict":"PASS","blockers":0,"importants":0}', 'wu/E-A/w1-reader']);
  let facts = await w.facts();
  let result = itemsOf({ git: facts, router: heard({}), sent: new Set() }).find((i) => i.kind === 'result') as Item;
  const none = await openItem(result, facts, w.parts);
  assert.deepEqual(none, { done: false, why: 'no wu-result page for E-A.W1 at its tip' });
  assert.equal(w.opened.length, 0);
  writeFileSync(path.join(w.dir, 'docs', 'work', '2026-11-22-e-a-w1-result.yaml'), RESULT_YAML);
  writeFileSync(path.join(w.dir, 'docs', 'work', '2026-11-22-e-a-w1-result.html'), '<p>The result.</p>\n');
  sh(GIT, w.dir, ['add', '.']);
  sh(GIT, w.dir, ['commit', '-q', '-m', 'the result']);
  sh(GIT, w.dir, ['notes', '--ref=cr', 'add', '-m', '{"schema":"cr/v1","verdict":"PASS","blockers":0,"importants":0}', 'wu/E-A/w1-reader']);
  facts = await w.facts();
  result = itemsOf({ git: facts, router: heard({}), sent: new Set() }).find((i) => i.kind === 'result') as Item;
  assert.deepEqual(await openItem(result, facts, w.parts), { done: true });
  assert.match(w.opened[0] as string, /\/docs\/work\/2026-11-22-e-a-w1-result\.html$/);
});

test('Open on an item whose tip moved since it was listed opens nothing', async (t) => {
  const w = world(t);
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const facts = await w.facts();
  const plan = itemsOf({ git: facts, router: heard({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: tip.slice(0, 7) } }), sent: new Set() }).find((i) => i.kind === 'plan') as Item;
  sh(GIT, w.dir, ['commit', '-q', '--allow-empty', '-m', 'moved']);
  const moved = await openItem(plan, await w.facts(), w.parts);
  assert.equal(moved.done, false);
  assert.equal(w.opened.length, 0);
});

test('Open on an epic result opens the page in the epic\'s worktree\'s docs/.archive in place, found through its data file; a page outside it is refused', async (t) => {
  const w = world(t);
  const wt = path.join(w.root, 'epic-wt');
  sh(GIT, w.dir, ['worktree', 'add', '-q', wt, 'epic/E-A']);
  const archive = mk(path.join(wt, 'docs', '.archive'));
  writeFileSync(path.join(archive, '2026-11-22-e-a-result.yaml'), 'artifact: epic-result\nstatus: ready\nid: E-A\npage: 2026-11-22-e-a-result.html\n');
  writeFileSync(path.join(archive, '2026-11-22-e-a-result.html'), '<p>The epic result.</p>\n');
  writeFileSync(path.join(archive, 'other.yaml'), 'artifact: epic-result\nid: E-B\npage: ../../escape.html\n');
  const facts = await w.facts();
  const epic: Item = { id: 'epic-result/E-A@aaaaaaaaaaaa', kind: 'epic-result', group: 'yours', wu: null, epic: 'E-A', title: 'Epic result to sign', branch: 'epic/E-A', tip: rev(w.dir, 'epic/E-A').slice(0, 7), since: null, why: 'x', open: 'archive', markable: false, worker: null, line: null, text: [] };
  assert.deepEqual(await openItem(epic, facts, w.parts), { done: true });
  assert.equal(fileURLToPath(w.opened[0] as string), path.join(archive, '2026-11-22-e-a-result.html'));
  assert.deepEqual(await openItem({ ...epic, epic: 'E-B' }, facts, w.parts), { done: false, why: 'no epic-result page for E-B in docs/.archive of epic/E-A\'s worktree' });
  sh(GIT, w.dir, ['worktree', 'remove', '--force', wt]);
  assert.deepEqual(await openItem(epic, await w.facts(), w.parts), { done: false, why: 'epic/E-A has no worktree here, so its result in docs/.archive cannot be found' });
  assert.equal(w.opened.length, 1);
});

test('the app folder keeps the last KEPT_COPIES copies per project', async (t) => {
  const w = world(t);
  const facts0 = await w.facts();
  void facts0;
  for (let i = 0; i < KEPT_COPIES + 2; i++) {
    sh(GIT, w.dir, ['commit', '-q', '--allow-empty', '-m', `step ${i}`]);
    const tip = rev(w.dir, 'wu/E-A/w1-reader');
    const facts = await w.facts();
    const plan = itemsOf({ git: facts, router: heard({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: tip.slice(0, 7) } }), sent: new Set() }).find((x) => x.kind === 'plan') as Item;
    assert.deepEqual(await openItem(plan, facts, w.parts), { done: true });
  }
  const kept = readdirSync(path.join(w.appDir, 'open', 'p0a1b2c3d')).filter((n) => !n.startsWith('.'));
  assert.equal(kept.length, KEPT_COPIES);
  assert.ok(kept.includes(rev(w.dir, 'wu/E-A/w1-reader')), 'the newest copy is kept');
});

test('the open sink appends each URL Open would hand the browser as one JSON line, owner-only', (t) => {
  const dir = tempDir(t, 'open-sink');
  const file = path.join(dir, 'opened.jsonl');
  const sink = openFileSink(file);
  sink('file:///x/a.html');
  sink('file:///x/b.html');
  const lines = readFileSync(file, 'utf8').trim().split('\n').map((l) => JSON.parse(l) as { url: string; at: string });
  assert.deepEqual(lines.map((l) => l.url), ['file:///x/a.html', 'file:///x/b.html']);
  for (const l of lines) assert.ok(!Number.isNaN(Date.parse(l.at)));
  assert.equal(statSync(file).mode & 0o777, 0o600);
});
