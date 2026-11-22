// Where a click points the window, at its real boundary: an alert line written by the real router-app.sh,
// its click through the alerter, the focus in waiting.get's view; a project switch clears it, so a
// focus never names another project's work unit (W5's follow-up, taken by E-APPLIST.W6).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { ViewReader } from '../../src/main/reader/view.ts';
import { Alerter, Focus, type ShownAlert } from '../../src/main/alerter/alerts.ts';
import { openLink } from '../../src/main/alerter/link.ts';
import { realGit, tempDir } from '../support/fs.ts';
import { routerProject, routerApp } from '../support/router.ts';

const GIT = realGit();

function launch(t: { after: (fn: () => void) => void }) {
  const a = routerProject(t, 'focus-a');
  const b = routerProject(t, 'focus-b');
  routerApp(a, 'start', 'keel-1c');
  routerApp(b, 'start', 'keel-1c');
  let current: { id: string; name: string; path: string } | null = { id: 'p0a1b2c3d', name: 'proj-a', path: a };
  const shown: Array<ShownAlert & { click: () => void }> = [];
  const focus = new Focus();
  const alerter = new Alerter({ appDir: path.join(tempDir(t, 'focus-app'), 'Keel'), sink: { show: (x, onClick) => shown.push({ ...x, click: onClick }) }, onClick: (wu, project) => focus.point(wu, project) });
  const reader = new ViewReader({
    project: async () => current,
    git: async () => GIT,
    runOptions: { path: [path.dirname(GIT), '/usr/bin', '/bin'], source: { HOME: a, LANG: 'C' } },
    alerter,
    focus: (project) => focus.get(project),
    projectChanged: () => focus.clear(),
  });
  return { a, b, shown, focus, reader, switchTo: (p: typeof current) => (current = p) };
}

test('an alert names the listed project, and its click points the view\'s focus at its work unit', async (t) => {
  const w = launch(t);
  await w.reader.read();
  routerApp(w.a, 'alert', JSON.stringify({ headline: 'Result to sign', lines: ['E-A.W1 passed review.'], wu: 'E-A.W1', link: 'keel://wu/E-A.W1' }));
  await w.reader.read();
  assert.deepEqual(w.shown.map(({ title, subtitle, wu }) => ({ title, subtitle, wu })), [{ title: 'Result to sign', subtitle: 'proj-a', wu: 'E-A.W1' }]);
  w.shown[0]?.click();
  await w.reader.read();
  assert.deepEqual(w.reader.view().focus, { wu: 'E-A.W1', seq: 1 });
});

test('a project switch clears the focus; reading the same project again keeps it; a link launched before the first read is kept', async (t) => {
  const w = launch(t);
  openLink('keel://wu/E-A.W1', w.focus);
  await w.reader.read();
  assert.deepEqual(w.reader.view().focus, { wu: 'E-A.W1', seq: 1 }, 'the first read is no switch');
  await w.reader.read();
  assert.deepEqual(w.reader.view().focus, { wu: 'E-A.W1', seq: 1 });
  w.switchTo({ id: 'p11111111', name: 'proj-b', path: w.b });
  await w.reader.read();
  assert.equal(w.reader.view().focus, null);
  openLink('keel://wu/E-B.W2', w.focus);
  await w.reader.read();
  assert.deepEqual(w.reader.view().focus, { wu: 'E-B.W2', seq: 2 });
  w.switchTo(null);
  await w.reader.read();
  assert.equal(w.reader.view().focus, null, 'removing the project clears it too');
});

test('W6 review round 1 (index.ts:191): a click on an alert shown for one project, made after a switch to another, points no focus there', async (t) => {
  const w = launch(t);
  await w.reader.read();
  routerApp(w.a, 'alert', JSON.stringify({ headline: 'Result to sign', lines: ['E-A.W1 passed review.'], wu: 'E-A.W1', link: 'keel://wu/E-A.W1' }));
  await w.reader.read();
  w.switchTo({ id: 'p11111111', name: 'proj-b', path: w.b });
  await w.reader.read();
  w.shown[0]?.click();
  await w.reader.read();
  assert.equal(w.reader.view().focus, null, 'project A\'s alert names A\'s work unit, not one of B\'s');
  // A keel:// link names no project: it points the listed one.
  openLink('keel://wu/E-B.W1', w.focus);
  await w.reader.read();
  assert.deepEqual(w.reader.view().focus, { wu: 'E-B.W1', seq: 2 });
});

test('W6 review round 1 (App.tsx:24): waiting.get\'s focus is the focus as it stands, so a window re-created after a delivered click sees none, before any new read', async (t) => {
  const w = launch(t);
  openLink('keel://wu/E-A.W1', w.focus);
  w.focus.opened(true);
  await w.reader.read();
  assert.deepEqual(w.reader.view().focus, { wu: 'E-A.W1', seq: 1 });
  w.focus.read(1);
  w.focus.opened(true);
  assert.equal(w.reader.view().focus, null, 'no read between: the view answers the focus now');
});
