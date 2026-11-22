// E-APPLIST's epic test (DEC-75): Playwright drives the unpackaged app, built by `pnpm build:e2e` (Vite's
// bundles only, the shipped page served from app://keel/ under the packaged content policy), on the
// world of e2e/world.ts: a temp keel project the real router-app.sh, the watchdog's facts step and
// notify-owner.py --send write to, and a temp app folder (KEEL_APP_DIR), so the owner's Keel, its folder
// and its single-instance lock are never touched. Alerts go to KEEL_APP_ALERT_SINK, Open's URLs to
// KEEL_APP_OPEN_SINK, and KEEL_APP_SILENCE_MS shortens the 15 minutes before 'router not heard'.
// Each step of the epic's flow is one named step; a step fails when the screen or a sink does not show
// what it looks for within expect's wait. Run by hand: cd app && pnpm e2e (never in keel check, DEC-73).
// test/integration/e2e-world.test.ts walks the same world through the reader, with no Electron.

import { test, expect, _electron as electron, type ElectronApplication, type Locator, type Page } from '@playwright/test';
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { APP_ROOT } from './build.ts';
import { makeWorld, sinkLines, EDITED_HTML, PLAN_UNIT, QUESTION, ROUTER, WORKER_UNIT, type World } from './world.ts';
import { SECOND_LAUNCH } from '../src/main/instance.ts';

/** Long enough that no step outlives it with the watchdog's tick at each step's start; step 11 waits it out. */
const SILENCE_MS = 45_000;
const PAGE = /^app:\/\/keel\//;
/** A console line the page's content policy writes when it refuses something. */
const REFUSAL = /Content[- ]Security[- ]Policy|Refused to (load|execute|apply|connect|frame|create)/i;

interface Running {
  app: ElectronApplication;
  page: Page;
}

/** The environment the app runs with: the world's seams, and nothing that would make Electron run as Node. */
function appEnv(w: World): Record<string, string> {
  const env = w.appEnv(SILENCE_MS);
  delete env.ELECTRON_RUN_AS_NODE;
  delete env.NODE_OPTIONS;
  return env;
}

/** Launch the app on the world's app folder; every window's console refusals and page errors are collected. */
async function launch(w: World, refusals: string[]): Promise<Running> {
  const app = await electron.launch({ args: [APP_ROOT], cwd: APP_ROOT, env: appEnv(w) });
  const watch = (p: Page) => {
    p.on('console', (m) => {
      if (REFUSAL.test(m.text())) refusals.push(`${p.url()}: ${m.text()}`);
    });
    p.on('pageerror', (e) => refusals.push(`${p.url()}: page error: ${e.message}`));
  };
  app.on('window', watch);
  const page = await app.firstWindow();
  watch(page);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  expect(page.url()).toMatch(PAGE);
  return { app, page };
}

/** A group of the list (Yours, Not yours, Sent) by its heading. */
function group(page: Page, name: 'Yours' | 'Not yours' | 'Sent'): Locator {
  return page.locator('section').filter({ has: page.getByRole('heading', { level: 2, name: new RegExp(`^${name}[ :,]`) }) });
}

/** An item row in a group, by its title. */
function item(page: Page, where: 'Yours' | 'Not yours' | 'Sent', title: string): Locator {
  return group(page, where).getByRole('group', { name: title, exact: true });
}

test('E-APPLIST: from a router line to the item on screen and out again, alerts, Open, Mark sent, restarts and silence', async () => {
  const w = makeWorld('e2e-walk');
  const refusals: string[] = [];
  let running: Running | null = null;
  const alerts = () => sinkLines(w.alertSink);
  try {
    let page!: Page;
    const planTip = w.tip(PLAN_UNIT.branch);

    await test.step('1. Launch on a temp app folder; Projects: Add (the folder dialog answered in the main part); Waits on you, empty', async () => {
      running = await launch(w, refusals);
      page = running.page;
      await running.app.evaluate(({ dialog }, dir) => {
        dialog.showOpenDialog = (async () => ({ canceled: false, filePaths: [dir] })) as unknown as typeof dialog.showOpenDialog;
      }, w.project);
      await expect(page.getByRole('heading', { level: 1, name: 'Projects' })).toBeVisible();
      await page.getByRole('button', { name: 'Add a project…' }).click();
      await expect(page.getByText('Project added.')).toBeVisible();
      await expect(page.getByText(w.name, { exact: true }).first()).toBeVisible();
      await page.getByRole('button', { name: /^Waits on you/ }).click();
      await expect(page.getByRole('heading', { level: 1, name: 'Waits on you' })).toBeVisible();
      await expect(page.getByText(/^The router has not started here/)).toBeVisible();
      await expect(page.getByRole('heading', { level: 2, name: 'Yours (0)' })).toBeVisible();
    });

    await test.step('2. Router start; the watchdog\'s tick (one busy worker on E-A.W2) and the stage building: work in motion under Not yours, naming the worker', async () => {
      w.routerApp('start', ROUTER);
      w.routerApp('stage', WORKER_UNIT.wu, 'building');
      w.tick();
      const building = item(page, 'Not yours', 'Building');
      await expect(building).toBeVisible();
      await expect(building).toContainText(`Its worker: ${WORKER_UNIT.worker}`);
      await expect(building).toContainText(WORKER_UNIT.wu);
    });

    await test.step('3. Plan to read on E-A.W1, and the sender\'s message: under Yours; exactly one alert line, the project as subtitle', async () => {
      w.tick();
      w.routerApp('stage', PLAN_UNIT.wu, 'plan-to-read', planTip.slice(0, 7));
      const sent = w.send(`wu: ${PLAN_UNIT.wu}\nPlan to read: ${PLAN_UNIT.wu}\nThe plan is committed.`);
      expect(sent.stderr).toMatch(/the desk alert was written \(n \d+\)/);
      await expect(item(page, 'Yours', 'Plan to read')).toBeVisible();
      await expect.poll(() => alerts().length).toBe(1);
      expect(alerts()[0]).toMatchObject({ title: `Plan to read: ${PLAN_UNIT.wu}`, subtitle: w.name, body: 'The plan is committed.', wu: PLAN_UNIT.wu });
    });

    await test.step('4. The plan\'s working page edited, then Open: one file URL under the app folder, holding the page as committed', async () => {
      w.editPlanPage(EDITED_HTML);
      await item(page, 'Yours', 'Plan to read').getByRole('button', { name: 'Open the plan' }).click();
      await expect(page.getByText('Opened in your browser.')).toBeVisible();
      await expect.poll(() => sinkLines(w.openSink).length).toBe(1);
      const url = sinkLines(w.openSink)[0]?.url as string;
      expect(url).toMatch(/^file:\/\//);
      const file = fileURLToPath(url);
      expect(file.startsWith(w.appDir + path.sep)).toBe(true);
      expect(readFileSync(file).equals(w.committedPage(PLAN_UNIT.branch))).toBe(true);
    });

    await test.step('5. Mark sent: under Sent. Quit and launch again on the same app folder: still under Sent, and no alert again', async () => {
      w.tick();
      await item(page, 'Yours', 'Plan to read').getByRole('button', { name: 'Mark sent' }).click();
      await expect(item(page, 'Sent', 'Plan to read')).toBeVisible();
      await (running as Running).app.close();
      running = null;
      w.tick();
      running = await launch(w, refusals);
      page = running.page;
      await expect(page.getByRole('heading', { level: 1, name: 'Waits on you' })).toBeVisible();
      await expect(item(page, 'Sent', 'Plan to read')).toBeVisible();
      await expect(item(page, 'Not yours', 'Building')).toBeVisible();
      expect(alerts()).toHaveLength(1);
    });

    await test.step('6. A second launch on the same app folder while the first runs: it says so on stderr and quits; the first window stays', async () => {
      const electronPath = createRequire(import.meta.url)('electron') as string;
      const second = spawn(electronPath, [APP_ROOT], { cwd: APP_ROOT, env: appEnv(w), stdio: ['ignore', 'pipe', 'pipe'] });
      let stderr = '';
      second.stderr.setEncoding('utf8').on('data', (d: string) => (stderr += d));
      second.stdout.resume();
      const exited = await new Promise<boolean>((resolve) => {
        const timer = setTimeout(() => resolve(false), 30_000);
        second.on('exit', () => {
          clearTimeout(timer);
          resolve(true);
        });
      });
      if (!exited) second.kill('SIGKILL');
      expect(exited, 'the second launch quits within 30 s').toBe(true);
      expect(stderr).toContain(SECOND_LAUNCH);
      await expect(page.getByRole('heading', { level: 1, name: 'Waits on you' })).toBeVisible();
      await expect(item(page, 'Sent', 'Plan to read')).toBeVisible();
    });

    await test.step('7. Plan approved: the plan leaves', async () => {
      w.tick();
      w.routerApp('stage', PLAN_UNIT.wu, 'plan-approved', planTip.slice(0, 7));
      await expect(page.getByRole('heading', { level: 2, name: 'Sent, waiting for a sign (0)' })).toBeVisible();
      await expect(page.getByRole('group', { name: 'Plan to read', exact: true })).toHaveCount(0);
    });

    await test.step('8. A PASS review note on E-A.W1\'s tip: result to sign; the sign-off note: to merge; the merge into the epic: it leaves', async () => {
      w.tick();
      w.git('notes', '--ref=cr', 'add', '-m', JSON.stringify({ schema: 'cr/v1', verdict: 'PASS', blockers: 0, importants: 0, code_diff_hash: 'x' }), PLAN_UNIT.branch);
      await expect(item(page, 'Yours', 'Result to sign')).toBeVisible();
      w.git('notes', '--ref=mr', 'add', '-m', '{"schema":"mr/v1"}', PLAN_UNIT.branch);
      await expect(item(page, 'Yours', 'Result to merge')).toBeVisible();
      w.merge(PLAN_UNIT.branch, 'epic/E-A');
      await expect(page.getByRole('group', { name: /^Result to (sign|merge)$/ })).toHaveCount(0);
      await expect(page.getByRole('heading', { level: 2, name: 'Yours (0)' })).toBeVisible();
    });

    await test.step('9. A question by the sender: it enters, Show the question gives its text; one more alert line; the router deletes it: it leaves', async () => {
      w.tick();
      const asked = w.send(`question: ${QUESTION.name}\n${QUESTION.headline}\n${QUESTION.lines[0]}`);
      expect(asked.stderr).toMatch(/the desk alert was written \(n \d+\)/);
      const question = item(page, 'Yours', 'Question from the router');
      await expect(question).toBeVisible();
      await expect(question).toContainText(QUESTION.headline);
      await question.getByRole('button', { name: 'Show the question' }).click();
      await expect(question.getByRole('list', { name: 'The question' })).toHaveText(QUESTION.lines[0]);
      await expect.poll(() => alerts().length).toBe(2);
      expect(alerts()[1]).toMatchObject({ title: QUESTION.headline, subtitle: w.name, body: QUESTION.lines[0], wu: null });
      w.routerApp('del', `question/${QUESTION.name}`);
      await expect(page.getByRole('group', { name: 'Question from the router', exact: true })).toHaveCount(0);
    });

    await test.step('10. Main moves (not pushed); the router restarts: the worker and the stage are gone, git\'s item stays', async () => {
      w.tick();
      w.git('commit', '-q', '--allow-empty', '-m', 'on main');
      await expect(item(page, 'Yours', 'Main not pushed')).toBeVisible();
      w.routerApp('start', ROUTER);
      await expect(page.getByRole('heading', { level: 2, name: 'Not yours: a worker acts (0)' })).toBeVisible();
      await expect(item(page, 'Yours', 'Main not pushed')).toBeVisible();
      expect(alerts()).toHaveLength(2);
    });

    await test.step(`11. No router line for ${SILENCE_MS / 1000} s (KEEL_APP_SILENCE_MS): 'router not heard'; one heartbeat: the bar goes`, async () => {
      const notHeard = page.getByText(/^The router is not heard/);
      await page.waitForTimeout(SILENCE_MS + 1_000);
      // A branch the list does not show, so the app reads now rather than at its minute's re-read.
      w.git('branch', 'e2e-nudge');
      await expect(notHeard).toBeVisible();
      await expect(item(page, 'Yours', 'Main not pushed')).toBeVisible();
      w.tick();
      await expect(notHeard).toHaveCount(0);
      expect(alerts()).toHaveLength(2);
    });

    await test.step('Throughout: no content-policy refusal or page error; the page is app://keel/', async () => {
      expect(page.url()).toMatch(PAGE);
      await page.getByRole('button', { name: 'Projects' }).click();
      await expect(page.getByRole('heading', { level: 1, name: 'Projects' })).toBeVisible();
      expect(refusals).toEqual([]);
    });
  } finally {
    if (running !== null) await (running as Running).app.close().catch(() => undefined);
    w.close();
  }
});
