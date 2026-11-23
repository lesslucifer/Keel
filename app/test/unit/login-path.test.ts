import { test } from 'node:test';
import assert from 'node:assert/strict';
import { START, END, SCRIPT, SYSTEM_PATH, LOGIN_TIMEOUT_MS, between, entries, fallback, shellOf } from '../../src/main/login-path.ts';

test('the script prints the PATH between two markers and holds no value from the app', () => {
  assert.equal(SCRIPT, `printf '%s%s%s\\n' '${START}' "$PATH" '${END}'`);
  assert.notEqual(START, END);
  assert.match(START, /^[A-Z_]+$/);
  assert.match(END, /^[A-Z_]+$/);
});

test('the login shell gets 5 s', () => {
  assert.equal(LOGIN_TIMEOUT_MS, 5_000);
});

test('between() takes the text between the markers, ignoring profile noise around them', () => {
  assert.equal(between(`Last login: Mon\nwelcome\n${START}/a:/b${END}\nbye\n`), '/a:/b');
  assert.equal(between(`${START}${END}`), '');
  assert.equal(between(`noise ${START}/x${END} ${START}/y${END}`), '/x');
});

test('between() is null with no markers, one marker, or the end before the start', () => {
  assert.equal(between('PATH=/usr/bin'), null);
  assert.equal(between(`${START}/usr/bin`), null);
  assert.equal(between(`/usr/bin${END}`), null);
  assert.equal(between(`${END}/usr/bin${START}`), null);
});

test('entries() drops empty and relative entries and repeats, keeping the order', () => {
  assert.deepEqual(entries('/opt/homebrew/bin::bin:./x:/usr/bin:/opt/homebrew/bin:~/bin:/bin:'), ['/opt/homebrew/bin', '/usr/bin', '/bin']);
  assert.deepEqual(entries(''), []);
});

test('entries() drops an entry with a control character', () => {
  assert.deepEqual(entries('/a\nb:/usr/bin:/c\u0000:/d\u0085'), ['/usr/bin']);
});

test('the fallback is the system PATH plus the set tools\' folders, with its reason', () => {
  assert.deepEqual(SYSTEM_PATH, ['/usr/bin', '/bin', '/usr/sbin', '/sbin']);
  assert.deepEqual(fallback(['/Users/me/.cargo/bin', 'rel', '/usr/bin', '/Users/me/.local/bin'], 'timed out'), {
    entries: ['/usr/bin', '/bin', '/usr/sbin', '/sbin', '/Users/me/.cargo/bin', '/Users/me/.local/bin'],
    source: 'fallback',
    reason: 'timed out',
  });
});

test('shellOf() takes the account\'s shell when it is absolute, else /bin/zsh', () => {
  assert.equal(shellOf({ shell: '/opt/homebrew/bin/fish' }), '/opt/homebrew/bin/fish');
  assert.equal(shellOf({ shell: null }), '/bin/zsh');
  assert.equal(shellOf({ shell: '' }), '/bin/zsh');
  assert.equal(shellOf({ shell: 'zsh' }), '/bin/zsh');
});
