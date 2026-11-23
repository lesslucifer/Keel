// Settings and the project list at their real boundary: files in a temp app folder, real git in temp
// projects, fake tools as scripts. The app writes only in its own folder: a project's every file
// and its git status are the same before and after add, remove and every settings write.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, symlinkSync, chmodSync, existsSync, appendFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import { readJson, writeAtomic, load, io } from '../../src/main/store.ts';
import { loadSettings, chooseTool, resetTool, SETTINGS_FILE } from '../../src/main/settings.ts';
import { loadProjects, addProject, removeProject, PROJECTS_FILE } from '../../src/main/projects.ts';
import { ProjectRunner } from '../../src/main/commands.ts';
import { lookup, toolStates, toolProblem } from '../../src/main/tools.ts';
import { tempDir, program, realGit, sh, snapshot, mk, recorder, oldGit, records, type OldGitAs } from '../support/fs.ts';

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
  // A link named git inside the project whose target is outside it: judged by its own path too.
  const linkIn = path.join(mk(path.join(project, 'links')), 'git');
  symlinkSync(GIT, linkIn);
  const viaOwnPath = await chooseTool(app, 'git', linkIn, [project], c.push);
  assert.match(viaOwnPath.ok ? 'accepted' : viaOwnPath.why, /inside the project/);
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

test('a git inside the folder being added is refused before it ever runs', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  const record = path.join(dir, 'record.json');
  const shim = recorder(path.join(mk(path.join(project, 'bin')), 'git'), record, 0, `${project}\n`);
  const linked = path.join(dir, 'link-bin');
  mkdirSync(linked);
  symlinkSync(shim, path.join(linked, 'git'));
  for (const git of [shim, path.join(linked, 'git')]) {
    const r = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
    assert.match(r.ok ? 'accepted' : r.why, /^git is inside the project proj/, git);
    assert.ok(!existsSync(record), `${git} was run on the folder`);
  }
});

test('a tool link inside a project, named through a linked parent folder, is refused, and such a git never runs on the folder', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  const record = path.join(dir, 'record.json');
  // The program is outside the project; the link to it sits inside the project's bin.
  const outside = recorder(path.join(mk(path.join(dir, 'outside')), 'git'), record, 0, `${project}\n`);
  symlinkSync(outside, path.join(mk(path.join(project, 'bin')), 'git'));
  // An alias of the parent folder (as /tmp is of /private/tmp), and one of the project's bin.
  symlinkSync(dir, path.join(dir, 'alias'));
  symlinkSync(path.join(project, 'bin'), path.join(dir, 'bin-alias'));
  const spellings = [path.join(dir, 'alias', 'proj', 'bin', 'git'), path.join(dir, 'bin-alias', 'git')];
  for (const git of spellings) {
    assert.match((await toolProblem('git', git, [project])) ?? 'accepted', /inside the project proj/, git);
    const r = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
    assert.match(r.ok ? 'accepted' : r.why, /^git is inside the project proj/, git);
    assert.ok(!existsSync(record), `${git} was run on the folder`);
  }
  // Found on a login-PATH entry spelled through the alias: skipped, as a listed project's own git.
  assert.equal(await lookup('git', [path.join(dir, 'alias', 'proj', 'bin')], [project]), null);
});

test('a git older than 2.45 (no --no-lazy-fetch) adds a project that is not a partial clone: the three reads run again without the flag', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  const record = path.join(dir, 'record.jsonl');
  const git = oldGit(path.join(mk(path.join(dir, 'old')), 'git'), record, GIT);
  const r = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
  assert.equal(r.ok, true, r.ok ? '' : r.why);
  const seen = records(record);
  const base = ['-c', 'core.fsmonitor=false', '--no-optional-locks'];
  assert.deepEqual(seen.map((x) => x.argv), [
    [...base, '--no-lazy-fetch', '-C', project, 'rev-parse', '--show-toplevel'],
    [...base, '-C', project, 'config', '--get-regexp', '^(extensions\\.partialclone|remote\\..*\\.(promisor|partialclonefilter))$'],
    [...base, '-C', project, 'rev-parse', '--show-toplevel'],
    [...base, '-C', project, 'rev-parse', '--git-common-dir'],
    [...base, '-C', project, 'ls-tree', '--full-tree', 'HEAD', '--', '.keel/config.toml'],
  ]);
  for (const x of seen) for (const key of Object.keys(x.env)) assert.ok(!key.startsWith('GIT_'), `${key} reached git`);
});

test('a git older than 2.45 is recognised whatever the language or case of its "unknown option" line', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const cases: Array<[string, OldGitAs]> = [['de', { lang: 'de' }], ['fr', { lang: 'fr' }], ['2.16', { version: '2.16' }]];
  for (const [name, as] of cases) {
    const project = keelRepo(path.join(dir, `proj-${name}`));
    const git = oldGit(path.join(mk(path.join(dir, `old-${name}`)), 'git'), path.join(dir, `record-${name}.jsonl`), GIT, as);
    const r = await addProject(path.join(dir, `app-${name}`), project, git, opts(home));
    assert.equal(r.ok, true, `${name}: ${r.ok ? '' : r.why}`);
  }
});

test('a git 2.17 to 2.30, whose rev-parse has no --path-format, adds a real main checkout and still refuses a linked worktree', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  sh(GIT, project, ['worktree', 'add', '-q', path.join(dir, 'linked')]);
  const git = oldGit(path.join(mk(path.join(dir, 'old')), 'git'), path.join(dir, 'record.jsonl'), GIT, { version: '2.30' });
  const linked = await addProject(path.join(dir, 'app-folder'), path.join(dir, 'linked'), git, opts(home));
  assert.match(linked.ok ? 'accepted' : linked.why, /linked worktree/);
  const r = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
  assert.equal(r.ok, true, r.ok ? '' : r.why);
});

test('a git older than 2.15 (no --no-optional-locks) is refused by name', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  const record = path.join(dir, 'record.jsonl');
  const git = oldGit(path.join(mk(path.join(dir, 'old')), 'git'), record, GIT, { version: '2.14' });
  const r = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
  assert.match(r.ok ? 'accepted' : r.why, /^this git is older than 2\.15/);
  assert.equal(records(record).length, 1);
});

test('a git older than 2.45 is refused for a partial clone, by name, and runs nothing past the config read', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  for (const [i, lines] of [
    ['[remote "origin"]', '\tpromisor = true'],
    ['[remote "origin"]', '\tpartialclonefilter = blob:none'],
    ['[core]', '\trepositoryformatversion = 1', '[extensions]', '\tpartialClone = origin'],
  ].entries()) {
    const project = keelRepo(path.join(dir, `proj${i}`));
    appendFileSync(path.join(project, '.git', 'config'), `${lines.join('\n')}\n`);
    const before = snapshot(project);
    const record = path.join(dir, `record${i}.jsonl`);
    const git = oldGit(path.join(mk(path.join(dir, `old${i}`)), 'git'), record, GIT);
    const r = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
    assert.match(r.ok ? 'accepted' : r.why, /^a partial clone needs git 2\.45 or later/, lines.join(' '));
    const base = ['-c', 'core.fsmonitor=false', '--no-optional-locks'];
    assert.deepEqual(records(record).map((x) => x.argv), [
      [...base, '--no-lazy-fetch', '-C', project, 'rev-parse', '--show-toplevel'],
      [...base, '-C', project, 'config', '--get-regexp', '^(extensions\\.partialclone|remote\\..*\\.(promisor|partialclonefilter))$'],
    ]);
    assert.deepEqual(snapshot(project), before);
  }
});

test('a git 2.45 or later never reads the config: the three reads carry --no-lazy-fetch', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  const record = path.join(dir, 'record.jsonl');
  const git = program(
    path.join(mk(path.join(dir, 'new')), 'git'),
    `#!${process.execPath}\nrequire('node:fs').appendFileSync(${JSON.stringify(record)}, JSON.stringify({ argv: process.argv.slice(2), env: {} }) + '\\n');\nconst r = require('node:child_process').spawnSync(${JSON.stringify(GIT)}, process.argv.slice(2), { stdio: 'inherit', env: process.env });\nprocess.exit(r.status ?? 1);\n`,
  );
  const r = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
  assert.equal(r.ok, true, r.ok ? '' : r.why);
  const seen = records(record).map((x) => x.argv);
  assert.equal(seen.length, 3);
  for (const argv of seen) assert.equal(argv[3], '--no-lazy-fetch', argv.join(' '));
});

/** Hold the store's next readJson of `file`: it answers only when the test releases it (a controlled promise, no sleep). */
function holdRead(t: { after: (fn: () => void) => void }, file: string) {
  const real = io.readJson;
  t.after(() => {
    io.readJson = real;
  });
  let release: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => (release = resolve));
  let reading: () => void = () => undefined;
  const firstRead = new Promise<void>((resolve) => (reading = resolve));
  const calls: string[] = [];
  io.readJson = async (f: string) => {
    calls.push(f);
    if (calls.length === 1) {
      reading();
      await gate;
    }
    return real(f);
  };
  return { calls, release, firstRead };
}

const parseObj = (v: unknown) => (typeof v === 'object' && v !== null && !Array.isArray(v) && 'ok' in v ? (v as { ok: number }) : null);

test('epic review round 1 (store.ts:82): two loads of one broken file run one at a time: the second reads only once the first has set the file aside, both load the defaults, and one file is set aside', async (t) => {
  const dir = tempDir(t, 'store-race');
  const file = path.join(dir, 'projects.json');
  writeFileSync(file, '{"half');
  const held = holdRead(t, file);
  const first = load(file, parseObj, () => ({ ok: 0 }));
  const second = load(file, parseObj, () => ({ ok: 0 }));
  await held.firstRead;
  assert.equal(held.calls.length, 1, 'the second load has not read the file while the first holds it');
  held.release();
  const [a, b] = await Promise.all([first, second]);
  assert.deepEqual([a.value, b.value], [{ ok: 0 }, { ok: 0 }]);
  assert.notEqual(a.setAside, null);
  assert.equal(b.setAside, null, 'the second found no file: nothing more to set aside');
  assert.deepEqual(readdirSync(dir).filter((n) => n.includes('.broken-')).length, 1);
});

test('epic review round 1 (store.ts:82): a write started while a load holds the file lands after it, so a file just written is never set aside', async (t) => {
  const dir = tempDir(t, 'store-write');
  const file = path.join(dir, 'projects.json');
  writeFileSync(file, '{"half');
  const held = holdRead(t, file);
  const loading = load(file, parseObj, () => ({ ok: 0 }));
  const writing = writeAtomic(file, { ok: 1 });
  await held.firstRead;
  held.release();
  await Promise.all([loading, writing]);
  assert.deepEqual(JSON.parse(readFileSync(file, 'utf8')), { ok: 1 }, 'the written value stands');
  assert.deepEqual((await load(file, parseObj, () => ({ ok: 0 }))).value, { ok: 1 });
});

test('epic review round 1 (store.ts:82): a broken file gone before it is set aside reads as missing: the defaults, nothing set aside, no ENOENT', async (t) => {
  const dir = tempDir(t, 'store-gone');
  const file = path.join(dir, 'settings.json');
  writeFileSync(file, 'not json');
  const real = io.readJson;
  t.after(() => {
    io.readJson = real;
  });
  io.readJson = async (f: string) => {
    const r = await real(f);
    rmSync(f);
    return r;
  };
  assert.deepEqual(await load(file, parseObj, () => ({ ok: 0 })), { value: { ok: 0 }, setAside: null });
});

test('epic review round 1 (store.ts:82): a write that fails leaves its file\'s turn: the next load and write of that file still run', async (t) => {
  const dir = tempDir(t, 'store-fail');
  const file = path.join(dir, 'sent.json');
  await assert.rejects(writeAtomic(file, { n: 1n }), TypeError);
  assert.deepEqual(await load(file, parseObj, () => ({ ok: 0 })), { value: { ok: 0 }, setAside: null });
  await writeAtomic(file, { ok: 2 });
  assert.deepEqual((await load(file, parseObj, () => ({ ok: 0 }))).value, { ok: 2 });
  assert.deepEqual(readdirSync(dir), ['sent.json'], 'no temp file left');
});

test('W12 review round 1 (commands.ts:327): adding a project reads through the same old-git runner as the reader and Open, so an old git and a failed settings read give the same answer there', async (t) => {
  const dir = tempDir(t, 'projects');
  const home = mk(path.join(dir, 'home'));
  const project = keelRepo(path.join(dir, 'proj'));
  // A git older than 2.45 whose config read fails (exit 2): neither "a partial clone" nor "not one".
  const git = program(path.join(mk(path.join(dir, 'odd')), 'git'), `#!/bin/sh\ncase "$*" in *--no-lazy-fetch*) echo "unknown option: --no-lazy-fetch" >&2; exit 129;; *--get-regexp*) exit 2;; esac\nexec ${GIT} "$@"\n`);
  const added = await addProject(path.join(dir, 'app-folder'), project, git, opts(home));
  const read = await new ProjectRunner(project, opts(home)).run(git, 'toplevel', {});
  assert.equal(typeof read, 'string');
  assert.deepEqual(added, { ok: false, why: read });
});
