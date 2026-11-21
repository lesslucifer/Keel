import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LOCKED, NO_EXTERNAL, ENV_KEYS, READ_TIMEOUT_MS, COMMANDS, argv, lock, childEnv } from '../../src/main/commands.ts';
import { seeded, hostileString, pick } from '../support/seeded.ts';

test('every fixed command starts with -c core.fsmonitor=false --no-optional-locks', () => {
  assert.deepEqual(LOCKED, ['-c', 'core.fsmonitor=false', '--no-optional-locks']);
  for (const name of Object.keys(COMMANDS)) assert.deepEqual(argv(name, { project: '/p' }).slice(0, 3), LOCKED, name);
});

test('the fixed commands of W7 are exactly the plan\'s three lines', () => {
  assert.deepEqual(Object.keys(COMMANDS).sort(), ['common-dir', 'has-config', 'toplevel']);
  assert.deepEqual(argv('toplevel', { project: '/Users/me/Keel' }), [...LOCKED, '-C', '/Users/me/Keel', 'rev-parse', '--show-toplevel']);
  assert.deepEqual(argv('common-dir', { project: '/Users/me/Keel' }), [...LOCKED, '-C', '/Users/me/Keel', 'rev-parse', '--path-format=absolute', '--git-common-dir']);
  assert.deepEqual(argv('has-config', { project: '/Users/me/Keel' }), [...LOCKED, '-C', '/Users/me/Keel', 'cat-file', '-e', 'HEAD:.keel/config.toml']);
});

test('a read times out after 10 s', () => {
  assert.equal(READ_TIMEOUT_MS, 10_000);
});

test('a project slot is one absolute path, passed as one argument whatever shell characters it holds', () => {
  const p = '/Users/me/a b/$(touch pwned); x`id`|&>*';
  assert.equal(argv('toplevel', { project: p })[4], p);
  assert.equal(argv('toplevel', { project: p }).length, 7);
});

test('a project slot refuses a relative path, an empty one, NUL, a control character, a leading dash, a home shorthand', () => {
  for (const project of ['Keel', '', './Keel', '-C/x', '/a\u0000b', '/a\nb', '/a\rb', '/a\u0085b', '~/Keel']) {
    assert.throws(() => argv('toplevel', { project }), /project/, JSON.stringify(project));
  }
  assert.throws(() => argv('toplevel', { project: 7 as unknown as string }), /project/);
});

test('an unknown command, a missing slot or an extra slot is refused', () => {
  assert.throws(() => argv('push', { project: '/p' }), /unknown/);
  assert.throws(() => argv('__proto__', { project: '/p' }), /unknown/);
  assert.throws(() => argv('constructor', { project: '/p' }), /unknown/);
  assert.throws(() => argv('toplevel', {}), /slots/);
  assert.throws(() => argv('toplevel', { project: '/p', extra: 'x' }), /slots/);
});

test('lock() puts --no-ext-diff --no-textconv right after a diff, show or log -p subcommand, and nowhere else', () => {
  assert.deepEqual(NO_EXTERNAL, ['--no-ext-diff', '--no-textconv']);
  assert.deepEqual(lock(['-C', '/p', 'diff', 'a', 'b']), [...LOCKED, '-C', '/p', 'diff', ...NO_EXTERNAL, 'a', 'b']);
  assert.deepEqual(lock(['show', 'HEAD']), [...LOCKED, 'show', ...NO_EXTERNAL, 'HEAD']);
  assert.deepEqual(lock(['-C', '/p', 'log', '-p', '-1']), [...LOCKED, '-C', '/p', 'log', ...NO_EXTERNAL, '-p', '-1']);
  assert.deepEqual(lock(['log', '--patch']), [...LOCKED, 'log', ...NO_EXTERNAL, '--patch']);
  assert.deepEqual(lock(['log', '-u']), [...LOCKED, 'log', ...NO_EXTERNAL, '-u']);
  assert.deepEqual(lock(['log', '--oneline']), [...LOCKED, 'log', '--oneline']);
  assert.deepEqual(lock(['-C', '/p', 'rev-parse', 'HEAD']), [...LOCKED, '-C', '/p', 'rev-parse', 'HEAD']);
});

test('the child environment holds only PATH (the login PATH) and HOME, USER, LOGNAME, LANG, TMPDIR from the app', () => {
  assert.deepEqual(ENV_KEYS, ['HOME', 'USER', 'LOGNAME', 'LANG', 'TMPDIR']);
  const env = childEnv(['/opt/homebrew/bin', '/usr/bin'], {
    HOME: '/Users/me', USER: 'me', LOGNAME: 'me', LANG: 'en_US.UTF-8', TMPDIR: '/tmp/x', PATH: '/evil',
    GIT_DIR: '/x', GIT_CONFIG_PARAMETERS: "'core.pager'='sh'", GIT_EXTERNAL_DIFF: '/x', NODE_OPTIONS: '--require /x', DYLD_INSERT_LIBRARIES: '/x', ELECTRON_RUN_AS_NODE: '1',
  });
  assert.deepEqual(env, { PATH: '/opt/homebrew/bin:/usr/bin', HOME: '/Users/me', USER: 'me', LOGNAME: 'me', LANG: 'en_US.UTF-8', TMPDIR: '/tmp/x' });
  assert.equal(Object.getPrototypeOf(env), Object.prototype);
});

test('the child environment skips an unset or empty key rather than passing it empty', () => {
  assert.deepEqual(childEnv(['/usr/bin'], { HOME: '/h', USER: '' }), { PATH: '/usr/bin', HOME: '/h' });
});

test('generated environments: no key outside the six ever reaches the child, and no GIT_* at all', () => {
  const rng = seeded(0x617e07);
  const names = ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_CONFIG', 'GIT_CONFIG_COUNT', 'GIT_CONFIG_KEY_0', 'GIT_SSH_COMMAND', 'GIT_PAGER', 'git_dir', 'HOME', 'USER', 'PATH', 'LANG', 'LC_ALL', 'TMPDIR', 'SHELL', 'PAGER', 'EDITOR', 'NODE_OPTIONS'];
  const allowed = new Set(['PATH', ...ENV_KEYS]);
  for (let i = 0; i < 1000; i++) {
    const source: Record<string, string> = {};
    for (let k = 0; k < 8; k++) source[pick(rng, names) + (rng() < 0.3 ? hostileString(rng, 3, ['_', 'X', '0']) : '')] = hostileString(rng, 6);
    for (const key of Object.keys(childEnv(['/usr/bin'], source))) {
      assert.ok(allowed.has(key), key);
      assert.ok(!key.toUpperCase().startsWith('GIT_'), key);
    }
  }
});
