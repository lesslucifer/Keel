// Open's reading of a data file's roles at its real boundary (W12 review round 1, blockers docs.ts:172 and
// docs.ts:127; owner ruling 2026-11-23): the app asks keel (keel doc-roles) and keeps no YAML reader of its
// own. The real keel is the one cargo test builds (target/debug/keel; keel check runs cargo test first);
// fake keels stand in for the error paths: an older keel, one that does not start, hangs or answers garbage.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { docHeads } from '../../src/main/open/roles.ts';
import { DOC_ROLES_BATCH } from '../../src/main/commands.ts';
import { tempDir, program, mk, realGit, sh, HANG, keel, keelPath, KEEL, REPO } from '../support/fs.ts';

const opts = { path: ['/usr/bin', '/bin'], source: { HOME: '/nonexistent', LANG: 'C' } };

/** Write each text as its own .yaml file in a temp folder; their absolute paths, in order. */
function files(t: { after: (fn: () => void) => void }, texts: readonly string[]): string[] {
  const dir = tempDir(t, 'doc-roles');
  return texts.map((text, i) => {
    const f = path.join(dir, `${i}.yaml`);
    writeFileSync(f, text);
    return f;
  });
}

test('W12 review round 1 (docs.ts:172, docs.ts:127): every case of the shared table gives Open, through keel doc-roles, the roles keel\'s data_roles gives it', async (t) => {
  const table = JSON.parse(readFileSync(path.join(REPO, 'tests', 'data', 'data-roles.json'), 'utf8')) as { cases: Array<{ src: string; roles: string[]; why: string }> };
  assert.ok(table.cases.length >= 50);
  const paths = files(t, table.cases.map((c) => c.src));
  const heads = await docHeads(keel(), paths, opts);
  assert.ok(typeof heads !== 'string', String(heads));
  table.cases.forEach((c, i) => assert.deepEqual(heads.get(paths[i] as string)?.artifact, c.roles, `${JSON.stringify(c.src)} (${c.why})`));
});

test('W12 review round 1 (docs.ts:172): every docs/**/*.yaml data file in the repository\'s history gives Open the roles, id and page keel gives it alone, in batches past DOC_ROLES_BATCH; the four docs the old reader missed have their role', async (t) => {
  const GIT = realGit();
  const listed = sh(GIT, REPO, ['rev-list', '--all', '--objects'])
    .split('\n')
    .map((l) => l.split(' '))
    .filter((p): p is [string, string] => p.length === 2 && /^docs\/.*\.yaml$/.test(p[1] as string));
  const blobs = [...new Map(listed.map(([sha, p]) => [sha, p])).entries()];
  assert.ok(blobs.length > 0);
  const paths = files(t, blobs.map(([sha]) => sh(GIT, REPO, ['cat-file', 'blob', sha])));
  const heads = await docHeads(keel(), paths, opts);
  assert.ok(typeof heads !== 'string', String(heads));
  assert.equal(heads.size, paths.length);
  // Each file alone, straight from keel: batching and reading the answer change nothing.
  for (const [i, f] of paths.entries()) {
    const alone = (JSON.parse(execFileSync(keel(), ['doc-roles', f], { cwd: '/', encoding: 'utf8' })) as { files: Array<{ roles: string[]; id: string | null; page: string | null }> }).files[0];
    assert.deepEqual(heads.get(f), { artifact: alone?.roles, id: alone?.id, page: alone?.page }, (blobs[i] as [string, string])[1]);
  }
  assert.ok(paths.length > DOC_ROLES_BATCH, 'the history spans more than one batch');
  const missed = /(e-applist-w8-result|e-keelfix-w5-plan|e-keelfix-w5-result|e-keelfix-w3-plan)\.yaml$/;
  const named = blobs.map(([, p], i) => [p, paths[i] as string] as const).filter(([p]) => missed.test(p));
  assert.ok(named.length >= 4, `${named.length} versions of the four docs`);
  for (const [p, f] of named) assert.ok((heads.get(f)?.artifact.length ?? 0) > 0, `${p} has its role`);
});

test('W12 (owner ruling 2026-11-23): keel runs with the locked environment, from /, as keel doc-roles and the files only', async (t) => {
  const dir = tempDir(t, 'doc-roles-env');
  const rec = path.join(dir, 'rec.json');
  const keel = program(
    path.join(mk(path.join(dir, 'bin')), 'keel'),
    `#!${process.execPath}\nconst fs = require('node:fs');\nconst files = process.argv.slice(3);\nfs.writeFileSync(${JSON.stringify(rec)}, JSON.stringify({ argv: process.argv.slice(2), env: process.env, cwd: process.cwd() }));\nprocess.stdout.write(JSON.stringify({ schema: 'doc-roles/v1', files: files.map((p) => ({ path: p, roles: ['wu-plan'], id: 'E-A.W1', page: 'a.html', problem: null })) }));\n`,
  );
  const paths = files(t, ['x', 'y']);
  const heads = await docHeads(keel, paths, { ...opts, source: { HOME: dir, LANG: 'C', GIT_DIR: '/evil', NODE_OPTIONS: '--evil' } });
  assert.ok(typeof heads !== 'string');
  assert.deepEqual(heads.get(paths[0] as string), { artifact: ['wu-plan'], id: 'E-A.W1', page: 'a.html' });
  const seen = JSON.parse(readFileSync(rec, 'utf8')) as { argv: string[]; env: Record<string, string>; cwd: string };
  assert.deepEqual(seen.argv, ['doc-roles', ...paths]);
  assert.equal(seen.cwd, '/');
  assert.equal(seen.env.PATH, '/usr/bin:/bin');
  assert.equal(seen.env.LC_ALL, 'C');
  assert.equal(seen.env.GIT_DIR, undefined);
  assert.equal(seen.env.NODE_OPTIONS, undefined);
});

test('W12 (owner ruling 2026-11-23): an older keel, with no doc-roles, makes Open say to update keel', async (t) => {
  const dir = tempDir(t, 'doc-roles-old');
  const old = program(path.join(mk(path.join(dir, 'bin')), 'keel'), '#!/bin/sh\necho "usage: keel <init [-f] | lint | check | verify-doc | verify-dod | preflight | mr <setup|commit|confirm> | hooks install [--force] | notes write <test|doc|cr>>" >&2\nexit 1\n');
  const why = await docHeads(old, files(t, ['artifact: wu-plan\n']), opts);
  assert.equal(typeof why, 'string');
  assert.match(why as string, /update keel/);
});

test('W12: a keel that answers something that is not doc-roles/v1, does not start, or does not answer in time gives why, never roles', async (t) => {
  const dir = tempDir(t, 'doc-roles-bad');
  const bin = mk(path.join(dir, 'bin'));
  const paths = files(t, ['artifact: wu-plan\n']);
  const garbage = program(path.join(bin, 'keel'), '#!/bin/sh\necho "not json"\n');
  assert.match(String(await docHeads(garbage, paths, opts)), /update keel/);
  assert.match(String(await docHeads(path.join(dir, 'gone', 'keel'), paths, opts)), /keel could not start \(ENOENT\)/);
  const slow = program(path.join(mk(path.join(dir, 'slow')), 'keel'), `#!/bin/sh\n${HANG}\n`);
  assert.match(String(await docHeads(slow, paths, { ...opts, timeoutMs: 300 })), /keel did not answer/);
});

test('W12 review round 2 (docs.ts:83): a file whose id keel gives with a control character gives no roles; every other file in the batch keeps its own, and Open is never told to update keel', async (t) => {
  const paths = files(t, [
    'artifact: wu-plan\nstatus: draft\nid: "E-A\\tx"\nepic: E-A\npage: 0.html\n',
    'artifact: wu-result\nstatus: ready\nid: E-A.W1\nepic: E-A\npage: 1.html\n',
    'artifact: wu-plan\nstatus: draft\nid: E-A.W2\nepic: E-A\npage: "2\\u0001.html"\n',
  ]);
  const said = JSON.parse(execFileSync(keel(), ['doc-roles', ...paths], { cwd: '/', encoding: 'utf8' })) as { files: Array<{ id: string | null; page: string | null }> };
  assert.equal(said.files[0]?.id, 'E-A\tx', 'keel gives the id as written, with its tab');
  assert.equal(said.files[2]?.page, '2\u0001.html', 'keel gives the page as written, with its control character');
  const heads = await docHeads(keel(), paths, opts);
  assert.ok(typeof heads !== 'string', String(heads));
  assert.deepEqual([...heads], [[paths[1], { artifact: ['wu-result'], id: 'E-A.W1', page: '1.html' }]]);
});

test('W12 review round 2 (commands.ts:351): a data file whose path keel cannot be asked about (a control character in its name or folder) gives no roles and throws nothing; the others are asked', async (t) => {
  const dir = tempDir(t, 'doc-roles-ctl');
  const odd = mk(path.join(dir, 'a\u0007b'));
  const good = path.join(dir, 'good.yaml');
  writeFileSync(good, 'artifact: epic-result\nstatus: ready\nid: E-A\npage: good.html\n');
  const named = path.join(dir, 'x\ny.yaml');
  const inside = path.join(odd, 'z.yaml');
  for (const f of [named, inside]) writeFileSync(f, 'artifact: epic-result\nstatus: ready\nid: E-A\npage: z.html\n');
  const heads = await docHeads(keel(), [named, good, inside], opts);
  assert.ok(typeof heads !== 'string', String(heads));
  assert.deepEqual([...heads], [[good, { artifact: ['epic-result'], id: 'E-A', page: 'good.html' }]]);
  assert.deepEqual(await docHeads(keel(), [named, inside], opts), new Map(), 'none askable: no keel run, no roles, no throw');
});

test('W12 review round 2 (fs.ts:12): the keel these tests run is found by its decoded path and CARGO_TARGET_DIR, and is built here: a missing keel fails the keel-backed tests, never skips them', () => {
  assert.equal(keelPath('/Users/a b/café', {}), '/Users/a b/café/target/debug/keel');
  assert.equal(keelPath('/Users/a b/café', { CARGO_TARGET_DIR: '' }), '/Users/a b/café/target/debug/keel');
  assert.equal(keelPath('/Users/a b/café', { CARGO_TARGET_DIR: '/var/build/t' }), '/var/build/t/debug/keel');
  assert.equal(keelPath('/Users/a b/café', { CARGO_TARGET_DIR: 'out' }), '/Users/a b/café/out/debug/keel');
  assert.ok(existsSync(path.join(REPO, 'Cargo.toml')), `${REPO} is the repository's root`);
  assert.equal(KEEL, keelPath(REPO, process.env));
  assert.equal(keel(), KEEL, `keel is built at ${KEEL}`);
});
