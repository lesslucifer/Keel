// The fixed commands at their real boundary, a spawned program: a fake git records its argv and
// environment; real git in temp repos answers from the repo named, never from an inherited GIT_*.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { run, capture, LOCKED, OUTPUT_CAP } from '../../src/main/commands.ts';
import { commitsConfig } from '../../src/main/projects.ts';
import { tempDir, recorder, program, realGit, sh, HANG, groupGone, holder, endHolder, bounded, alive, within } from '../support/fs.ts';

// The app's own environment carries git settings that would redirect or hijack git if they reached it.
process.env.GIT_DIR = '/nonexistent/hijack/.git';
process.env.GIT_WORK_TREE = '/nonexistent';
process.env.GIT_CONFIG_PARAMETERS = "'core.fsmonitor'='true'";
process.env.GIT_EXTERNAL_DIFF = '/nonexistent/diff';
process.env.KEEL_TEST_SECRET = 'must-not-leak';

test('a fake git gets the locked flags first, the slot whole, no shell, and no GIT_* or other inherited variable', async (t) => {
  const dir = tempDir(t, 'run');
  const record = path.join(dir, 'record.json');
  const git = recorder(path.join(dir, 'git'), record, 0, '/somewhere\n');
  const project = path.join(dir, `$(touch pwned); x \`touch ${dir}/pwned2\``);
  const r = await run(git, 'toplevel', { project }, { path: ['/usr/bin', '/bin'] });
  assert.deepEqual([r.code, r.stdout, r.timedOut, r.startError], [0, '/somewhere\n', false, null]);
  const seen = JSON.parse(readFileSync(record, 'utf8')) as { argv: string[]; env: Record<string, string> };
  assert.deepEqual(seen.argv, ['-c', 'core.fsmonitor=false', '--no-optional-locks', '--no-lazy-fetch', '-C', project, 'rev-parse', '--show-toplevel']);
  assert.deepEqual(seen.argv.slice(0, LOCKED.length), LOCKED);
  assert.ok(!existsSync(path.join(dir, 'pwned')) && !existsSync(path.join('/', 'pwned')) && !existsSync(path.join(dir, 'pwned2')), 'a shell ran the slot');
  assert.equal(seen.env.PATH, '/usr/bin:/bin');
  // __CF_USER_TEXT_ENCODING is set by macOS's CoreFoundation inside the child itself (env -i node shows it too).
  for (const key of Object.keys(seen.env)) {
    assert.ok(!key.startsWith('GIT_'), `${key} reached git`);
    assert.ok(['PATH', 'HOME', 'USER', 'LOGNAME', 'LANG', 'TMPDIR', '__CF_USER_TEXT_ENCODING'].includes(key), `${key} reached git`);
  }
});

test('a fake git that exits non-zero is reported with its code and stderr, not thrown', async (t) => {
  const dir = tempDir(t, 'run');
  const git = program(path.join(dir, 'git'), "#!/bin/sh\necho 'fatal: not a git repository' >&2\nexit 128\n");
  const r = await run(git, 'toplevel', { project: dir }, { path: ['/usr/bin', '/bin'] });
  assert.deepEqual([r.code, r.timedOut], [128, false]);
  assert.match(r.stderr, /not a git repository/);
});

test('a git that hangs is killed at the timeout with its whole process group, and reported so', async (t) => {
  const dir = tempDir(t, 'run');
  const git = program(path.join(dir, 'git'), `#!/bin/sh\necho $$ > ${dir}/pid\n${HANG}\n`);
  const r = await run(git, 'toplevel', { project: dir }, { path: ['/usr/bin', '/bin'], timeoutMs: 300 });
  assert.deepEqual([r.timedOut, r.code], [true, null]);
  assert.ok(groupGone(Number(readFileSync(path.join(dir, 'pid'), 'utf8'))), 'the hung git or its child is still running');
});

test('a program that exits while a descendant that left its process group still holds its output is answered from its own exit, within the timeout', async (t) => {
  const dir = tempDir(t, 'run');
  const pidFile = path.join(dir, 'holder');
  endHolder(t, pidFile);
  const fake = program(path.join(dir, 'fake'), `#!/bin/sh\necho answer\n${holder(pidFile, true)}\nexit 0\n`);
  const r = await bounded(6_000, 'capture', capture(fake, [], { PATH: '/usr/bin:/bin' }, 2_000));
  assert.deepEqual([r.code, r.timedOut, r.startError, r.stdout], [0, false, null, 'answer\n']);
});

test('a program that exits 0 while its own background job holds its output is answered with its exit code, and the job is never killed', async (t) => {
  const dir = tempDir(t, 'run');
  const pidFile = path.join(dir, 'holder');
  endHolder(t, pidFile);
  const fake = program(path.join(dir, 'fake'), `#!/bin/sh\necho answer\n${holder(pidFile, false)}\nexit 0\n`);
  const start = Date.now();
  const r = await capture(fake, [], { PATH: '/usr/bin:/bin' }, 1_500);
  assert.deepEqual([r.code, r.timedOut, r.startError, r.stdout], [0, false, null, 'answer\n']);
  assert.ok(await within(3_000, () => existsSync(pidFile) && readFileSync(pidFile, 'utf8').trim() !== ''), 'the background job never started');
  const pid = Number(readFileSync(pidFile, 'utf8'));
  // Past the timeout the program had: its group is never killed after it exited by itself.
  await new Promise((resolve) => setTimeout(resolve, Math.max(0, start + 2_000 - Date.now())));
  assert.ok(alive(pid), 'the background job was killed after the program had exited by itself');
});

test('a git that cannot start is reported with the reason, not thrown', async (t) => {
  const dir = tempDir(t, 'run');
  const r = await run(path.join(dir, 'no-such-git'), 'toplevel', { project: dir }, { path: ['/usr/bin'] });
  assert.deepEqual([r.code, r.startError], [null, 'ENOENT']);
});

test('output past the cap is dropped, and the run still ends', async (t) => {
  const dir = tempDir(t, 'run');
  const big = program(path.join(dir, 'big'), `#!/bin/sh\n/usr/bin/head -c ${OUTPUT_CAP + 4096} /dev/zero\n`);
  const r = await capture(big, [], { PATH: '/usr/bin:/bin' }, 10_000);
  assert.equal(r.code, 0);
  assert.equal(r.stdout.length, OUTPUT_CAP);
});

test('a git path that is not absolute is refused before anything runs', async () => {
  await assert.rejects(run('git', 'toplevel', { project: '/p' }, { path: ['/usr/bin'] }), /absolute/);
});

test('real git answers from the project named, not from the GIT_DIR the app inherited', async (t) => {
  const git = realGit();
  const dir = tempDir(t, 'run');
  const a = path.join(dir, 'a');
  const b = path.join(dir, 'b');
  mkdirSync(a);
  mkdirSync(b);
  sh(git, a, ['init', '-q']);
  sh(git, b, ['init', '-q']);
  process.env.GIT_DIR = path.join(b, '.git');
  const opts = { path: [path.dirname(git), '/usr/bin', '/bin'] };
  const top = await run(git, 'toplevel', { project: a }, opts);
  assert.equal(top.code, 0, top.stderr);
  assert.equal(top.stdout.trim(), a);
  assert.equal((await run(git, 'common-dir', { project: a }, opts)).stdout.trim(), path.join(a, '.git'));
  const hasConfig = async (project: string) => {
    const r = await run(git, 'has-config', { project }, opts);
    return r.code === 0 && commitsConfig(r.stdout);
  };
  assert.equal(await hasConfig(a), false);
  mkdirSync(path.join(a, '.keel'));
  writeFileSync(path.join(a, '.keel', 'config.toml'), 'code_paths = []\n');
  sh(git, a, ['add', '.keel/config.toml']);
  sh(git, a, ['commit', '-q', '-m', 'keel']);
  assert.equal(await hasConfig(a), true);
  assert.equal(await hasConfig(b), false);
});
