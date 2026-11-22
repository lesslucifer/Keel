// E-APPLIST.W9's contract at its real boundary, with no Electron and no port: the epic test's world
// (app/e2e/world.ts) written to by the real router-app.sh, the watchdog's facts step on a fake session
// list and notify-owner.py --send on its test seam, read by the real ViewReader with the real Alerter
// on the file sink, SentMarks and Open, as the shell wires them. Each step of the Playwright walk
// (applist.e2e.spec.ts) holds here exactly the items and the alert lines that walk looks for on screen,
// so a drift in the router scripts or the reader fails in keel check before the slow test runs.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ViewReader } from '../../src/main/reader/view.ts';
import { Alerter } from '../../src/main/alerter/alerts.ts';
import { fileSink, openFileSink } from '../../src/main/alerter/seams.ts';
import { SentMarks } from '../../src/main/items/sent.ts';
import { openListed } from '../../src/main/open/open.ts';
import type { Item, View } from '../../src/shared/contract.ts';
import { makeWorld, onPath, sinkLines, EDITED_HTML, PLAN_UNIT, QUESTION, ROUTER, WORKER_UNIT } from '../../e2e/world.ts';

const SILENCE_MS = 60_000;

test('the epic walk\'s world, read as the app reads it: each step holds exactly the items and alert lines the e2e looks for on screen', async (t) => {
  const w = makeWorld('e2e-world');
  t.after(() => w.close());
  const git = onPath('git');
  const project = { id: 'p0e2e0001', name: w.name, path: w.project };
  const runOptions = { path: [path.dirname(git), '/usr/bin', '/bin'], source: { HOME: w.root, LANG: 'C' } };
  // The reader's clock: the real one, moved on only for the silence step.
  let ahead = 0;
  /** One app launch on the world's app folder: the alerter on the file sink, the marks, the reader. */
  const launch = () => {
    const alerter = new Alerter({ appDir: w.appDir, sink: fileSink(w.alertSink), onClick: () => undefined });
    const marks = new SentMarks(w.appDir);
    const reader = new ViewReader({ project: async () => project, git: async () => git, runOptions, sent: (p) => marks.get(p), alerter, silenceMs: SILENCE_MS, now: () => Date.now() + ahead });
    return { reader, marks };
  };
  let app = launch();
  const read = async (): Promise<View> => {
    await app.reader.read();
    const v = app.reader.view();
    assert.equal(v.git?.problem ?? null, null);
    return v;
  };
  /** The list as the walk checks it on screen: group, title, work unit, worker. */
  const list = async () => (await read()).items.map((i) => [i.group, i.title, i.wu, i.worker]);
  const item = async (title: string) => (await read()).items.find((i) => i.title === title) as Item;
  const alerts = () => sinkLines(w.alertSink).map(({ title, subtitle, body, wu }) => ({ title, subtitle, body, wu }));
  const building = ['not-yours', 'Building', WORKER_UNIT.wu, WORKER_UNIT.worker];

  // 1. The project listed, the router not started: nothing waits, no router run.
  assert.deepEqual(await list(), []);
  assert.equal((await read()).router.run, null);

  // 2. The router starts and stages building on E-A.W2; one watchdog tick writes the busy worker and the heartbeat.
  w.routerApp('start', ROUTER);
  w.routerApp('stage', WORKER_UNIT.wu, 'building');
  assert.match(w.tick(), new RegExp(`^${WORKER_UNIT.worker}: busy · quiet \\? · ${WORKER_UNIT.branch} · `, 'm'));
  let v = await read();
  assert.deepEqual(v.router.values[`worker/${WORKER_UNIT.worker}`], { state: 'busy', session: WORKER_UNIT.session, branch: WORKER_UNIT.branch, tip: w.tip(WORKER_UNIT.branch).slice(0, 7), dirty: 0 });
  assert.deepEqual(v.router.values.router, { name: ROUTER, state: 'busy', workers: 1 });
  assert.deepEqual(await list(), [building]);

  // 3. Plan to read on E-A.W1, and the sender's message naming it: one alert line, the send itself failing (nothing listens).
  const planTip = w.tip(PLAN_UNIT.branch);
  w.routerApp('stage', PLAN_UNIT.wu, 'plan-to-read', planTip.slice(0, 7));
  const sent = w.send(`wu: ${PLAN_UNIT.wu}\nPlan to read: ${PLAN_UNIT.wu}\nThe plan is committed.`);
  assert.notEqual(sent.code, 0, 'the send fails: nothing listens on the seam\'s port');
  assert.match(sent.stderr, /the desk alert was written \(n \d+\)/);
  assert.deepEqual(await list(), [['yours', 'Plan to read', PLAN_UNIT.wu, null], building]);
  assert.deepEqual(alerts(), [{ title: `Plan to read: ${PLAN_UNIT.wu}`, subtitle: w.name, body: 'The plan is committed.', wu: PLAN_UNIT.wu }]);
  assert.equal((await read()).alerts.at(-1)?.link, `keel://wu/${PLAN_UNIT.wu}`);

  // 4. The plan's working page edited, then Open: one file URL under the app folder, holding the committed bytes.
  w.editPlanPage(EDITED_HTML);
  const plan = await item('Plan to read');
  assert.deepEqual(await openListed(plan, app.reader.view(), [project], { appDir: w.appDir, git, runOptions, openUrl: async (url) => openFileSink(w.openSink)(url) }), { done: true });
  const opened = sinkLines(w.openSink);
  assert.equal(opened.length, 1);
  const file = fileURLToPath(opened[0]?.url as string);
  assert.ok(file.startsWith(w.appDir + path.sep), file);
  assert.ok(file.endsWith(PLAN_UNIT.page), file);
  assert.deepEqual(readFileSync(file), w.committedPage(PLAN_UNIT.branch));
  assert.notDeepEqual(readFileSync(file), Buffer.from(EDITED_HTML));

  // 5. Mark sent: under Sent. A new launch on the same app folder: still under Sent, and no alert again.
  await app.marks.set(project.id, plan.id, true, new Set((await read()).items.filter((i) => i.markable).map((i) => i.id)));
  assert.deepEqual(await list(), [building, ['sent', 'Plan to read', PLAN_UNIT.wu, null]]);
  app = launch();
  assert.deepEqual(await list(), [building, ['sent', 'Plan to read', PLAN_UNIT.wu, null]]);
  assert.equal(alerts().length, 1);

  // 6. (A second launch is Electron's: the walk alone checks it.) 7. Plan approved: the plan leaves.
  w.tick();
  w.routerApp('stage', PLAN_UNIT.wu, 'plan-approved', planTip.slice(0, 7));
  assert.deepEqual(await list(), [building]);

  // 8. A PASS review note on the unit's tip: result to sign; the sign-off note: to merge; the merge into the epic: gone.
  w.git('notes', '--ref=cr', 'add', '-m', JSON.stringify({ schema: 'cr/v1', verdict: 'PASS', blockers: 0, importants: 0, code_diff_hash: 'x' }), PLAN_UNIT.branch);
  assert.deepEqual(await list(), [['yours', 'Result to sign', PLAN_UNIT.wu, null], building]);
  w.git('notes', '--ref=mr', 'add', '-m', '{"schema":"mr/v1"}', PLAN_UNIT.branch);
  assert.deepEqual(await list(), [['yours', 'Result to merge', PLAN_UNIT.wu, null], building]);
  w.merge(PLAN_UNIT.branch, 'epic/E-A');
  assert.deepEqual(await list(), [building]);

  // 9. A question, by the sender (one more alert line, no work unit); its text shown; then the router deletes it.
  w.tick();
  const asked = w.send(`question: ${QUESTION.name}\n${QUESTION.headline}\n${QUESTION.lines[0]}`);
  assert.match(asked.stderr, /the desk alert was written \(n \d+\)/);
  const question = await item('Question from the router');
  assert.deepEqual([question.group, question.why, question.open, question.text], ['yours', QUESTION.headline, 'text', [...QUESTION.lines]]);
  assert.deepEqual(alerts().at(-1), { title: QUESTION.headline, subtitle: w.name, body: QUESTION.lines[0], wu: null });
  assert.equal(alerts().length, 2);
  w.routerApp('del', `question/${QUESTION.name}`);
  assert.deepEqual(await list(), [building]);

  // 10. Main moves (not pushed), then the router restarts: the worker and the stage are gone, git's item stays.
  w.git('commit', '-q', '--allow-empty', '-m', 'on main');
  assert.deepEqual(await list(), [['yours', 'Main not pushed', null, null], building]);
  w.routerApp('start', ROUTER);
  v = await read();
  assert.deepEqual(v.router.values, {});
  assert.deepEqual(await list(), [['yours', 'Main not pushed', null, null]]);
  assert.equal(alerts().length, 2, 'a new run alerts nothing again');

  // 11. Silence past the limit: not heard; one heartbeat: heard again.
  ahead = SILENCE_MS + 1_000;
  assert.equal((await read()).router.heard, false);
  ahead = 0;
  w.tick();
  v = await read();
  assert.equal(v.router.heard, true);
  assert.deepEqual(v.router.values.router, { name: ROUTER, state: 'busy', workers: 1 });
  assert.equal(alerts().length, 2);
});
