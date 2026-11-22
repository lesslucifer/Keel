// The alerter at its real boundary: alert lines written by the real router-app.sh, read by the view
// reader, shown through a recording sink in place of macOS's alert, the record in a temp app folder.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, statSync, existsSync } from 'node:fs';
import path from 'node:path';
import { ViewReader } from '../../src/main/reader/view.ts';
import { Alerter, Focus, RECORD_FILE, type ShownAlert } from '../../src/main/alerter/alerts.ts';
import { openLink } from '../../src/main/alerter/link.ts';
import { fileSink } from '../../src/main/alerter/seams.ts';
import { realGit, tempDir } from '../support/fs.ts';
import { routerProject, routerApp } from '../support/router.ts';

const GIT = realGit();

/** One app launch: a view reader with an alerter on `appDir`, its alerts recorded, its clicks pressed by hand. */
function launch(dir: string | (() => string), appDir: string) {
  const root = typeof dir === 'string' ? () => dir : dir;
  const shown: Array<ShownAlert & { click: () => void }> = [];
  const focus = new Focus();
  const alerter = new Alerter({ appDir, sink: { show: (a, onClick) => shown.push({ ...a, click: onClick }) }, onClick: (wu) => focus.point(wu) });
  const reader = new ViewReader({
    project: async () => ({ id: 'p0a1b2c3d', name: 'proj', path: root() }),
    git: async () => GIT,
    runOptions: { path: [path.dirname(GIT), '/usr/bin', '/bin'], source: { HOME: root(), LANG: 'C' } },
    alerter,
    focus: () => focus.get(),
    scheme: () => 'Keel',
  });
  return { shown, focus, reader };
}

const alertLine = (dir: string, headline: string, wu?: string) =>
  routerApp(dir, 'alert', JSON.stringify({ headline, lines: ['a line'], ...(wu === undefined ? {} : { wu, link: `keel://wu/${wu}` }) }));

test('each alert line shows one alert, once; the view marks it shown; the record is the app folder\'s, owner-only', async (t) => {
  const dir = routerProject(t, 'alerter');
  const appDir = path.join(tempDir(t, 'alerter-app'), 'Keel');
  routerApp(dir, 'start', 'keel-1c');
  const app = launch(dir, appDir);
  await app.reader.read();
  assert.equal(app.shown.length, 0);
  alertLine(dir, 'Plan to read', 'E-A.W1');
  await app.reader.read();
  assert.deepEqual(app.shown.map(({ title, body, wu }) => ({ title, body, wu })), [{ title: 'Plan to read', body: 'a line', wu: 'E-A.W1' }]);
  alertLine(dir, 'Question');
  await app.reader.read();
  await app.reader.read();
  assert.deepEqual(app.shown.map((s) => s.title), ['Plan to read', 'Question']);
  assert.deepEqual(app.reader.view().alerts.map((a) => [a.headline, a.shown]), [['Plan to read', true], ['Question', true]]);
  assert.equal(statSync(path.join(appDir, RECORD_FILE)).mode & 0o777, 0o600);
  assert.equal(existsSync(path.join(dir, '.router', 'app', RECORD_FILE)), false, 'nothing written into the project');
});

test('an app restart shows no alert again; lines written while it was closed make one summary alert', async (t) => {
  const dir = routerProject(t, 'alerter-restart');
  const appDir = path.join(tempDir(t, 'alerter-app'), 'Keel');
  routerApp(dir, 'start', 'keel-1c');
  alertLine(dir, 'First', 'E-A.W1');
  const first = launch(dir, appDir);
  await first.reader.read();
  assert.equal(first.shown.length, 1);
  const again = launch(dir, appDir);
  await again.reader.read();
  assert.equal(again.shown.length, 0, 'no alert twice across a restart');
  alertLine(dir, 'While closed 1');
  alertLine(dir, 'While closed 2');
  alertLine(dir, 'While closed 3');
  const third = launch(dir, appDir);
  await third.reader.read();
  assert.deepEqual(third.shown.map(({ title, wu }) => ({ title, wu })), [{ title: '3 alerts while Keel was closed', wu: null }]);
  alertLine(dir, 'Live again', 'E-A.W2');
  await third.reader.read();
  assert.deepEqual(third.shown.map((s) => s.title), ['3 alerts while Keel was closed', 'Live again']);
});

test('a click on an alert points the view\'s focus at its work unit, and the view changes; a summary\'s click opens the list', async (t) => {
  const dir = routerProject(t, 'alerter-click');
  const appDir = path.join(tempDir(t, 'alerter-app'), 'Keel');
  routerApp(dir, 'start', 'keel-1c');
  const app = launch(dir, appDir);
  await app.reader.read();
  alertLine(dir, 'Result to sign', 'E-A.W3');
  await app.reader.read();
  (app.shown[0] as { click: () => void }).click();
  assert.equal(await app.reader.read(), true);
  assert.deepEqual(app.reader.view().focus, { wu: 'E-A.W3', seq: 1 });
  app.focus.point(null);
  await app.reader.read();
  assert.deepEqual(app.reader.view().focus, { wu: null, seq: 2 });
  assert.equal(app.reader.view().scheme, 'Keel');
});

test('a keel://wu link points the focus like a click; a refused link changes nothing', async (t) => {
  const dir = routerProject(t, 'alerter-link');
  const app = launch(dir, path.join(tempDir(t, 'alerter-app'), 'Keel'));
  await app.reader.read();
  assert.equal(openLink('keel://wu/E-APPLIST.W2', app.focus), true);
  assert.equal(await app.reader.read(), true);
  assert.deepEqual(app.reader.view().focus, { wu: 'E-APPLIST.W2', seq: 1 });
  assert.equal(openLink('keel://../../etc/passwd', app.focus), false);
  assert.equal(await app.reader.read(), false);
});

test('a router restart starts afresh: its alerts are new lines, each shown', async (t) => {
  const dir = routerProject(t, 'alerter-run');
  const appDir = path.join(tempDir(t, 'alerter-app'), 'Keel');
  routerApp(dir, 'start', 'keel-1c');
  alertLine(dir, 'Old run');
  const app = launch(dir, appDir);
  await app.reader.read();
  routerApp(dir, 'start', 'keel-1c');
  alertLine(dir, 'New run');
  await app.reader.read();
  assert.deepEqual(app.shown.map((s) => s.title), ['Old run', 'New run']);
});

test('the file sink (W9\'s seam) appends each alert as one JSON line', async (t) => {
  const file = path.join(tempDir(t, 'sink'), 'alerts.jsonl');
  const sink = fileSink(file);
  sink.show({ title: 'A', body: 'b', wu: 'E-A.W1' }, () => undefined);
  sink.show({ title: 'C', body: 'd', wu: null }, () => undefined);
  const lines = readFileSync(file, 'utf8').trim().split('\n').map((l) => JSON.parse(l) as Record<string, unknown>);
  assert.deepEqual(lines.map(({ title, body, wu }) => ({ title, body, wu })), [{ title: 'A', body: 'b', wu: 'E-A.W1' }, { title: 'C', body: 'd', wu: null }]);
});

test('a project switched in while the app runs: its stored lines make one summary; switching back shows nothing again', async (t) => {
  const a = routerProject(t, 'alerter-a');
  const b = routerProject(t, 'alerter-b');
  routerApp(a, 'start', 'keel-1c');
  routerApp(b, 'start', 'other-router');
  alertLine(a, 'A one');
  for (const h of ['B one', 'B two', 'B three']) alertLine(b, h);
  let current = a;
  const app = launch(() => current, path.join(tempDir(t, 'alerter-app'), 'Keel'));
  await app.reader.read();
  assert.deepEqual(app.shown.map((s) => s.title), ['A one']);
  current = b;
  await app.reader.read();
  assert.deepEqual(app.shown.map((s) => s.title), ['A one', '3 new alerts'], 'Keel was running: a neutral title');
  current = a;
  await app.reader.read();
  assert.equal(app.shown.length, 2, 'back on the first project: nothing again');
});

test('an app restart with one line written while it was closed shows that line as itself, on a run it already knows', async (t) => {
  const dir = routerProject(t, 'alerter-one');
  const appDir = path.join(tempDir(t, 'alerter-app'), 'Keel');
  routerApp(dir, 'start', 'keel-1c');
  alertLine(dir, 'Seen', 'E-A.W1');
  const first = launch(dir, appDir);
  await first.reader.read();
  alertLine(dir, 'While closed', 'E-A.W2');
  const again = launch(dir, appDir);
  await again.reader.read();
  assert.deepEqual(again.shown.map(({ title, wu }) => ({ title, wu })), [{ title: 'While closed', wu: 'E-A.W2' }]);
});

/** A launch on a project the test can switch, as remove then add does while the app runs. */
function switchable(appDir: string, first: string) {
  let current = first;
  const app = launch(() => current, appDir);
  return { app, to: (dir: string) => (current = dir) };
}

test('a known run switched in after another project spent the app\'s first look gets one summary for its new lines (review round 2, alerts.ts:80)', async (t) => {
  const a = routerProject(t, 'alerter-ka');
  const b = routerProject(t, 'alerter-kb');
  const appDir = path.join(tempDir(t, 'alerter-app'), 'Keel');
  routerApp(a, 'start', 'keel-1c');
  routerApp(b, 'start', 'other-router');
  alertLine(a, 'A seen');
  alertLine(b, 'B seen');
  const one = switchable(appDir, b);
  await one.app.reader.read();
  one.to(a);
  await one.app.reader.read();
  assert.deepEqual(one.app.shown.map((s) => s.title), ['B seen', 'A seen'], 'session 1 knows both runs');
  for (const h of ['B two', 'B three', 'B four']) alertLine(b, h);
  const two = switchable(appDir, a);
  await two.app.reader.read();
  assert.equal(two.app.shown.length, 0, 'session 2 starts on A: nothing new there');
  two.to(b);
  await two.app.reader.read();
  assert.deepEqual(two.app.shown.map(({ title, wu }) => ({ title, wu })), [{ title: '3 new alerts', wu: null }]);
});

test('a launch on a project with no router run does not spend the first look: a known run switched in then summarises its lines written while closed (review round 2, alerts.ts:124)', async (t) => {
  const a = routerProject(t, 'alerter-na');
  const quiet = routerProject(t, 'alerter-quiet');
  const appDir = path.join(tempDir(t, 'alerter-app'), 'Keel');
  routerApp(a, 'start', 'keel-1c');
  alertLine(a, 'One');
  const first = launch(a, appDir);
  await first.reader.read();
  for (const h of ['Two', 'Three', 'Four']) alertLine(a, h);
  const s = switchable(appDir, quiet);
  await s.app.reader.read();
  assert.equal(s.app.reader.view().router.run, null, 'the quiet project has no run');
  s.to(a);
  await s.app.reader.read();
  assert.deepEqual(s.app.shown.map(({ title, wu }) => ({ title, wu })), [{ title: '3 alerts while Keel was closed', wu: null }]);
});

test('a router restart that writes two lines before the app\'s next read gives one summary that does not say Keel was closed (review round 2, alerts.ts:68)', async (t) => {
  const dir = routerProject(t, 'alerter-restart2');
  const app = launch(dir, path.join(tempDir(t, 'alerter-app'), 'Keel'));
  routerApp(dir, 'start', 'keel-1c');
  await app.reader.read();
  routerApp(dir, 'start', 'keel-1c');
  alertLine(dir, 'After restart 1');
  alertLine(dir, 'After restart 2');
  await app.reader.read();
  assert.deepEqual(app.shown.map((x) => x.title), ['2 new alerts']);
});
