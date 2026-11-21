// The six requests' handlers at their real boundary: through the dispatcher the shell hands to
// ipcMain.handle, with the shell's parts faked (the two dialogs, the test alert, the push), real git
// in temp projects and fake tools in temp folders.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdirSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { dispatcher, type EventLike } from '../../src/main/bridge.ts';
import { makeHandlers, prepare, type Parts } from '../../src/main/handlers.ts';
import { loadSettings } from '../../src/main/settings.ts';
import type { AlertOutcome, LoginPathState, Tool } from '../../src/shared/contract.ts';
import { tempDir, program, realGit, sh, snapshot, mk } from '../support/fs.ts';

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
  const seen = { folders: 0, files: [] as Tool[], pushes: 0, alerts: 0 };
  const answers = { folder: null as string | null, file: null as string | null, alert: 'shown' as AlertOutcome };
  const parts: Parts = {
    appDir: path.join(dir, 'app-folder'),
    version: '0.1.0',
    loginPath,
    env: { HOME: mk(path.join(dir, 'home')), LANG: 'C' },
    chooseFolder: async () => { seen.folders++; return answers.folder; },
    chooseFile: async (tool) => { seen.files.push(tool); return answers.file; },
    testAlert: async () => { seen.alerts++; return answers.alert; },
    push: () => { seen.pushes++; },
  };
  const ask = dispatcher(() => ({ contentsId: 1, pages: [PAGE] }), makeHandlers(parts));
  const call = (name: string, args: Record<string, string> = {}) => ask(own, { name, args });
  return { dir, bin, keel, claude, loginPath, seen, answers, parts, call };
}

test('state.get answers the version, the PATH and its source, each tool\'s state and the project list', async (t) => {
  const w = world(t);
  assert.deepEqual(await w.call('state.get'), {
    ok: true,
    value: {
      version: '0.1.0',
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
