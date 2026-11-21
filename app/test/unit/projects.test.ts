import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseProjects, mintId, MAX_PROJECTS, PROJECTS_FILE, defaultProjects, commitsConfig } from '../../src/main/projects.ts';
import { seeded } from '../support/seeded.ts';

test('the project list lives in projects.json and starts empty', () => {
  assert.equal(PROJECTS_FILE, 'projects.json');
  assert.deepEqual(defaultProjects(), { version: 1, projects: [] });
});

test('project list parse: version 1, each project an id, an absolute path and a plain name', () => {
  const one = { version: 1, projects: [{ id: 'p0123abcd', path: '/Users/me/Keel', name: 'Keel' }] };
  assert.deepEqual(parseProjects(one), one);
});

test('project list parse refuses a half list: unknown version, extra keys, a bad id, a relative path, a name with a control, repeats', () => {
  const p = { id: 'p0123abcd', path: '/Users/me/Keel', name: 'Keel' };
  const bad: unknown[] = [
    null, [], { version: 2, projects: [] }, { version: 1 }, { version: 1, projects: [], x: 1 }, { version: 1, projects: {} },
    { version: 1, projects: [{ ...p, extra: 1 }] }, { version: 1, projects: [{ ...p, id: 'P0123ABCD' }] }, { version: 1, projects: [{ ...p, path: 'Keel' }] },
    { version: 1, projects: [{ ...p, path: '/a\nb' }] }, { version: 1, projects: [{ ...p, name: 'a\u0007b' }] }, { version: 1, projects: [{ ...p, name: 7 }] },
    { version: 1, projects: [p, { ...p, path: '/x' }] }, { version: 1, projects: [p, { ...p, id: 'p00000000' }] },
    { version: 1, projects: [{ id: 'p0123abcd', path: '/Users/me/Keel' }] }, { version: 1, projects: [null] },
  ];
  for (const v of bad) assert.equal(parseProjects(v), null, JSON.stringify(v));
});

test('E-APPLIST keeps one project', () => {
  assert.equal(MAX_PROJECTS, 1);
});

test('a minted id is p and 8 hex digits, never one already taken', () => {
  const rng = seeded(0x1d07);
  const bytes = (n: number) => Buffer.from(Array.from({ length: n }, () => Math.floor(rng() * 256)));
  const taken: string[] = [];
  for (let i = 0; i < 200; i++) {
    const id = mintId(taken, bytes);
    assert.match(id, /^p[0-9a-f]{8}$/);
    assert.ok(!taken.includes(id));
    taken.push(id);
  }
  let calls = 0;
  const stuck = (n: number) => (calls++ < 3 ? Buffer.alloc(n, 0) : Buffer.alloc(n, 1));
  assert.equal(mintId(['p00000000'], stuck), 'p01010101');
});

test('HEAD commits .keel/config.toml only as a regular file: ls-tree shows mode 100644 or 100755 and a blob, and nothing else', () => {
  const sha1 = 'a'.repeat(40);
  const sha256 = 'b'.repeat(64);
  assert.equal(commitsConfig(`100644 blob ${sha1}\t.keel/config.toml\n`), true);
  assert.equal(commitsConfig(`100755 blob ${sha256}\t.keel/config.toml\n`), true);
  for (const out of [
    '',
    `040000 tree ${sha1}\t.keel/config.toml\n`,
    `120000 blob ${sha1}\t.keel/config.toml\n`,
    `160000 commit ${sha1}\t.keel/config.toml\n`,
    `100644 blob ${sha1}\t.keel/config.tom\n`,
    `100644 blob ${sha1}\t.keel/config.toml`,
    `100644 blob ${sha1}\t.keel/config.toml\n100644 blob ${sha1}\t.keel/config.toml\n`,
    `100644 blob ${'a'.repeat(39)}\t.keel/config.toml\n`,
    `100644 tree ${sha1}\t.keel/config.toml\n`,
  ]) {
    assert.equal(commitsConfig(out), false, JSON.stringify(out));
  }
});
