// The PATH read at its real boundary, a spawned login shell; fake shells stand in for the owner's.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { readLoginPath, SYSTEM_PATH } from '../../src/main/login-path.ts';
import { tempDir, program, HANG, groupGone } from '../support/fs.ts';

process.env.GIT_DIR = '/nonexistent/.git';
process.env.KEEL_TEST_SECRET = 'must-not-leak';

test('a login shell printing profile noise around the markers gives its PATH, relative and empty entries dropped', async (t) => {
  const dir = tempDir(t, 'login');
  const shell = program(path.join(dir, 'zsh'), [
    '#!/bin/sh',
    `printf '%s\\n' "$@" > ${dir}/args`,
    `/usr/bin/env > ${dir}/env`,
    "echo 'Last login: Fri Nov 20 on ttys001'",
    "echo 'oh-my-zsh: update available'",
    "PATH='/opt/homebrew/bin::bin:/usr/bin:./x:/Users/me/.cargo/bin:/opt/homebrew/bin:'",
    'export PATH',
    '/bin/sh -c "$2"',
    "echo 'goodbye'",
  ].join('\n'));
  const r = await readLoginPath({ shell });
  assert.deepEqual(r, { entries: ['/opt/homebrew/bin', '/usr/bin', '/Users/me/.cargo/bin'], source: 'login-shell', reason: null });
  assert.equal(readFileSync(path.join(dir, 'args'), 'utf8').split('\n')[0], '-ilc');
  const env = readFileSync(path.join(dir, 'env'), 'utf8');
  assert.match(env, /^TERM=dumb$/m);
  assert.match(env, new RegExp(`^SHELL=${shell}$`, 'm'));
  assert.match(env, /^HOME=\//m);
  assert.doesNotMatch(env, /^GIT_/m);
  assert.doesNotMatch(env, /KEEL_TEST_SECRET/);
});

test('a login shell that hangs is killed at the timeout with its children, and the fallback says why', async (t) => {
  const dir = tempDir(t, 'login');
  const shell = program(path.join(dir, 'zsh'), `#!/bin/sh\necho $$ > ${dir}/pid\n${HANG}\n`);
  const r = await readLoginPath({ shell, timeoutMs: 300, toolDirs: ['/Users/me/.cargo/bin'] });
  assert.deepEqual(r, { entries: [...SYSTEM_PATH, '/Users/me/.cargo/bin'], source: 'fallback', reason: 'the login shell did not answer within 0.3 s' });
  assert.ok(groupGone(Number(readFileSync(path.join(dir, 'pid'), 'utf8'))), 'the hung shell or its child is still running');
});

test('a login shell that exits non-zero falls back, even if it printed a PATH', async (t) => {
  const dir = tempDir(t, 'login');
  const shell = program(path.join(dir, 'zsh'), '#!/bin/sh\n/bin/sh -c "$2"\nexit 3\n');
  assert.deepEqual(await readLoginPath({ shell }), { entries: SYSTEM_PATH, source: 'fallback', reason: 'the login shell exited 3' });
});

test('a login shell that prints no markers, or no absolute folder, falls back', async (t) => {
  const dir = tempDir(t, 'login');
  const quiet = program(path.join(dir, 'quiet'), "#!/bin/sh\necho 'no path here'\n");
  assert.deepEqual(await readLoginPath({ shell: quiet }), { entries: SYSTEM_PATH, source: 'fallback', reason: 'the login shell printed no PATH' });
  const empty = program(path.join(dir, 'empty'), "#!/bin/sh\nPATH='rel:' /bin/sh -c \"$2\"\n");
  assert.deepEqual(await readLoginPath({ shell: empty }), { entries: SYSTEM_PATH, source: 'fallback', reason: "the login shell's PATH has no absolute folder" });
});

test('a login shell that cannot start falls back', async (t) => {
  const dir = tempDir(t, 'login');
  const r = await readLoginPath({ shell: path.join(dir, 'missing-shell') });
  assert.equal(r.source, 'fallback');
  assert.match(r.reason ?? '', /^the login shell could not start/);
});
