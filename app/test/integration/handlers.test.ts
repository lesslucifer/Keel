// The nine requests' handlers at their real boundary: through the dispatcher the shell hands to
// ipcMain.handle, with the shell's parts faked (the two dialogs, the test alert, the push), real git
// in temp projects and fake tools in temp folders.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdirSync, readdirSync, existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { dispatcher, type EventLike } from '../../src/main/bridge.ts';
import { makeHandlers, prepare, readerParts, type Parts } from '../../src/main/handlers.ts';
import { loadSettings } from '../../src/main/settings.ts';
import type { AlertOutcome, Item, LoginPathState, Tool, View } from '../../src/shared/contract.ts';
import { emptyView } from '../../src/main/reader/view.ts';
import { tempDir, program, realGit, sh, snapshot, mk, recorder, bounded, within, HANG } from '../support/fs.ts';

const GIT = realGit();
const PAGE = 'app://keel/index.html';
const own: EventLike = { sender: { id: 1 }, senderFrame: { url: PAGE, parent: null } };

function keelRepo(dir: string): string {
  mkdirSync(path.join(dir, '.keel'), { recursive: true });
  writeFileSync(path.join(dir, '.keel', 'config.toml'), 'code_paths = ["src/"]\n');
  sh(GIT, dir, ['init', '-q']);
  sh(GIT, dir, ['add', '.']);
  sh(GIT, dir, ['commit', '-q', '-m', 'init']);
  return dir;
}

/** A world: a temp app folder, a bin folder with fake keel and claude, the real git's folder on the PATH, and recorded shell parts. */
function world(t: { after: (fn: () => void) => void }) {
  const dir = tempDir(t, 'handlers');
  const bin = mk(path.join(dir, 'bin'));
  const keel = program(path.join(bin, 'keel'), '#!/bin/sh\n');
  const claude = program(path.join(bin, 'claude'), '#!/bin/sh\n');
  const loginPath: LoginPathState = { entries: [bin, path.dirname(GIT)], source: 'login-shell', reason: null };
  const seen = { folders: 0, files: [] as Tool[], pushes: 0, alerts: 0, opened: [] as string[], marked: [] as Array<[string, boolean]>, rereads: 0 };
  const answers = { folder: null as string | null, file: null as string | null, alert: 'shown' as AlertOutcome, view: emptyView() as View };
  const parts: Parts = {
    appDir: path.join(dir, 'app-folder'),
    version: '0.1.0',
    packaged: false,
    loginPath,
    env: { HOME: mk(path.join(dir, 'home')), LANG: 'C' },
    chooseFolder: async () => { seen.folders++; return answers.folder; },
    chooseFile: async (tool) => { seen.files.push(tool); return answers.file; },
    testAlert: async () => { seen.alerts++; return answers.alert; },
    push: () => { seen.pushes++; },
    view: () => answers.view,
    open: async (item) => { seen.opened.push(item.id); return { done: true }; },
    mark: async (item, sent) => { seen.marked.push([item.id, sent]); return { done: true }; },
    reread: async () => { seen.rereads++; },
  };
  const ask = dispatcher(() => ({ contentsId: 1, pages: [PAGE] }), makeHandlers(parts));
  const call = (name: string, args: Record<string, string> = {}) => ask(own, { name, args });
  return { dir, bin, keel, claude, loginPath, seen, answers, parts, call };
}

test('state.get answers the version, whether the app is packaged, the PATH and its source, each tool\'s state and the project list', async (t) => {
  const w = world(t);
  assert.deepEqual(await w.call('state.get'), {
    ok: true,
    value: {
      version: '0.1.0',
      packaged: false,
      path: w.loginPath,
      tools: {
        keel: { path: w.keel, source: 'path', problem: null },
        git: { path: GIT, source: 'path', problem: null },
        claude: { path: w.claude, source: 'path', problem: null },
      },
      projects: [],
    },
  });
});

test('W9: state.get says whether the app is packaged as the shell gives it, so the window names the place macOS files its alerts (Keel or Electron)', async (t) => {
  const w = world(t);
  w.parts.packaged = true;
  const packaged = (await w.call('state.get')) as { ok: true; value: Record<string, unknown> };
  assert.equal(packaged.value.packaged, true);
  assert.deepEqual(Object.keys(packaged.value).sort(), ['packaged', 'path', 'projects', 'tools', 'version']);
  w.parts.packaged = false;
  assert.equal(((await w.call('state.get')) as { ok: true; value: Record<string, unknown> }).value.packaged, false);
});

test('projects.add opens the folder dialog, lists a Keel main checkout, pushes once; state.get shows it; remove takes it off and leaves its files', async (t) => {
  const w = world(t);
  const project = keelRepo(path.join(w.dir, 'proj'));
  const before = snapshot(project);
  w.answers.folder = project;
  assert.deepEqual(await w.call('projects.add'), { ok: true, value: { done: true } });
  assert.deepEqual([w.seen.folders, w.seen.pushes], [1, 1]);
  const state = (await w.call('state.get')) as { ok: true; value: { projects: Array<{ id: string; path: string; name: string }> } };
  assert.equal(state.value.projects.length, 1);
  const { id } = state.value.projects[0] as { id: string };
  assert.deepEqual(state.value.projects[0], { id, path: project, name: 'proj' });
  assert.deepEqual(await w.call('projects.remove', { project: id }), { ok: true, value: { done: true } });
  assert.equal(w.seen.pushes, 2);
  assert.deepEqual(await w.call('projects.remove', { project: id }), { ok: true, value: { done: false, why: 'no such project' } });
  assert.equal(w.seen.pushes, 2);
  assert.deepEqual(snapshot(project), before);
});

test('projects.add is refused before any dialog while a project is listed, or while git is not found', async (t) => {
  const w = world(t);
  w.answers.folder = keelRepo(path.join(w.dir, 'proj'));
  await w.call('projects.add');
  const r = (await w.call('projects.add')) as { ok: true; value: { done: false; why: string } };
  assert.equal(r.value.done, false);
  assert.match(r.value.why, /one project/);
  assert.deepEqual([w.seen.folders, w.seen.pushes], [1, 1]);
  const noGit = world(t);
  noGit.loginPath.entries.splice(1);
  const r2 = (await noGit.call('projects.add')) as { ok: true; value: { done: false; why: string } };
  assert.match(r2.value.why, /git/);
  assert.equal(noGit.seen.folders, 0);
});

test('a cancelled dialog changes nothing and pushes nothing; a refused folder says why', async (t) => {
  const w = world(t);
  assert.deepEqual(await w.call('projects.add'), { ok: true, value: { done: false, why: 'cancelled' } });
  assert.deepEqual(await w.call('settings.choose', { tool: 'git' }), { ok: true, value: { done: false, why: 'cancelled' } });
  w.answers.folder = mk(path.join(w.dir, 'plain'));
  assert.deepEqual(await w.call('projects.add'), { ok: true, value: { done: false, why: 'not a git checkout' } });
  assert.equal(w.seen.pushes, 0);
  assert.ok(!readdirSync(w.dir).includes('app-folder'), 'the app folder was written');
});

test('settings.choose opens the file dialog for that tool, saves a good path and pushes once; a bad one is refused; settings.reset drops it and pushes', async (t) => {
  const w = world(t);
  const other = program(path.join(mk(path.join(w.dir, 'other')), 'keel'), '#!/bin/sh\n');
  w.answers.file = other;
  assert.deepEqual(await w.call('settings.choose', { tool: 'keel' }), { ok: true, value: { done: true } });
  assert.deepEqual([w.seen.files, w.seen.pushes], [['keel'], 1]);
  const state = (await w.call('state.get')) as { ok: true; value: { tools: { keel: unknown } } };
  assert.deepEqual(state.value.tools.keel, { path: other, source: 'set', problem: null });
  w.answers.file = w.claude;
  assert.deepEqual(await w.call('settings.choose', { tool: 'git' }), { ok: true, value: { done: false, why: 'the program must be named git' } });
  assert.equal(w.seen.pushes, 1);
  assert.deepEqual(await w.call('settings.reset', { tool: 'keel' }), { ok: true, value: { done: true } });
  assert.equal(w.seen.pushes, 2);
  assert.deepEqual((await loadSettings(w.parts.appDir)).value, { version: 1, tools: {} });
});

test('alert.test answers what the shell\'s alert answered', async (t) => {
  const w = world(t);
  for (const outcome of ['shown', 'not-shown', 'not-supported'] as const) {
    w.answers.alert = outcome;
    assert.deepEqual(await w.call('alert.test'), { ok: true, value: outcome });
  }
  assert.equal(w.seen.alerts, 3);
});

test('waiting.get answers the reader\'s view as it stands, every worker string through plain(), and changes nothing', async (t) => {
  const w = world(t);
  assert.deepEqual(await w.call('waiting.get'), { ok: true, value: emptyView() });
  const view = emptyView();
  view.router.run = 'keel-1c.20261122-090000.42';
  view.router.values = { 'question/q1': { headline: 'Merge\u202e now?\n', lines: ['a\u0007b'] } };
  w.answers.view = view;
  const a = (await w.call('waiting.get')) as { ok: true; value: View };
  assert.equal(a.value.router.run, 'keel-1c.20261122-090000.42');
  assert.deepEqual(a.value.router.values, { 'question/q1': { headline: 'Merge  now? ', lines: ['a b'] } });
  assert.equal(w.seen.pushes, 0);
  assert.equal(existsSync(w.parts.appDir), false);
});

/** An item as the view lists it, for the handlers' lookups. */
function item(over: Partial<Item> & Pick<Item, 'id' | 'kind' | 'open' | 'markable'>): Item {
  return { group: 'yours', wu: null, epic: null, title: 'An item', branch: null, tip: null, since: null, why: 'Why.', worker: null, line: null, text: [], ...over };
}

test('item.open opens only an item the view lists now, and only a page or an epic result; a gone, moved, text or empty item opens nothing', async (t) => {
  const w = world(t);
  const view = emptyView();
  view.items = [
    item({ id: 'result/E-A.W1@0f1e2d3', kind: 'result', open: 'page', markable: false }),
    item({ id: 'epic-result/E-A@a1b2c3d', kind: 'epic-result', open: 'archive', markable: false }),
    item({ id: 'question/q1@a1b2c3d4', kind: 'question', open: 'text', markable: true }),
    item({ id: 'push/main@9a8b7c6', kind: 'push', open: 'none', markable: false }),
  ];
  w.answers.view = view;
  assert.deepEqual(await w.call('item.open', { item: 'result/E-A.W1@0f1e2d3' }), { ok: true, value: { done: true } });
  assert.deepEqual(await w.call('item.open', { item: 'epic-result/E-A@a1b2c3d' }), { ok: true, value: { done: true } });
  assert.deepEqual(w.seen.opened, ['result/E-A.W1@0f1e2d3', 'epic-result/E-A@a1b2c3d']);
  const left = (await w.call('item.open', { item: 'result/E-A.W1@1111111' })) as { ok: true; value: { done: false; why: string } };
  assert.equal(left.value.done, false);
  assert.match(left.value.why, /left the list or moved on/);
  assert.deepEqual(await w.call('item.open', { item: 'question/q1@a1b2c3d4' }), { ok: true, value: { done: false, why: 'a question shows its text in the list itself' } });
  assert.deepEqual(await w.call('item.open', { item: 'push/main@9a8b7c6' }), { ok: true, value: { done: false, why: 'this item has nothing to open' } });
  assert.deepEqual(await w.call('item.open', { item: '../../etc/passwd' }), { ok: false, refused: 'bad-slot:item' });
  assert.deepEqual(w.seen.opened, ['result/E-A.W1@0f1e2d3', 'epic-result/E-A@a1b2c3d']);
});

test('item.mark marks only a markable item the view lists now, yes or no', async (t) => {
  const w = world(t);
  const view = emptyView();
  view.items = [item({ id: 'plan/E-A.W2@0f1e2d3', kind: 'plan', open: 'page', markable: true }), item({ id: 'result/E-A.W1@0f1e2d3', kind: 'result', open: 'page', markable: false })];
  w.answers.view = view;
  assert.deepEqual(await w.call('item.mark', { item: 'plan/E-A.W2@0f1e2d3', sent: 'yes' }), { ok: true, value: { done: true } });
  assert.deepEqual(await w.call('item.mark', { item: 'plan/E-A.W2@0f1e2d3', sent: 'no' }), { ok: true, value: { done: true } });
  assert.deepEqual(await w.call('item.mark', { item: 'result/E-A.W1@0f1e2d3', sent: 'yes' }), { ok: true, value: { done: false, why: 'only a plan to read or a question can be marked sent' } });
  const left = (await w.call('item.mark', { item: 'plan/E-A.W2@1111111', sent: 'yes' })) as { ok: true; value: { done: boolean } };
  assert.equal(left.value.done, false);
  assert.deepEqual(w.seen.marked, [['plan/E-A.W2@0f1e2d3', true], ['plan/E-A.W2@0f1e2d3', false]]);
  assert.equal(w.seen.rereads, 2, 'a read again after each mark made, none after a refused one');
});

test('epic review round 1 (index.ts:266): a mark holds the queue only for its write; the read again after it waits outside the queue, and the mark answers once that read is done', async (t) => {
  const w = world(t);
  const view = emptyView();
  view.items = [item({ id: 'plan/E-A.W2@0f1e2d3', kind: 'plan', open: 'page', markable: true })];
  w.answers.view = view;
  // An explicit hold: the read after the mark ends only when the test releases it (a watcher read waiting on git).
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => (release = resolve));
  let reread = false;
  w.parts.reread = async () => {
    reread = true;
    await held;
  };
  let answered = false;
  const marking = w.call('item.mark', { item: 'plan/E-A.W2@0f1e2d3', sent: 'yes' }).then((a) => {
    answered = true;
    return a;
  });
  // The queue answers while the read is held: state.get, settings.reset and projects.remove each go through it.
  assert.equal(((await w.call('state.get')) as { ok: boolean }).ok, true);
  assert.deepEqual(await w.call('settings.reset', { tool: 'keel' }), { ok: true, value: { done: true } });
  assert.deepEqual(await w.call('projects.remove', { project: 'p00000000' }), { ok: true, value: { done: false, why: 'no such project' } });
  assert.deepEqual(w.seen.marked, [['plan/E-A.W2@0f1e2d3', true]], 'the mark was written in the queue');
  assert.equal(reread, true, 'the read again started');
  assert.equal(answered, false, 'the mark answers only after that read');
  release();
  assert.deepEqual(await marking, { ok: true, value: { done: true } });
});

test('the reader\'s parts give the listed project with its main checkout, and the usable git; none while unlisted or unusable', async (t) => {
  const w = world(t);
  const rp = readerParts({ appDir: w.parts.appDir, loginPath: w.loginPath, env: w.parts.env as Record<string, string> });
  assert.equal(await rp.project(), null);
  assert.equal(await rp.git(), GIT);
  assert.deepEqual(rp.runOptions, { path: w.loginPath.entries, source: w.parts.env });
  const project = keelRepo(path.join(w.dir, 'proj'));
  w.answers.folder = project;
  await w.call('projects.add');
  const listed = await rp.project();
  assert.deepEqual(listed && { name: listed.name, path: listed.path }, { name: 'proj', path: project });
  assert.match(listed?.id ?? '', /^p[0-9a-f]{8}$/);
  const unusable = readerParts({ appDir: w.parts.appDir, loginPath: { entries: [w.bin], source: 'login-shell', reason: null } });
  assert.equal(await unusable.git(), null, 'no git on that PATH');
});

test('changes run one at a time: two projects.add at once list one project', async (t) => {
  const w = world(t);
  const a = keelRepo(path.join(w.dir, 'a'));
  const b = keelRepo(path.join(w.dir, 'b'));
  let n = 0;
  w.parts.chooseFolder = async () => (n++ === 0 ? a : b);
  const [ra, rb] = await Promise.all([w.call('projects.add'), w.call('projects.add')]);
  assert.deepEqual(ra, { ok: true, value: { done: true } });
  assert.equal((rb as { ok: true; value: { done: boolean } }).value.done, false);
  const state = (await w.call('state.get')) as { ok: true; value: { projects: unknown[] } };
  assert.equal(state.value.projects.length, 1);
  assert.equal(w.seen.pushes, 1);
});

test('projects.add refuses a git inside the folder being added, set or found on the PATH, before it runs there', async (t) => {
  const w = world(t);
  const project = keelRepo(path.join(w.dir, 'proj'));
  const record = path.join(w.dir, 'record.json');
  const shim = recorder(path.join(mk(path.join(project, 'bin')), 'git'), record, 0, `${project}\n`);
  w.answers.folder = project;
  // Set while nothing is listed: the tool check passes, for no project holds it yet.
  w.answers.file = shim;
  assert.deepEqual(await w.call('settings.choose', { tool: 'git' }), { ok: true, value: { done: true } });
  const set = (await w.call('projects.add')) as { ok: true; value: { done: false; why: string } };
  assert.equal(set.value.done, false);
  assert.match(set.value.why, /^git is inside the project proj/);
  assert.ok(!existsSync(record), 'the set git was run on the folder');
  // Unset, and found first on a PATH entry inside the folder.
  await w.call('settings.reset', { tool: 'git' });
  w.loginPath.entries.unshift(path.dirname(shim));
  const found = (await w.call('projects.add')) as { ok: true; value: { done: false; why: string } };
  assert.match(found.value.why, /^git is inside the project proj/);
  assert.ok(!existsSync(record), 'the git found on the PATH was run on the folder');
  assert.deepEqual((await w.call('state.get') as { ok: true; value: { projects: unknown[] } }).value.projects, []);
});

test('state.get answers while a folder or file dialog stays open; the change goes on when the dialog answers', async (t) => {
  const w = world(t);
  const project = keelRepo(path.join(w.dir, 'proj'));
  const keel = program(path.join(mk(path.join(w.dir, 'other')), 'keel'), '#!/bin/sh\n');
  let answerFolder: (v: string | null) => void = () => undefined;
  let answerFile: (v: string | null) => void = () => undefined;
  let open = 0;
  w.parts.chooseFolder = () => { open++; return new Promise((r) => { answerFolder = r; }); };
  w.parts.chooseFile = () => { open++; return new Promise((r) => { answerFile = r; }); };
  t.after(() => { answerFolder(null); answerFile(null); });
  const adding = w.call('projects.add');
  assert.ok(await within(5_000, () => open === 1), 'the folder dialog never opened');
  const during = (await bounded(3_000, 'state.get while the folder dialog is open', w.call('state.get'))) as { ok: true; value: { projects: unknown[] } };
  assert.deepEqual(during.value.projects, []);
  answerFolder(project);
  assert.deepEqual(await adding, { ok: true, value: { done: true } });
  const choosing = w.call('settings.choose', { tool: 'keel' });
  assert.ok(await within(5_000, () => open === 2), 'the file dialog never opened');
  const again = (await bounded(3_000, 'state.get while the file dialog is open', w.call('state.get'))) as { ok: true; value: { projects: unknown[] } };
  assert.equal(again.value.projects.length, 1);
  answerFile(keel);
  assert.deepEqual(await choosing, { ok: true, value: { done: true } });
  assert.equal(w.seen.pushes, 2);
});

test('state.get, remove and reset answer while projects.add waits on a git that hangs', async (t) => {
  const w = world(t);
  const project = keelRepo(path.join(w.dir, 'proj'));
  const pidFile = path.join(w.dir, 'git-pid');
  program(path.join(w.bin, 'git'), `#!/bin/sh\necho $$ > ${pidFile}\n${HANG}\n`);
  const end = () => {
    try {
      process.kill(-Number(readFileSync(pidFile, 'utf8')), 'SIGKILL');
    } catch {
      // gone
    }
  };
  t.after(end);
  w.answers.folder = project;
  const adding = w.call('projects.add');
  assert.ok(await within(5_000, () => existsSync(pidFile) && readFileSync(pidFile, 'utf8').trim() !== ''), 'git never ran');
  const state = (await bounded(3_000, 'state.get while git hangs', w.call('state.get'))) as { ok: true; value: { projects: unknown[] } };
  assert.deepEqual(state.value.projects, []);
  assert.deepEqual(await bounded(3_000, 'settings.reset while git hangs', w.call('settings.reset', { tool: 'keel' })), { ok: true, value: { done: true } });
  assert.deepEqual(await bounded(3_000, 'projects.remove while git hangs', w.call('projects.remove', { project: 'p00000000' })), { ok: true, value: { done: false, why: 'no such project' } });
  end();
  const r = (await adding) as { ok: true; value: { done: boolean } };
  assert.equal(r.value.done, false);
});

test('prepare() loads the settings, then reads the PATH with the set tools\' folders for its fallback', async (t) => {
  const w = world(t);
  const other = program(path.join(mk(path.join(w.dir, 'tools')), 'keel'), '#!/bin/sh\n');
  w.answers.file = other;
  await w.call('settings.choose', { tool: 'keel' });
  let given: readonly string[] = [];
  const loginPath = await prepare(w.parts.appDir, async (toolDirs) => {
    given = toolDirs;
    return { entries: ['/usr/bin'], source: 'fallback', reason: 'test' };
  });
  assert.deepEqual(given, [path.dirname(other)]);
  assert.deepEqual(loginPath, { entries: ['/usr/bin'], source: 'fallback', reason: 'test' });
});

test('W6 review round 1 (handlers.ts:167): item.open runs one at a time in the main part, so two Opens of one tip (a reloaded window) never copy at once', async (t) => {
  const w = world(t);
  const view = emptyView();
  view.items = [item({ id: 'result/E-A.W1@0f1e2d3', kind: 'result', open: 'page', markable: false })];
  w.answers.view = view;
  const events: string[] = [];
  let release: () => void = () => undefined;
  let calls = 0;
  w.parts.open = async (it) => {
    calls += 1;
    const n = calls;
    events.push(`start ${n}`);
    if (n === 1) await new Promise<void>((resolve) => (release = resolve));
    events.push(`end ${n} ${it.id}`);
    return { done: true };
  };
  const first = w.call('item.open', { item: 'result/E-A.W1@0f1e2d3' });
  const second = w.call('item.open', { item: 'result/E-A.W1@0f1e2d3' });
  await new Promise((r) => setImmediate(r));
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(events, ['start 1'], 'the second waits for the first');
  release();
  assert.deepEqual(await Promise.all([first, second]), [{ ok: true, value: { done: true } }, { ok: true, value: { done: true } }]);
  assert.deepEqual(events, ['start 1', 'end 1 result/E-A.W1@0f1e2d3', 'start 2', 'end 2 result/E-A.W1@0f1e2d3']);
});

test('W6 review round 1 (open.ts:78): removing a project removes its copies of committed docs from the app folder', async (t) => {
  const w = world(t);
  const project = keelRepo(path.join(w.dir, 'proj'));
  w.answers.folder = project;
  await w.call('projects.add');
  const state = (await w.call('state.get')) as { ok: true; value: { projects: Array<{ id: string }> } };
  const id = state.value.projects[0]?.id as string;
  const copies = path.join(w.parts.appDir, 'open', id, 'a'.repeat(40));
  mkdirSync(copies, { recursive: true });
  writeFileSync(path.join(copies, 'x.html'), 'x\n');
  mkdirSync(path.join(w.parts.appDir, 'open', 'p99999999'), { recursive: true });
  assert.deepEqual(await w.call('projects.remove', { project: id }), { ok: true, value: { done: true } });
  assert.equal(existsSync(path.join(w.parts.appDir, 'open', id)), false);
  assert.equal(existsSync(path.join(w.parts.appDir, 'open', 'p99999999')), true, 'only that project\'s copies');
});

test('W6 review rounds 2 and 3 (handlers.ts:143, :144): removing a project removes its copies only after an Open in flight ends, and never holds the settings queue while it waits', async (t) => {
  const w = world(t);
  const project = keelRepo(path.join(w.dir, 'proj'));
  w.answers.folder = project;
  await w.call('projects.add');
  const state = (await w.call('state.get')) as { ok: true; value: { projects: Array<{ id: string }> } };
  const id = state.value.projects[0]?.id as string;
  const copies = path.join(w.parts.appDir, 'open', id);
  mkdirSync(copies, { recursive: true });
  const view = emptyView();
  view.items = [item({ id: 'result/E-A.W1@0f1e2d3', kind: 'result', open: 'page', markable: false })];
  w.answers.view = view;
  // An explicit hold: the Open ends only when the test releases it.
  let release: () => void = () => undefined;
  const held = new Promise<void>((resolve) => (release = resolve));
  w.parts.open = async () => {
    await held;
    return { done: true };
  };
  const opening = w.call('item.open', { item: 'result/E-A.W1@0f1e2d3' });
  const removing = w.call('projects.remove', { project: id });
  // The settings queue answers while the Open is held: the removal's wait is outside it.
  const bound = new Promise<'held'>((resolve) => setTimeout(() => resolve('held'), 3000));
  const answered = await Promise.race([w.call('state.get').then((a) => (a as { ok: boolean }).ok), bound]);
  assert.equal(answered, true, 'state.get answered with the Open still held');
  const listed = (await w.call('state.get')) as { ok: true; value: { projects: unknown[] } };
  assert.deepEqual(listed.value.projects, [], 'the project left the list');
  assert.equal(existsSync(copies), true, 'its copies wait for the Open in flight');
  release();
  await opening;
  assert.deepEqual(await removing, { ok: true, value: { done: true } });
  assert.equal(existsSync(copies), false);
});

test('W6 review round 3 (alerts.ts:152): waiting.get tells the shell which focus it handed the page, so a reload or a new window clears only a focus a page has read', async (t) => {
  const w = world(t);
  const seen: number[] = [];
  w.parts.seen = (seq) => seen.push(seq);
  await w.call('waiting.get');
  const view = emptyView();
  view.focus = { wu: 'E-A.W1', seq: 4 };
  w.answers.view = view;
  await w.call('waiting.get');
  assert.deepEqual(seen, [4], 'only a focus the answer carried');
});
