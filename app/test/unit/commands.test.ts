import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LOCKED, LOCKED_BEFORE_2_45, MIN_GIT, NO_EXTERNAL, ENV_KEYS, READ_TIMEOUT_MS, COMMANDS, argv, lock, childEnv, refusedFlag } from '../../src/main/commands.ts';
import { seeded, hostileString, pick } from '../support/seeded.ts';

/** Good slots for each fixed command: the project, and W5's note kind and branch ref where a command takes them. */
const SLOTS: Record<string, Record<string, string>> = {
  toplevel: { project: '/p' },
  'common-dir': { project: '/p' },
  'has-config': { project: '/p' },
  'promisor-config': { project: '/p' },
  refs: { project: '/p' },
  'note-of': { project: '/p', kind: 'mr', object: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' },
  'main-name': { project: '/p', object: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' },
  merged: { project: '/p', ref: 'refs/heads/epic/E-APPLIST' },
  worktrees: { project: '/p' },
};

test('every fixed command starts with -c core.fsmonitor=false --no-optional-locks --no-lazy-fetch', () => {
  // --no-lazy-fetch: in a partial clone a read never fetches a missing object into the project's .git.
  assert.deepEqual(LOCKED, ['-c', 'core.fsmonitor=false', '--no-optional-locks', '--no-lazy-fetch']);
  for (const name of Object.keys(COMMANDS)) assert.deepEqual(argv(name, SLOTS[name] as Record<string, string>).slice(0, 4), LOCKED, name);
});

test('the fixed commands of W7 are exactly the plan\'s three lines and, for a git older than 2.45, the promisor read; W5 adds four reads', () => {
  assert.deepEqual(Object.keys(COMMANDS).sort(), ['common-dir', 'has-config', 'main-name', 'merged', 'note-of', 'promisor-config', 'refs', 'toplevel', 'worktrees']);
  assert.deepEqual(argv('promisor-config', { project: '/Users/me/Keel' }, true), [
    '-c', 'core.fsmonitor=false', '--no-optional-locks', '-C', '/Users/me/Keel', 'config', '--get-regexp', '^(extensions\\.partialclone|remote\\..*\\.(promisor|partialclonefilter))$',
  ]);
  assert.deepEqual(argv('toplevel', { project: '/Users/me/Keel' }), [...LOCKED, '-C', '/Users/me/Keel', 'rev-parse', '--show-toplevel']);
  // No --path-format (git 2.31): the app resolves a relative answer against the project itself.
  assert.deepEqual(argv('common-dir', { project: '/Users/me/Keel' }), [...LOCKED, '-C', '/Users/me/Keel', 'rev-parse', '--git-common-dir']);
  assert.deepEqual(argv('has-config', { project: '/Users/me/Keel' }), [...LOCKED, '-C', '/Users/me/Keel', 'ls-tree', '--full-tree', 'HEAD', '--', '.keel/config.toml']);
});

test('a read times out after 10 s', () => {
  assert.equal(READ_TIMEOUT_MS, 10_000);
});

test('a project slot is one absolute path, passed as one argument whatever shell characters it holds', () => {
  const p = '/Users/me/a b/$(touch pwned); x`id`|&>*';
  assert.equal(argv('toplevel', { project: p })[LOCKED.length + 1], p);
  assert.equal(argv('toplevel', { project: p }).length, LOCKED.length + 4);
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

test('the child environment holds only PATH (the login PATH), LC_ALL=C, and HOME, USER, LOGNAME, LANG, TMPDIR from the app', () => {
  assert.deepEqual(ENV_KEYS, ['HOME', 'USER', 'LOGNAME', 'LANG', 'TMPDIR']);
  const env = childEnv(['/opt/homebrew/bin', '/usr/bin'], {
    HOME: '/Users/me', USER: 'me', LOGNAME: 'me', LANG: 'en_US.UTF-8', TMPDIR: '/tmp/x', PATH: '/evil',
    GIT_DIR: '/x', GIT_CONFIG_PARAMETERS: "'core.pager'='sh'", GIT_EXTERNAL_DIFF: '/x', NODE_OPTIONS: '--require /x', DYLD_INSERT_LIBRARIES: '/x', ELECTRON_RUN_AS_NODE: '1',
  });
  assert.deepEqual(env, { PATH: '/opt/homebrew/bin:/usr/bin', LC_ALL: 'C', HOME: '/Users/me', USER: 'me', LOGNAME: 'me', LANG: 'en_US.UTF-8', TMPDIR: '/tmp/x' });
  assert.equal(Object.getPrototypeOf(env), Object.prototype);
});

test('the child environment skips an unset or empty key rather than passing it empty', () => {
  assert.deepEqual(childEnv(['/usr/bin'], { HOME: '/h', USER: '', LC_ALL: 'de_DE.UTF-8', LANGUAGE: 'de' }), { PATH: '/usr/bin', LC_ALL: 'C', HOME: '/h' });
});

test('generated environments: no key outside the seven ever reaches the child, and no GIT_* at all', () => {
  const rng = seeded(0x617e07);
  const names = ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_CONFIG', 'GIT_CONFIG_COUNT', 'GIT_CONFIG_KEY_0', 'GIT_SSH_COMMAND', 'GIT_PAGER', 'git_dir', 'HOME', 'USER', 'PATH', 'LANG', 'LC_ALL', 'TMPDIR', 'SHELL', 'PAGER', 'EDITOR', 'NODE_OPTIONS'];
  const allowed = new Set(['PATH', 'LC_ALL', ...ENV_KEYS]);
  for (let i = 0; i < 1000; i++) {
    const source: Record<string, string> = {};
    for (let k = 0; k < 8; k++) source[pick(rng, names) + (rng() < 0.3 ? hostileString(rng, 3, ['_', 'X', '0']) : '')] = hostileString(rng, 6);
    for (const key of Object.keys(childEnv(['/usr/bin'], source))) {
      assert.ok(allowed.has(key), key);
      assert.ok(!key.toUpperCase().startsWith('GIT_'), key);
    }
  }
});

test('a git older than 2.45 runs the same lines without --no-lazy-fetch, and only that flag is dropped', () => {
  assert.equal(MIN_GIT, '2.45');
  assert.deepEqual(LOCKED_BEFORE_2_45, ['-c', 'core.fsmonitor=false', '--no-optional-locks']);
  for (const name of Object.keys(COMMANDS)) {
    const slots = SLOTS[name] as Record<string, string>;
    assert.deepEqual(argv(name, slots, true), argv(name, slots).filter((a) => a !== '--no-lazy-fetch'), name);
  }
  assert.deepEqual(lock(['show', 'HEAD'], true), [...LOCKED_BEFORE_2_45, 'show', ...NO_EXTERNAL, 'HEAD']);
});

test('the locked flag an old git refused is read from exit 129 and the flag named in stderr, in any language or case', () => {
  const usage = 'usage: git [--version] [--help] [-C <path>] [-c <name>=<value>]\n';
  const r = (code: number | null, stderr: string) => ({ code, stderr, stdout: '', timedOut: false, startError: null, cut: false });
  for (const line of ['unknown option: --no-lazy-fetch', 'Unknown option: --no-lazy-fetch', 'Unbekannte Option: --no-lazy-fetch', 'option inconnue : --no-lazy-fetch']) {
    assert.equal(refusedFlag(r(129, `${line}\n${usage}`)), '--no-lazy-fetch', line);
  }
  assert.equal(refusedFlag(r(129, `Unknown option: --no-optional-locks\n${usage}`)), '--no-optional-locks');
  assert.equal(refusedFlag(r(128, 'unknown option: --no-lazy-fetch\n')), null);
  assert.equal(refusedFlag(r(null, 'unknown option: --no-lazy-fetch\n')), null);
  assert.equal(refusedFlag(r(129, 'error: unknown option: --no-lazy-fetchy\n')), null);
  assert.equal(refusedFlag(r(129, `unknown option: --frobnicate\n${usage}`)), null);
});

test('W5\'s four reads are exactly these lines: the branches and notes refs, one tip\'s note of one kind, what a branch holds, the worktrees', () => {
  const P = '/Users/me/Keel';
  assert.deepEqual(argv('refs', { project: P }), [...LOCKED, '-C', P, 'for-each-ref', '--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track,nobracket)', 'refs/heads/', 'refs/notes/']);
  assert.deepEqual(argv('note-of', { project: P, kind: 'cr', object: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }), [...LOCKED, '-C', P, 'notes', '--ref=refs/notes/cr', 'list', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa']);
  assert.deepEqual(argv('merged', { project: P, ref: 'refs/heads/wu/E-APPLIST/w5-reader-watcher' }), [...LOCKED, '-C', P, 'for-each-ref', '--merged=refs/heads/wu/E-APPLIST/w5-reader-watcher', '--format=%(refname)', 'refs/heads/']);
  assert.deepEqual(argv('worktrees', { project: P }), [...LOCKED, '-C', P, 'worktree', 'list', '--porcelain']);
  assert.deepEqual(argv('main-name', { project: P, object: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }), [...LOCKED, '-C', P, 'name-rev', '--name-only', '--no-undefined', '--refs=refs/heads/main', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa']);
});

test('a note kind is one of mr, cr, dod; a ref is main, an epic or a work unit branch by keel\'s naming, nothing else', () => {
  const O = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
  for (const kind of ['mr', 'cr', 'dod']) assert.doesNotThrow(() => argv('note-of', { project: '/p', kind, object: O }), kind);
  for (const kind of ['test', 'doc', '', 'MR', '../mr', 'mr list', '-m']) assert.throws(() => argv('note-of', { project: '/p', kind, object: O }), /kind/, kind);
  for (const object of ['b'.repeat(64), O]) assert.doesNotThrow(() => argv('note-of', { project: '/p', kind: 'mr', object }), object);
  for (const object of ['HEAD', 'main', '-p', O.slice(1), O.toUpperCase(), `${O} x`, '', 'g'.repeat(40)]) assert.throws(() => argv('note-of', { project: '/p', kind: 'mr', object }), /object/, object);
  for (const ref of ['refs/heads/main', 'refs/heads/epic/E-APPLIST', 'refs/heads/wu/E-APPLIST/w5-reader-watcher', 'refs/heads/wu/E-APPLIST/w8-window-foundation_look-variants']) {
    assert.doesNotThrow(() => argv('merged', { project: '/p', ref }), ref);
  }
  const bad = ['main', 'refs/heads/', 'refs/heads/topic', 'refs/heads/epic/', 'refs/heads/epic/a/b', 'refs/heads/wu/E-X', 'refs/heads/wu/E-X/w1/x', 'refs/remotes/origin/main', 'refs/heads/epic/E..X', 'refs/heads/epic/E-X.lock', 'refs/heads/epic/-x', 'refs/heads/epic/E X', 'refs/heads/epic/E@{1}', '--all', 'refs/heads/epic/E\nX', `refs/heads/epic/${'x'.repeat(101)}`];
  for (const ref of bad) assert.throws(() => argv('merged', { project: '/p', ref }), /ref/, JSON.stringify(ref));
});

test('generated refs: every one the slot takes is a whole keel branch ref with no dash-led, dotted or locked part', () => {
  const rng = seeded(0x2ef5);
  // Mostly name characters, so many refs pass and the property has something to judge; then every character a ref must not hold.
  const alphabet = [...'abcZ019'.repeat(4), '-', '_', '.', '.', '/', '@', '{', ' ', ':', '~', '^', '\\', '*', '?', '['];
  let accepted = 0;
  for (let i = 0; i < 3000; i++) {
    const ref = pick(rng, ['refs/heads/epic/', 'refs/heads/wu/', 'refs/heads/wu/E-A/', 'refs/heads/', '']) + hostileString(rng, 14, alphabet);
    let ok = true;
    try {
      argv('merged', { project: '/p', ref });
    } catch {
      ok = false;
    }
    if (ok) {
      accepted++;
      assert.match(ref, /^refs\/heads\/(main|epic\/[A-Za-z0-9][A-Za-z0-9._-]*|wu\/[A-Za-z0-9][A-Za-z0-9._-]*\/[A-Za-z0-9][A-Za-z0-9._-]*)$/, ref);
      assert.ok(!ref.includes('..') && !/\.lock(\/|$)/.test(ref) && !/\.(\/|$)/.test(ref), ref);
    }
  }
  assert.ok(accepted > 50, `only ${accepted} generated refs accepted: the property is too thin`);
});
