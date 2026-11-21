// Settings and the project list at their real boundary: files in a temp app folder, real git in temp
// projects, fake tools as scripts. The app writes only in its own folder: a project's every file
// and its git status are the same before and after add, remove and every settings write.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, symlinkSync, chmodSync } from 'node:fs';
import path from 'node:path';
import { readJson, writeAtomic, load } from '../../src/main/store.ts';
import { loadSettings, chooseTool, resetTool, SETTINGS_FILE } from '../../src/main/settings.ts';
import { loadProjects, addProject, removeProject, PROJECTS_FILE } from '../../src/main/projects.ts';
import { lookup, toolStates } from '../../src/main/tools.ts';
import { tempDir, program, realGit, sh, snapshot, mk } from '../support/fs.ts';

const GIT = realGit();
const PATH = [path.dirname(GIT), '/usr/bin', '/bin'];
// Real git under the app's environment reads HOME's config: point HOME at an empty temp folder per test.
const opts = (home: string) => ({ path: PATH, source: { HOME: home, LANG: 'C' } });

/** A main checkout whose HEAD commits .keel/config.toml. */
function keelRepo(dir: string): string {
  mkdirSync(path.join(dir, '.keel'), { recursive: true });
  writeFileSync(path.join(dir, '.keel', 'config.toml'), 'code_paths = ["src/"]\n');
  writeFileSync(path.join(dir, 'README.md'), 'x\n');
  sh(GIT, dir, ['init', '-q']);
  sh(GIT, dir, ['add', '.']);
  sh(GIT, dir, ['commit', '-q', '-m', 'init']);
  return dir;
}

function status(dir: string): string {
  return sh(GIT, dir, ['--no-optional-locks', 'status', '--porcelain=v1', '--untracked-files=all', '--ignored']);
}

/** A counter standing in for the shell's push('state.changed'). */
function pushes() {
  const c = { n: 0, push: () => { c.n++; } };
  return c;
}

test('the store writes atomically: the value round-trips, no temp file is left, the file and new folder are the owner\'s only', async (t) => {
  const app = path.join(tempDir(t, 'store'), 'app-folder');
  const file = path.join(app, 'x.json');
  assert.deepEqual(await readJson(file), { kind: 'missing' });
  await writeAtomic(file, { version: 1, a: [1, 'b'] });
  await writeAtomic(file, { version: 1, a: [2] });
  assert.deepEqual(await readJson(file), { kind: 'ok', value: { version: 1, a: [2] } });
  assert.deepEqual(readdirSync(app), ['x.json']);
  assert.equal(statSync(file).mode & 0o777, 0o600);
  assert.equal(statSync(app).mode & 0o777, 0o700);
});

test('a broken file - half written, too large, or a shape the app does not know - is set aside and the defaults load', async (t) => {
  const app = tempDir(t, 'store');
  const file = path.join(app, SETTINGS_FILE);
  writeFileSync(file, '{"version": 1, "tools": {"git": "/usr/bi');
  const now = new Date(Date.UTC(2026, 10, 20, 12, 0, 0));
  assert.deepEqual(await load(file, () => null, () => ({ d: 1 }), now), { value: { d: 1 }, setAside: `${file}.broken-20261120T120000Z` });
  assert.equal(readFileSync(`${file}.broken-20261120T120000Z`, 'utf8'), '{"version": 1, "tools": {"git": "/usr/bi');
  writeFileSync(file, '{"version": 9, "tools": {}}');
  const again = await loadSettings(app, now);
  assert.deepEqual(again, { value: { version: 1, tools: {} }, setAside: `${file}.broken-20261120T120000Z-1` });
  writeFileSync(file, ' '.repeat(1024 * 1024 + 1));
  assert.deepEqual(await readJson(file), { kind: 'broken', why: 'too large' });
  assert.equal((await loadSettings(app, now)).setAside, `${file}.broken-20261120T120000Z-2`);
  assert.equal((await loadSettings(app)).setAside, null);
});

test('settings round-trip across a fresh load: a chosen tool is kept, a reset one dropped; each change pushes state.changed once', async (t) => {
  const dir = tempDir(t, 'settings');
  const app = path.join(dir, 'app-folder');
  const keel = program(path.join(dir, 'keel'), '#!/bin/sh\n');
  const c = pushes();
  assert.deepEqual(await chooseTool(app, 'keel', keel, [], c.push), { ok: true, settings: { version: 1, tools: { keel } } });
  assert.equal(c.n, 1);
  assert.deepEqual((await loadSettings(app)).value, { version: 1, tools: { keel } });
  await chooseTool(app, 'git', GIT, [], c.push);
  assert.deepEqual(await resetTool(app, 'keel', c.push), { version: 1, tools: { git: GIT } });
  assert.equal(c.n, 3);
  assert.deepEqual((await loadSettings(app)).value, { version: 1, tools: { git: GIT } });
});

test('a tool path that is relative, a folder, not executable, misnamed, missing or inside a listed project is refused, not saved, and pushes nothing', async (t) => {
  const dir = tempDir(t, 'settings');
  const app = path.join(dir, 'app-folder');
  mkdirSync(path.join(dir, 'folder', 'git'), { recursive: true });
  writeFileSync(path.join(dir, 'git'), '#!/bin/sh\n');
  chmodSync(path.join(dir, 'git'), 0o644);
  const project = mk(path.join(dir, 'proj'));
  const inProject = program(path.join(mk(path.join(project, 'bin')), 'git'), '#!/bin/sh\n');
  const outside = program(path.join(dir, 'outside-git'), '#!/bin/sh\n');
  symlinkSync(inProject, path.join(dir, 'link-git'));
  const c = pushes();
  const cases: Array<[string, RegExp]> = [
    ['git', /absolute/],
    [path.join(dir, 'folder', 'git'), /not a file/],
    [path.join(dir, 'git'), /not executable/],
    [outside, /named git/],
    [path.join(dir, 'missing', 'git'), /not found/],
    [inProject, /inside the project/],
    [path.join(dir, 'link-git'), /named git/],
  ];
  for (const [file, why] of cases) {
    const r = await chooseTool(app, 'git', file, [project], c.push);
    assert.match(r.ok ? 'accepted' : r.why, why, file);
  }
  // A link named git, outside the project, whose target is inside it: judged by its real path too.
  symlinkSync(inProject, path.join(dir, 'folder', 'git', 'git'));
  const viaLink = await chooseTool(app, 'git', path.join(dir, 'folder', 'git', 'git'), [project], c.push);
  assert.match(viaLink.ok ? 'accepted' : viaLink.why, /inside the project/);
  assert.equal(c.n, 0);
  assert.deepEqual((await loadSettings(app)).value, { version: 1, tools: {} });
});

test('a tool named by a link keeps the link\'s path, so a versioned target (claude\'s) still checks', async (t) => {
  const dir = tempDir(t, 'settings');
  const target = program(path.join(mk(path.join(dir, 'versions')), '2.1.0'), '#!/bin/sh\n');
  symlinkSync(target, path.join(mk(path.join(dir, 'bin')), 'claude'));
  const r = await chooseTool(path.join(dir, 'app-folder'), 'claude', path.join(dir, 'bin', 'claude'), []);
  assert.deepEqual(r, { ok: true, settings: { version: 1, tools: { claude: path.join(dir, 'bin', 'claude') } } });
});

test('unset tools are looked up on the PATH, first folder first, skipping a listed project; set ones are checked where they are', async (t) => {
  const dir = tempDir(t, 'tools');
  const a = mk(path.join(dir, 'a'));
  const b = mk(path.join(dir, 'b'));
  writeFileSync(path.join(a, 'keel'), 'not executable');
  const keel = program(path.join(b, 'keel'), '#!/bin/sh\n');
  const claudeA = program(path.join(a, 'claude'), '#!/bin/sh\n');
  program(path.join(b, 'claude'), '#!/bin/sh\n');
  assert.equal(await lookup('keel', [a, b], []), keel);
  assert.equal(await lookup('claude', [a, b], []), claudeA);
  assert.equal(await lookup('git', [a, b], []), null);
  assert.equal(await lookup('claude', [a, b], [a]), path.join(b, 'claude'));
  assert.deepEqual(await toolStates({ version: 1, tools: { git: path.join(dir, 'gone', 'git') } }, [a, b], []), {
    keel: { path: keel, source: 'path', problem: null },
    git: { path: path.join(dir, 'gone', 'git'), source: 'set', problem: 'not found' },
    claude: { path: claudeA, source: 'path', problem: null },
  });
  assert.deepEqual((await toolStates({ version: 1, tools: {} }, [], [])).git, { path: null, source: 'missing', problem: 'not found on the PATH' });
});

test('a main checkout whose HEAD commits .keel/config.toml is added; its files and git status stay the same through add, settings writes and remove', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const app = path.join(dir, 'app-folder');
  const project = keelRepo(path.join(dir, 'proj'));
  const before = snapshot(project);
  const statusBefore = status(project);
  const c = pushes();
  const r = await addProject(app, project, GIT, opts(home), c.push);
  assert.equal(r.ok, true, r.ok ? '' : r.why);
  assert.equal(c.n, 1);
  const list = (await loadProjects(app)).value;
  assert.equal(list.projects.length, 1);
  const id = list.projects[0]?.id ?? '';
  assert.match(id, /^p[0-9a-f]{8}$/);
  assert.deepEqual(list.projects[0], { id, path: project, name: 'proj' });
  assert.deepEqual(snapshot(project), before);
  await chooseTool(app, 'git', GIT, [project], c.push);
  assert.deepEqual(snapshot(project), before);
  await resetTool(app, 'git', c.push);
  assert.deepEqual(snapshot(project), before);
  assert.deepEqual(await removeProject(app, id, c.push), { ok: true, list: { version: 1, projects: [] } });
  assert.equal(c.n, 4);
  assert.deepEqual((await loadProjects(app)).value, { version: 1, projects: [] });
  assert.deepEqual(snapshot(project), before);
  assert.equal(status(project), statusBefore);
  assert.deepEqual(readdirSync(app).sort(), [PROJECTS_FILE, SETTINGS_FILE].sort());
});

test('a folder given as a subfolder is refused, and one given by a link is listed by its real path', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  mkdirSync(path.join(project, 'sub'));
  const sub = await addProject(path.join(dir, 'app1'), path.join(project, 'sub'), GIT, opts(home));
  assert.match(sub.ok ? 'accepted' : sub.why, /not the top folder/);
  symlinkSync(project, path.join(dir, 'link'));
  const viaLink = await addProject(path.join(dir, 'app2'), path.join(dir, 'link'), GIT, opts(home));
  assert.equal(viaLink.ok && viaLink.project.path, project);
});

test('a linked worktree, a plain folder, a config only in the working tree, a missing folder and a second project are refused, write nothing and push nothing', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const app = path.join(dir, 'app-folder');
  const main = keelRepo(path.join(dir, 'main'));
  sh(GIT, main, ['worktree', 'add', '-q', path.join(dir, 'linked')]);
  const plainDir = mk(path.join(dir, 'plain'));
  const loose = path.join(dir, 'loose');
  mkdirSync(path.join(loose, '.keel'), { recursive: true });
  writeFileSync(path.join(loose, 'README.md'), 'x\n');
  sh(GIT, loose, ['init', '-q']);
  sh(GIT, loose, ['add', 'README.md']);
  sh(GIT, loose, ['commit', '-q', '-m', 'init']);
  writeFileSync(path.join(loose, '.keel', 'config.toml'), 'code_paths = []\n');
  const c = pushes();
  const cases: Array<[string, RegExp]> = [
    [path.join(dir, 'linked'), /linked worktree/],
    [plainDir, /not a git checkout/],
    [loose, /HEAD does not commit \.keel\/config\.toml/],
    [path.join(dir, 'missing'), /not found/],
    ['relative/x', /absolute/],
  ];
  for (const [folder, why] of cases) {
    const before = snapshot(dir);
    const r = await addProject(app, folder, GIT, opts(home), c.push);
    assert.match(r.ok ? 'accepted' : r.why, why, folder);
    assert.deepEqual(snapshot(dir), before, folder);
  }
  assert.equal(c.n, 0);
  assert.equal((await addProject(app, main, GIT, opts(home), c.push)).ok, true);
  const second = keelRepo(path.join(dir, 'second'));
  const listed = readFileSync(path.join(app, PROJECTS_FILE), 'utf8');
  const r = await addProject(app, second, GIT, opts(home), c.push);
  assert.match(r.ok ? 'accepted' : r.why, /one project/);
  const again = await addProject(app, main, GIT, opts(home), c.push);
  assert.match(again.ok ? 'accepted' : again.why, /one project/);
  assert.equal(c.n, 1);
  assert.equal(readFileSync(path.join(app, PROJECTS_FILE), 'utf8'), listed);
});

test('removing an id not on the list is refused, changes nothing and pushes nothing', async (t) => {
  const app = tempDir(t, 'projects');
  const c = pushes();
  assert.deepEqual(await removeProject(app, 'p00000000', c.push), { ok: false, why: 'no such project' });
  assert.deepEqual(await removeProject(app, '../x', c.push), { ok: false, why: 'no such project' });
  assert.equal(c.n, 0);
  assert.deepEqual(readdirSync(app), []);
});

test('a HEAD whose .keel/config.toml is a folder or a link is refused: the project must commit it as a file', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const folder = path.join(dir, 'folder');
  mkdirSync(path.join(folder, '.keel', 'config.toml'), { recursive: true });
  writeFileSync(path.join(folder, '.keel', 'config.toml', 'x'), 'x\n');
  const link = path.join(dir, 'link');
  mkdirSync(path.join(link, '.keel'), { recursive: true });
  writeFileSync(path.join(link, 'real.toml'), 'code_paths = []\n');
  symlinkSync('../real.toml', path.join(link, '.keel', 'config.toml'));
  for (const repo of [folder, link]) {
    sh(GIT, repo, ['init', '-q']);
    sh(GIT, repo, ['add', '.']);
    sh(GIT, repo, ['commit', '-q', '-m', 'init']);
    const r = await addProject(path.join(dir, `app-${path.basename(repo)}`), repo, GIT, opts(home));
    assert.match(r.ok ? 'accepted' : r.why, /HEAD does not commit \.keel\/config\.toml/, repo);
  }
});

test('a git too old for --no-lazy-fetch is named, not reported as a folder that is not a git checkout', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  const old = program(path.join(mk(path.join(dir, 'old')), 'git'), "#!/bin/sh\necho 'unknown option: --no-lazy-fetch' >&2\nexit 129\n");
  const r = await addProject(path.join(dir, 'app-folder'), project, old, opts(home));
  assert.match(r.ok ? 'accepted' : r.why, /older than 2\.44/);
});
