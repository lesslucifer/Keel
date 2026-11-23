// Open's pure parts (src/main/open/docs.ts): the tree it copies, the files it refuses, a data file's
// head, the page a role and an id name, and the only URLs it hands the browser.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { parseTree, copyPlan, parseDocRoles, docRolesWhy, findPage, fileUrlInside, COPY_CAP, HEAD_CAP, roleOf, DOC_ROLES_SCHEMA } from '../../src/main/open/docs.ts';
import type { Captured } from '../../src/main/commands.ts';
import { OUTPUT_CAP } from '../../src/main/commands.ts';
import { seeded, hostileString } from '../support/seeded.ts';

const O = (c: string) => c.repeat(40);
const row = (mode: string, type: string, oid: string, size: string, p: string) => `${mode} ${type} ${oid} ${size.padStart(7)}\t${p}\0`;

test('the tree read gives each entry\'s mode, type, id, size and path; a submodule has no size; a row that does not parse is dropped', () => {
  const out = row('100644', 'blob', O('a'), '120', 'docs/work/x-plan.html') + row('100755', 'blob', O('b'), '7', 'docs/run.sh') + row('160000', 'commit', O('c'), '-', 'docs/sub') + row('120000', 'blob', O('d'), '11', 'docs/link') + 'garbage\0';
  assert.deepEqual(parseTree(out), [
    { mode: '100644', type: 'blob', oid: O('a'), size: 120, path: 'docs/work/x-plan.html' },
    { mode: '100755', type: 'blob', oid: O('b'), size: 7, path: 'docs/run.sh' },
    { mode: '160000', type: 'commit', oid: O('c'), size: null, path: 'docs/sub' },
    { mode: '120000', type: 'blob', oid: O('d'), size: 11, path: 'docs/link' },
  ]);
  assert.deepEqual(parseTree(''), []);
});

test('the copy takes regular files under docs/ only: no link, no submodule, no path that climbs or has a control character, none over the output cap, all within COPY_CAP', () => {
  const e = (p: string, size = 10, mode = '100644', type = 'blob') => ({ mode, type, oid: O('a'), size, path: p });
  const plan = copyPlan([
    e('docs/work/x-plan.html'),
    e('docs/run.sh', 10, '100755'),
    e('docs/link', 10, '120000'),
    e('docs/sub', 0, '160000', 'commit'),
    e('docs/../etc/passwd'),
    e('docs/./x.html'),
    e('docs//x.html'),
    e('/docs/x.html'),
    e('src/main.ts'),
    e('docs/a\u0007b.html'),
    e('docs/big.bin', OUTPUT_CAP + 1),
  ]);
  assert.deepEqual(plan.files.map((f) => f.path), ['docs/run.sh', 'docs/work/x-plan.html']);
  assert.equal(plan.skipped.length, 9);
  assert.ok(COPY_CAP >= 8 * 1024 * 1024);
});

test('W6 review round 1 (docs.ts:52): the copy order puts the files named first (the data file, then the page) before the rest, the rest by path; the cap is charged on what is copied, not here', () => {
  const e = (p: string, size = 10) => ({ mode: '100644', type: 'blob', oid: O('a'), size, path: p });
  const many = [e('docs/a/1.html', OUTPUT_CAP), e('docs/a/2.html', OUTPUT_CAP), e('docs/work/p.html'), e('docs/work/p.yaml'), e('docs/b.html')];
  assert.deepEqual(copyPlan(many, ['docs/work/p.yaml', 'docs/work/p.html']).files.map((f) => f.path), ['docs/work/p.yaml', 'docs/work/p.html', 'docs/a/1.html', 'docs/a/2.html', 'docs/b.html']);
  assert.deepEqual(copyPlan(many, ['docs/none.html']).files.length, 5, 'a first name not in the tree changes nothing');
});

test('generated paths: no path the copy keeps can leave its folder', () => {
  const rng = seeded(0xd0c5);
  const alphabet = ['docs/', 'a', '.', '..', '/', '\\', '%2e', '-', ' ', '\u0000', 'é'];
  for (let i = 0; i < 2000; i++) {
    const p = rng() < 0.5 ? `docs/${hostileString(rng, 20, alphabet)}` : hostileString(rng, 24, alphabet);
    const kept = copyPlan([{ mode: '100644', type: 'blob', oid: O('a'), size: 1, path: p }]).files;
    if (kept.length === 0) continue;
    const root = '/tmp/root';
    const target = path.resolve(root, p);
    assert.ok(target.startsWith(`${root}/docs/`), JSON.stringify(p));
  }
});

test('W12 (owner ruling 2026-11-23): keel doc-roles\' answer gives each asked file its roles, id and page; any other form is no answer', () => {
  const asked = ['/a/x.yaml', '/a/y.yaml'];
  const entry = (p: string, roles: unknown, id: unknown = null, page: unknown = null) => ({ path: p, roles, id, page, problem: null });
  const answer = (files: unknown[], schema: unknown = DOC_ROLES_SCHEMA) => JSON.stringify({ schema, files });
  const ok = parseDocRoles(answer([entry('/a/x.yaml', ['wu-plan'], 'E-A.W1', 'x.html'), { ...entry('/a/y.yaml', []), problem: 'no field "artifact"' }]), asked);
  assert.deepEqual(ok && [...ok], [
    ['/a/x.yaml', { artifact: ['wu-plan'], id: 'E-A.W1', page: 'x.html' }],
    ['/a/y.yaml', { artifact: [], id: null, page: null }],
  ]);
  const both = [entry('/a/x.yaml', []), entry('/a/y.yaml', [])];
  for (const bad of [
    'not json',
    '[]',
    answer(both, 'doc-roles/v2'),
    answer([entry('/a/x.yaml', [])]),
    answer([entry('/a/y.yaml', []), entry('/a/x.yaml', [])]),
    answer([entry('/a/x.yaml', [3]), entry('/a/y.yaml', [])]),
    answer([entry('/a/x.yaml', 'wu-plan'), entry('/a/y.yaml', [])]),
    answer([entry('/a/x.yaml', [], 7), entry('/a/y.yaml', [])]),
    answer([entry('/a/x.yaml', [], null, 'a\u0000.html'), entry('/a/y.yaml', [])]),
    answer([null, entry('/a/y.yaml', [])]),
  ]) assert.equal(parseDocRoles(bad, asked), null, bad);
  assert.ok(HEAD_CAP >= 16 * 1024);
});

test('W12 (owner ruling 2026-11-23): why keel gave no answer: it did not start, did not answer, was cut, or is a keel with no doc-roles (update keel)', () => {
  const r = (over: Partial<Captured>): Captured => ({ code: 0, stdout: '', stderr: '', timedOut: false, startError: null, cut: false, ...over });
  assert.equal(docRolesWhy(r({})), null);
  assert.equal(docRolesWhy(r({ code: null, startError: 'ENOENT' })), 'keel could not start (ENOENT)');
  assert.match(docRolesWhy(r({ code: null, timedOut: true })) ?? '', /^keel did not answer/);
  assert.match(docRolesWhy(r({ cut: true })) ?? '', /cut/);
  assert.match(docRolesWhy(r({ code: 1, stderr: 'usage: keel <init [-f] | lint ...>' })) ?? '', /update keel$/);
});

test('the page a role and an id name: the page line of the data file whose head matches, beside it, a plain .html name; the newest by path when several match', () => {
  const heads = [
    { path: 'docs/work/2026-11-20-e-a-w1-plan.yaml', head: { artifact: ['wu-plan'], id: 'E-A.W1', page: '2026-11-20-e-a-w1-plan.html' } },
    { path: 'docs/work/2026-11-21-e-a-w1-result.yaml', head: { artifact: ['wu-result'], id: 'E-A.W1', page: '2026-11-21-e-a-w1-result.html' } },
    { path: 'docs/work/2026-11-22-e-a-w1-plan.yaml', head: { artifact: ['wu-plan'], id: 'E-A.W1', page: '2026-11-22-e-a-w1-plan.html' } },
    { path: 'docs/work/evil.yaml', head: { artifact: ['wu-plan'], id: 'E-A.W2', page: '../../etc/passwd.html' } },
    { path: 'docs/work/odd.yaml', head: { artifact: ['wu-plan'], id: 'E-A.W3', page: 'x.yaml' } },
  ];
  assert.equal(findPage(heads, 'wu-plan', 'E-A.W1'), 'docs/work/2026-11-22-e-a-w1-plan.html');
  assert.equal(findPage(heads, 'wu-result', 'E-A.W1'), 'docs/work/2026-11-21-e-a-w1-result.html');
  assert.equal(findPage(heads, 'wu-plan', 'E-A.W2'), null, 'a page that climbs is refused');
  assert.equal(findPage(heads, 'wu-plan', 'E-A.W3'), null, 'a page that is not .html is refused');
  assert.equal(findPage(heads, 'epic-result', 'E-A'), null);
});

test('each kind opens its role: a plan the WU plan, a result the WU result, an epic result the epic result; others none', () => {
  assert.equal(roleOf('plan'), 'wu-plan');
  assert.equal(roleOf('result'), 'wu-result');
  assert.equal(roleOf('epic-result'), 'epic-result');
  for (const k of ['question', 'worker', 'task', 'push', 'motion'] as const) assert.equal(roleOf(k), null);
});

test('the only URL handed to the browser is a file URL of a file inside an allowed folder', () => {
  assert.equal(fileUrlInside('/Users/me/Library/Keel/open/p1/abc/docs/x.html', ['/Users/me/Library/Keel/open']), 'file:///Users/me/Library/Keel/open/p1/abc/docs/x.html');
  assert.equal(fileUrlInside('/Users/me/code/keel/docs/.archive/r.html', ['/Users/me/code/keel/docs/.archive']), 'file:///Users/me/code/keel/docs/.archive/r.html');
  assert.equal(fileUrlInside('/Users/me/a b/#x?.html', ['/Users/me/a b']), 'file:///Users/me/a%20b/%23x%3F.html');
  for (const p of ['/Users/me/Library/Keel/open/../secrets.html', '/Users/me/Library/Keel/openx/y.html', 'relative/x.html', '/Users/me/Library/Keel/open']) {
    assert.equal(fileUrlInside(p, ['/Users/me/Library/Keel/open']), null, p);
  }
});
