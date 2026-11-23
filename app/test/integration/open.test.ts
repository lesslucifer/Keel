// Open at its real boundary: a temp keel project with committed docs, real git through the fixed reads,
// a temp app folder, and a recording opener in place of the browser. The page opened is the committed
// one, copied with the docs beside it, even while the working file differs; an epic result opens in place.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, readFileSync, existsSync, readdirSync, symlinkSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { openItem, openListed, KEPT_COPIES } from '../../src/main/open/open.ts';
import { emptyView } from '../../src/main/reader/view.ts';
import { openFileSink } from '../../src/main/alerter/seams.ts';
import { GitReader } from '../../src/main/reader/git.ts';
import { itemsOf } from '../../src/shared/items.ts';
import type { GitFacts, Item, RouterView } from '../../src/shared/contract.ts';
import { realGit, sh, tempDir, mk, snapshot, program, oldGit, records } from '../support/fs.ts';

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

/** Commit `files` on the checked-out branch: each path with its bytes. */
function commitFiles(dir: string, files: Record<string, string | Buffer>, message: string) {
  for (const [p, bytes] of Object.entries(files)) {
    mkdirSync(path.dirname(path.join(dir, p)), { recursive: true });
    writeFileSync(path.join(dir, p), bytes);
  }
  sh(GIT, dir, ['add', '.']);
  sh(GIT, dir, ['commit', '-q', '-m', message]);
}

const planItem = async (w: ReturnType<typeof world>, tip: string): Promise<{ plan: Item; facts: GitFacts }> => {
  const facts = await w.facts();
  return { plan: itemsOf({ git: facts, router: heard({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: tip.slice(0, 7) } }), sent: new Set() }).find((i) => i.kind === 'plan') as Item, facts };
};

test('W6 review round 1 (docs.ts:52): the copy cap counts only what is copied, and the page and its data file are copied first: docs that sort before them and pass 8 MiB never crowd them out', async (t) => {
  const w = world(t);
  // Eight docs of 1 MiB each sort first and fill the 8 MiB cap exactly; a binary before them counted too, before.
  const big = 'x'.repeat(1024 * 1024 - 1) + '\n';
  const files: Record<string, string | Buffer> = { 'docs/a-binary.bin': Buffer.alloc(900_000, 0xff) };
  for (let i = 0; i < 8; i++) files[`docs/a-big/f${i}.html`] = big;
  files['docs/work/2026-11-22-e-a-w1-result.yaml'] = RESULT_YAML;
  files['docs/work/2026-11-22-e-a-w1-result.html'] = '<p>The result.</p>\n';
  commitFiles(w.dir, files, 'big docs');
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  assert.deepEqual(await openItem(plan, facts, w.parts), { done: true });
  assert.equal(readFileSync(fileURLToPath(w.opened[0] as string), 'utf8'), PLAN_PAGE);
  const copy = path.join(w.appDir, 'open', 'p0a1b2c3d', tip);
  const size = (d: string): number => readdirSync(d, { withFileTypes: true }).reduce((n, e) => n + (e.isDirectory() ? size(path.join(d, e.name)) : statSync(path.join(d, e.name)).size), 0);
  assert.ok(size(path.join(copy, 'docs')) <= 8 * 1024 * 1024, 'within the cap');
  assert.equal(existsSync(path.join(copy, 'docs', 'a-binary.bin')), false, 'a binary is not copied and not counted');
  // The result's page sorts after the big docs: its Open finds it whole in the kept copy, or rebuilds the copy with it first (round 2).
  sh(GIT, w.dir, ['notes', '--ref=cr', 'add', '-m', '{"schema":"cr/v1","verdict":"PASS","blockers":0,"importants":0}', 'wu/E-A/w1-reader']);
  const facts2 = await w.facts();
  const result = itemsOf({ git: facts2, router: heard({}), sent: new Set() }).find((i) => i.kind === 'result') as Item;
  assert.deepEqual(await openItem(result, facts2, w.parts), { done: true });
  assert.equal(readFileSync(fileURLToPath(w.opened[1] as string), 'utf8'), '<p>The result.</p>\n');
  assert.ok(size(path.join(copy, 'docs')) <= 8 * 1024 * 1024 + 2 * 1024 * 1024, 'at most the page and its data file over the cap');
});

test('W6 review round 1 (docs.ts:52): a copy whose page could not be copied is never kept as complete; Open says so and tries again next time', async (t) => {
  const w = world(t);
  commitFiles(w.dir, { 'docs/work/2026-11-22-e-a-w1-plan.html': Buffer.from([0x3c, 0x70, 0x3e, 0xff, 0xfe, 0x0a]) }, 'a page that is not UTF-8');
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  assert.deepEqual(await openItem(plan, facts, w.parts), { done: false, why: 'the wu-plan page for E-A.W1 could not be copied as text' });
  assert.equal(existsSync(path.join(w.appDir, 'open', 'p0a1b2c3d', tip, '.complete')), false);
  assert.deepEqual(readdirSync(path.join(w.appDir, 'open', 'p0a1b2c3d')), [], 'nothing kept');
  assert.equal(w.opened.length, 0);
});

test('W6 review round 1 (open.ts:100): paths that differ only in case (one file, or a file and a folder) do not throw the copy away on a case-insensitive disk: the colliding path is skipped', async (t) => {
  const w = world(t);
  const scratch = mk(path.join(w.root, 'blobs'));
  const blob = (name: string, text: string) => {
    writeFileSync(path.join(scratch, name), text);
    return sh(GIT, w.dir, ['hash-object', '-w', path.join(scratch, name)]).trim();
  };
  for (const [p, text] of [['docs/A.md', 'upper\n'], ['docs/a.md', 'lower\n'], ['docs/X', 'a file\n'], ['docs/x/inside.md', 'in a folder\n']] as const) {
    sh(GIT, w.dir, ['update-index', '--add', '--cacheinfo', `100644,${blob(p.replaceAll('/', '_'), text)},${p}`]);
  }
  sh(GIT, w.dir, ['commit', '-q', '-m', 'case twins']);
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  assert.deepEqual(await openItem(plan, facts, w.parts), { done: true });
  assert.equal(readFileSync(fileURLToPath(w.opened[0] as string), 'utf8'), PLAN_PAGE);
});

test('W6 review round 1 (open.ts:70, open.ts:78): an Open removes a temp folder a crash left, and the copies of projects no longer listed', async (t) => {
  const w = world(t);
  const open = path.join(w.appDir, 'open');
  const stale = mk(path.join(open, 'p0a1b2c3d', '.tmp-123-abcd1234'));
  writeFileSync(path.join(stale, 'half.html'), 'half\n');
  const gone = mk(path.join(open, 'p99999999', 'a'.repeat(40)));
  writeFileSync(path.join(gone, 'x.html'), 'x\n');
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  assert.deepEqual(await openItem(plan, facts, { ...w.parts, listed: ['p0a1b2c3d'] }), { done: true });
  assert.equal(existsSync(stale), false, 'the crash\'s temp folder is gone');
  assert.deepEqual(readdirSync(open), ['p0a1b2c3d'], 'a project no longer listed keeps no copies');
  assert.deepEqual(readdirSync(path.join(open, 'p0a1b2c3d')), [tip]);
});

test('W6 review round 2 (open.ts:117): reusing a copy never serves a case twin: the page and its data file are checked against their blobs, and a copy that differs is rebuilt', async (t) => {
  const w = world(t);
  const scratch = mk(path.join(w.root, 'blobs2'));
  const add = (p: string, text: string) => {
    const f = path.join(scratch, p.replaceAll('/', '_'));
    writeFileSync(f, text);
    sh(GIT, w.dir, ['update-index', '--add', '--cacheinfo', `100644,${sh(GIT, w.dir, ['hash-object', '-w', f]).trim()},${p}`]);
  };
  // The result's page and a twin differing only in its folder's case; the twin sorts first.
  add('docs/Work/2026-11-22-e-a-w1-result.html', '<p>A TWIN, not the committed page.</p>\n');
  add('docs/work/2026-11-22-e-a-w1-result.yaml', RESULT_YAML);
  add('docs/work/2026-11-22-e-a-w1-result.html', '<p>The committed result.</p>\n');
  sh(GIT, w.dir, ['commit', '-q', '-m', 'a result and a case twin']);
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  assert.deepEqual(await openItem(plan, facts, w.parts), { done: true }, 'the plan\'s copy is made first');
  sh(GIT, w.dir, ['notes', '--ref=cr', 'add', '-m', '{"schema":"cr/v1","verdict":"PASS","blockers":0,"importants":0}', 'wu/E-A/w1-reader']);
  const facts2 = await w.facts();
  const result = itemsOf({ git: facts2, router: heard({}), sent: new Set() }).find((i) => i.kind === 'result') as Item;
  assert.deepEqual(await openItem(result, facts2, w.parts), { done: true });
  assert.equal(readFileSync(fileURLToPath(w.opened[1] as string), 'utf8'), '<p>The committed result.</p>\n');
});

test('W6 review round 2 (open.ts:151): a kept copy whose page is not the committed bytes (a write cut short) is never served: it is rebuilt through a temp folder and renamed over, never written into in place', async (t) => {
  const w = world(t);
  commitFiles(w.dir, { 'docs/work/2026-11-22-e-a-w1-result.yaml': RESULT_YAML, 'docs/work/2026-11-22-e-a-w1-result.html': '<p>The result, whole.</p>\n' }, 'the result');
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  assert.deepEqual(await openItem(plan, facts, w.parts), { done: true });
  const copy = path.join(w.appDir, 'open', 'p0a1b2c3d', tip);
  writeFileSync(path.join(copy, 'docs', 'work', '2026-11-22-e-a-w1-result.html'), '<p>The res');
  writeFileSync(path.join(copy, 'sentinel'), 'left in the kept copy\n');
  sh(GIT, w.dir, ['notes', '--ref=cr', 'add', '-m', '{"schema":"cr/v1","verdict":"PASS","blockers":0,"importants":0}', 'wu/E-A/w1-reader']);
  const facts2 = await w.facts();
  const result = itemsOf({ git: facts2, router: heard({}), sent: new Set() }).find((i) => i.kind === 'result') as Item;
  assert.deepEqual(await openItem(result, facts2, w.parts), { done: true });
  assert.equal(readFileSync(fileURLToPath(w.opened[1] as string), 'utf8'), '<p>The result, whole.</p>\n');
  assert.equal(existsSync(path.join(copy, 'sentinel')), false, 'the copy was rebuilt and renamed over, not written into');
  assert.deepEqual(readdirSync(path.join(w.appDir, 'open', 'p0a1b2c3d')), [tip], 'no temp folder left');
});

test('W6 review round 2 (index.ts:233): Open takes the project with the item and the facts from one view; a view of a project no longer listed opens nothing and says so', async (t) => {
  const w = world(t);
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  const view = { ...emptyView(), project: { id: 'p0a1b2c3d', name: 'proj' }, git: facts, items: [plan] };
  const { project: _p, ...rest } = w.parts;
  void _p;
  const switched = [{ id: 'p11111111', path: path.join(w.root, 'other'), name: 'other' }];
  assert.deepEqual(await openListed(plan, view, switched, rest), { done: false, why: 'the list is of a project no longer listed: it shows the listed one at its next read' });
  assert.deepEqual(await openListed(plan, { ...view, project: null }, switched, rest), { done: false, why: 'no project is listed' });
  assert.equal(w.opened.length, 0);
  assert.equal(existsSync(path.join(w.appDir, 'open', 'p11111111')), false, 'nothing filed under the other project');
  const listed = [{ id: 'p0a1b2c3d', path: w.dir, name: 'proj' }, ...switched];
  assert.deepEqual(await openListed(plan, view, listed, rest), { done: true });
  assert.ok((w.opened[0] as string).includes('/open/p0a1b2c3d/'));
});

test('W6 review round 3 (open.ts:124, nit :143): reusing a kept copy costs one git call (the tree read), its files checked against the tree\'s blob ids in-process; a fresh copy reads each file from git once', async (t) => {
  const w = world(t);
  const log = path.join(w.root, 'git-calls.log');
  const wrapper = program(path.join(mk(path.join(w.root, 'bin')), 'git'), `#!/bin/sh\nprintf '%s\\n' "$*" >> '${log}'\nexec '${GIT}' "$@"\n`);
  const parts = { ...w.parts, git: wrapper };
  commitFiles(w.dir, { 'docs/work/2026-11-22-e-a-w1-result.yaml': RESULT_YAML, 'docs/work/2026-11-22-e-a-w1-result.html': '<p>The result.</p>\n', 'docs/work/notes.yaml': 'artifact: none\n' }, 'more docs');
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const { plan, facts } = await planItem(w, tip);
  assert.deepEqual(await openItem(plan, facts, parts), { done: true });
  const fresh = readFileSync(log, 'utf8').trim().split('\n');
  const blobs = fresh.filter((l) => / cat-file blob /.test(l));
  assert.equal(new Set(blobs).size, blobs.length, 'no file read twice in a fresh copy');
  writeFileSync(log, '');
  assert.deepEqual(await openItem(plan, facts, parts), { done: true });
  const reuse = readFileSync(log, 'utf8').trim().split('\n').filter((l) => l !== '');
  assert.equal(reuse.length, 1, reuse.join('\n'));
  assert.match(reuse[0] as string, / ls-tree /);
  assert.equal(readFileSync(fileURLToPath(w.opened[1] as string), 'utf8'), PLAN_PAGE);
});

test('epic review round 1 (open.ts:131): with a git older than 2.45, Open reads a project that is not a partial clone without --no-lazy-fetch, after one config read; a partial clone is refused and says why', async (t) => {
  const w = world(t);
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const facts = await w.facts();
  const plan = itemsOf({ git: facts, router: heard({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: tip.slice(0, 7) } }), sent: new Set() }).find((i) => i.kind === 'plan') as Item;
  const rec = path.join(w.root, 'old.jsonl');
  const old = oldGit(path.join(mk(path.join(w.root, 'old')), 'git'), rec, GIT);
  assert.deepEqual(await openItem(plan, facts, { ...w.parts, git: old }), { done: true });
  assert.equal(readFileSync(fileURLToPath(w.opened[0] as string), 'utf8'), PLAN_PAGE);
  const runs = records(rec).map((r) => r.argv);
  assert.equal(runs.filter((a) => a.includes('--get-regexp')).length, 1, 'one config read');
  assert.ok(runs.filter((a) => !a.includes('--get-regexp')).slice(1).every((a) => !a.includes('--no-lazy-fetch')), 'after the first refusal, no line carries --no-lazy-fetch');
  // A partial clone: an old git may not read it (it could fetch), so Open refuses and says why.
  sh(GIT, w.dir, ['config', 'remote.origin.promisor', 'true']);
  const fresh = await openItem(plan, facts, { ...w.parts, git: old, appDir: path.join(w.root, 'Keel2') });
  assert.equal(fresh.done, false);
  assert.match((fresh as { why: string }).why, /partial clone needs git 2\.45/);
});

test('W12 review round 1 nit (open.ts:132): Open reads through the reader\'s runner for the project, so an old git it already knows costs Open no refused run and no config read', async (t) => {
  const w = world(t);
  const tip = rev(w.dir, 'wu/E-A/w1-reader');
  const rec = path.join(w.root, 'old.jsonl');
  const old = oldGit(path.join(mk(path.join(w.root, 'old')), 'git'), rec, GIT);
  const reader = new GitReader(w.dir, w.parts.runOptions);
  const facts = await reader.read(old);
  const plan = itemsOf({ git: facts, router: heard({ 'wu/E-A.W1/stage': { stage: 'plan to read', tip: tip.slice(0, 7) } }), sent: new Set() }).find((i) => i.kind === 'plan') as Item;
  const n = records(rec).length;
  const view = { ...emptyView(), project: { id: 'p0a1b2c3d', name: 'proj' }, git: facts, items: [plan] };
  const { project: _p, ...rest } = w.parts;
  const r = await openListed(plan, view, [{ id: 'p0a1b2c3d', name: 'proj', path: w.dir }], { ...rest, git: old, runnerFor: (p) => (p === w.dir ? reader.runner : null) });
  assert.deepEqual(r, { done: true });
  const runs = records(rec).slice(n).map((x) => x.argv);
  assert.ok(runs.length > 0);
  assert.ok(runs.every((a) => !a.includes('--no-lazy-fetch') && !a.includes('--get-regexp')), JSON.stringify(runs));
});
