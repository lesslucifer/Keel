// Open's pure parts (src/main/open/docs.ts): the tree it copies, the files it refuses, a data file's
// head, the page a role and an id name, and the only URLs it hands the browser.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { parseTree, copyPlan, dataHead, findPage, fileUrlInside, COPY_CAP, HEAD_CAP, roleOf } from '../../src/main/open/docs.ts';
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
  assert.deepEqual(plan.files.map((f) => f.path), ['docs/work/x-plan.html', 'docs/run.sh']);
  assert.equal(plan.skipped.length, 9);
  const many = Array.from({ length: 20 }, (_, i) => e(`docs/f${i}.html`, Math.floor(COPY_CAP / 10)));
  const capped = copyPlan(many);
  assert.ok(capped.files.reduce((n, f) => n + (f.size ?? 0), 0) <= COPY_CAP);
  assert.equal(capped.files.length + capped.skipped.length, 20);
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

test('a data file\'s head: its top-level artifact (one or a list), id and page; quotes taken off; anchors, aliases and nested keys are not read', () => {
  assert.deepEqual(dataHead('artifact: wu-plan\nstatus: ready\nid: E-APPLIST.W6\nepic: E-APPLIST\npage: 2026-11-22-e-applist-w6-plan.html\n'), { artifact: ['wu-plan'], id: 'E-APPLIST.W6', page: '2026-11-22-e-applist-w6-plan.html' });
  assert.deepEqual(dataHead('artifact: [epic-result, wu-result]\nid: "E-A"\npage: \'x-result.html\'\n'), { artifact: ['epic-result', 'wu-result'], id: 'E-A', page: 'x-result.html' });
  assert.deepEqual(dataHead('details:\n  id: E-B.W1\n  page: y.html\nartifact: wu-result\n'), { artifact: ['wu-result'], id: null, page: null });
  assert.deepEqual(dataHead('artifact: &a wu-plan\nid: *b\npage: x.html\n'), { artifact: [], id: null, page: 'x.html' });
  assert.deepEqual(dataHead('artifact: wu-plan\r\nid: E-A.W1\r\npage: x.html\r\n'), { artifact: ['wu-plan'], id: 'E-A.W1', page: 'x.html' });
  assert.deepEqual(dataHead(''), { artifact: [], id: null, page: null });
  assert.ok(HEAD_CAP >= 16 * 1024);
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
