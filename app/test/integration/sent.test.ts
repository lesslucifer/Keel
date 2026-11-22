// Mark sent at its real boundary: the marks in a temp app folder, written atomically and owner-only, read
// back by a new app launch; the list drawn by the view reader from real git and the real router-app.sh.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { statSync, writeFileSync, readdirSync, existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { SentMarks, SENT_FILE } from '../../src/main/items/sent.ts';
import { ViewReader } from '../../src/main/reader/view.ts';
import type { Item } from '../../src/shared/contract.ts';
import { realGit, sh, tempDir, mk } from '../support/fs.ts';
import { routerProject, routerApp } from '../support/router.ts';

const GIT = realGit();

test('marks are kept in the app folder, owner-only, and a new launch reads them back; a broken file is set aside', async (t) => {
  const appDir = path.join(tempDir(t, 'sent'), 'Keel');
  const first = new SentMarks(appDir);
  assert.deepEqual([...(await first.get('p0a1b2c3d'))], []);
  await first.set('p0a1b2c3d', 'plan/E-A.W1@abcdef0', true, new Set(['plan/E-A.W1@abcdef0']));
  assert.equal(statSync(path.join(appDir, SENT_FILE)).mode & 0o777, 0o600);
  assert.deepEqual([...(await new SentMarks(appDir).get('p0a1b2c3d'))], ['plan/E-A.W1@abcdef0']);
  writeFileSync(path.join(appDir, SENT_FILE), '{ half');
  assert.deepEqual([...(await new SentMarks(appDir).get('p0a1b2c3d'))], []);
  assert.ok(readdirSync(appDir).some((n) => n.startsWith(`${SENT_FILE}.broken-`)));
});

test('a plan marked sent sits under Sent across an app restart, and leaves the list when the router moves its stage on', async (t) => {
  const dir = routerProject(t, 'sent-flow');
  const appDir = path.join(mk(path.join(tempDir(t, 'sent-app'), 'x')), 'Keel');
  sh(GIT, dir, ['commit', '-q', '--allow-empty', '-m', 'init']);
  sh(GIT, dir, ['branch', '-M', 'main']);
  sh(GIT, dir, ['branch', 'epic/E-A']);
  sh(GIT, dir, ['branch', 'wu/E-A/w1-reader', 'epic/E-A']);
  const tip = sh(GIT, dir, ['rev-parse', 'wu/E-A/w1-reader']).trim().slice(0, 7);
  routerApp(dir, 'start', 'keel-1c');
  routerApp(dir, 'stage', 'E-A.W1', 'plan-to-read', tip);
  const launch = () => {
    const marks = new SentMarks(appDir);
    const reader = new ViewReader({
      project: async () => ({ id: 'p0a1b2c3d', name: 'proj', path: dir }),
      git: async () => GIT,
      runOptions: { path: [path.dirname(GIT), '/usr/bin', '/bin'], source: { HOME: dir, LANG: 'C' } },
      sent: (project) => marks.get(project),
    });
    return { marks, reader };
  };
  const plan = async (r: ViewReader): Promise<Item | undefined> => {
    await r.read();
    return r.view().items.find((i) => i.kind === 'plan');
  };
  const one = launch();
  const listed = (await plan(one.reader)) as Item;
  assert.equal(listed.group, 'yours');
  await one.marks.set('p0a1b2c3d', listed.id, true, new Set(one.reader.view().items.filter((i) => i.markable).map((i) => i.id)));
  assert.equal((await plan(one.reader))?.group, 'sent');
  assert.deepEqual(one.reader.view().counts, { yours: 0, notYours: 0, sent: 1 });
  const two = launch();
  assert.equal((await plan(two.reader))?.group, 'sent', 'a new launch reads the mark back');
  await two.marks.set('p0a1b2c3d', listed.id, false, new Set([listed.id]));
  assert.equal((await plan(two.reader))?.group, 'yours', 'Not sent');
  await two.marks.set('p0a1b2c3d', listed.id, true, new Set([listed.id]));
  routerApp(dir, 'stage', 'E-A.W1', 'plan-approved', tip);
  assert.equal(await plan(two.reader), undefined, 'the stage moved on: the item left, and its mark with it');
  assert.equal(existsSync(path.join(dir, SENT_FILE)), false);
  assert.match(readFileSync(path.join(appDir, SENT_FILE), 'utf8'), /"version": 1/);
});
