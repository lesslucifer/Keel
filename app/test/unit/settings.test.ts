import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseSettings, defaultSettings, SETTINGS_FILE } from '../../src/main/settings.ts';

test('settings live in settings.json and start with no tool set', () => {
  assert.equal(SETTINGS_FILE, 'settings.json');
  assert.deepEqual(defaultSettings(), { version: 1, tools: {} });
});

test('settings parse: version 1 with any of the three tools, each an absolute path named for its tool', () => {
  assert.deepEqual(parseSettings({ version: 1, tools: {} }), { version: 1, tools: {} });
  const full = { version: 1, tools: { keel: '/Users/me/.cargo/bin/keel', git: '/opt/homebrew/bin/git', claude: '/Users/me/.local/bin/claude' } };
  assert.deepEqual(parseSettings(full), full);
});

test('settings parse refuses a half file, an unknown version, extra keys, wrong types and a tool path that is not its tool', () => {
  const bad: unknown[] = [
    null, [], 'x', 1, {}, { version: 2, tools: {} }, { version: '1', tools: {} }, { version: 1 }, { tools: {} }, { version: 1, tools: {}, extra: 1 },
    { version: 1, tools: { node: '/usr/bin/node' } }, { version: 1, tools: { git: 'git' } }, { version: 1, tools: { git: '/usr/bin/sh' } },
    { version: 1, tools: { git: 1 } }, { version: 1, tools: [] }, { version: 1, tools: null }, { version: 1, tools: { git: '/usr/bin/git\n' } },
    { version: 1, tools: { __proto__: { git: '/usr/bin/git' } } }, JSON.parse('{"version": 1, "tools": {"__proto__": "/x/__proto__"}}'),
  ];
  for (const v of bad) assert.equal(parseSettings(v), null, JSON.stringify(v));
});
