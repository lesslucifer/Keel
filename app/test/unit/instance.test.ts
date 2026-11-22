// A second launch (E-APPLIST.W9, backlog A3's follow-up from the W5 live check): one that loses the
// single-instance lock says so on stderr before it quits, instead of quitting with no word.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { SECOND_LAUNCH, secondLaunch } from '../../src/main/instance.ts';

const SHELL = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'src', 'main', 'index.ts');

test('a launch that loses the lock writes exactly one line, in these words, and nothing else', () => {
  assert.equal(SECOND_LAUNCH, 'Keel is already running: this launch quits; quit the other first');
  const written: string[] = [];
  secondLaunch((text) => written.push(text));
  assert.deepEqual(written, ['Keel is already running: this launch quits; quit the other first\n']);
});

test('the shell says it on stderr where the lock is refused, before it quits', () => {
  const shell = readFileSync(SHELL, 'utf8');
  const branch = /if \(!app\.requestSingleInstanceLock\(\)\) \{\n([^}]*)\}/.exec(shell)?.[1] ?? '';
  assert.deepEqual(
    branch.split('\n').map((l) => l.trim()).filter((l) => l !== ''),
    ['secondLaunch((text) => process.stderr.write(text));', 'app.quit();'],
  );
});
