import { test } from 'node:test';
import assert from 'node:assert/strict';
import { toolShapeOk, inside, PATH_CAP } from '../../src/main/tools.ts';
import type { Tool } from '../../src/shared/contract.ts';

test('a tool path\'s shape: absolute, under the cap, no control character, its last part the tool\'s name', () => {
  assert.equal(PATH_CAP, 1024);
  assert.ok(toolShapeOk('git', '/usr/bin/git'));
  assert.ok(toolShapeOk('claude', '/Users/me/.local/bin/claude'));
  assert.ok(toolShapeOk('keel', '/Users/me/.cargo/bin/keel'));
  const bad: Array<[Tool, unknown]> = [
    ['git', 'git'], ['git', '/usr/bin/git/'], ['git', '/usr/bin/gitx'], ['keel', '/usr/bin/git'], ['git', '/usr/bin/\u0000git'], ['git', '/usr/bin\n/git'],
    ['git', ''], ['git', `/${'a'.repeat(PATH_CAP)}/git`], ['git', 7], ['git', null],
  ];
  for (const [tool, p] of bad) assert.ok(!toolShapeOk(tool, p), String(p).slice(0, 40));
});

test('inside(): a path is inside a folder only at or below it, never beside it with a shared prefix', () => {
  assert.ok(inside('/Users/me/Keel/bin/git', '/Users/me/Keel'));
  assert.ok(inside('/Users/me/Keel', '/Users/me/Keel'));
  assert.ok(!inside('/Users/me/Keel2/bin/git', '/Users/me/Keel'));
  assert.ok(!inside('/Users/me', '/Users/me/Keel'));
  assert.ok(!inside('/Users/me/..Keel/git', '/Users/me/Keel'));
});
