// The router's update log at its real boundary: a temp project whose .router/app/ is written by the
// real scripts/router-app.sh (copied in, as the router runs it from its checkout), read by the
// reader the app runs. Needs sh, git and jq, as the Rust suite's router tests do.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, chmodSync, readFileSync, writeFileSync, appendFileSync, renameSync, symlinkSync, mkdirSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { RouterLog, SILENCE_MS } from '../../src/main/reader/log.ts';
import { tempDir, realGit, sh, mk, snapshot } from '../support/fs.ts';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const SCRIPT = path.join(REPO, 'scripts', 'router-app.sh');
const GIT = realGit();

function onPath(name: string): string {
  for (const dir of (process.env.PATH ?? '').split(':')) {
    if (dir.startsWith('/') && existsSync(path.join(dir, name))) return path.join(dir, name);
  }
  throw new Error(`no ${name} on PATH: the router's update log needs it`);
}
const JQ = onPath('jq');

/** A temp main checkout holding a copy of router-app.sh, as a project the router runs in. */
function project(t: { after: (fn: () => void) => void }): string {
  const dir = mk(path.join(tempDir(t, 'router-log'), 'proj'));
  sh(GIT, dir, ['init', '-q']);
  mk(path.join(dir, 'scripts'));
  copyFileSync(SCRIPT, path.join(dir, 'scripts', 'router-app.sh'));
  chmodSync(path.join(dir, 'scripts', 'router-app.sh'), 0o755);
  return dir;
}

/** Run the project's router-app.sh with `args`, as the router or the watchdog does. */
function routerApp(dir: string, ...args: string[]): string {
  return execFileSync(path.join(dir, 'scripts', 'router-app.sh'), args, {
    encoding: 'utf8',
    env: { PATH: [path.dirname(JQ), path.dirname(GIT), '/usr/bin', '/bin'].join(':'), HOME: dir, LANG: 'C' },
  });
}

const appDir = (dir: string) => path.join(dir, '.router', 'app');
const state = (dir: string) => JSON.parse(readFileSync(path.join(appDir(dir), 'state.json'), 'utf8')) as { run: string; n: number; at: string; values: Record<string, unknown>; alerts: unknown[] };

/** A started log with a stage, a worker, a question, an alert and a delete. */
function busyLog(dir: string): void {
  routerApp(dir, 'start', 'keel-1c');
  routerApp(dir, 'stage', 'E-APPLIST.W5', 'plan-to-read', 'abc1234');
  routerApp(dir, 'set', 'worker/e-applist-w5', '{"state":"busy","branch":"wu/E-APPLIST/w5-reader-watcher"}');
  routerApp(dir, 'set', 'question/merge-order', '{"headline":"Merge W5 first?","lines":["W6 builds on it."]}');
  routerApp(dir, 'alert', '{"headline":"Plan to read","lines":["E-APPLIST.W5"],"wu":"E-APPLIST.W5","link":"keel://wu/E-APPLIST.W5"}');
  routerApp(dir, 'set', 'router', '{"name":"keel-1c","state":"busy","workers":1}');
  routerApp(dir, 'del', 'question/merge-order');
}

test('the reader folds the real log to exactly the values and alerts the writer\'s state holds, and writes nothing', async (t) => {
  const dir = project(t);
  busyLog(dir);
  const before = snapshot(dir);
  const log = new RouterLog({ root: dir });
  assert.equal(await log.refresh(), true);
  const s = state(dir);
  const v = log.view();
  assert.deepEqual({ run: v.run, n: v.n, values: v.values, lastAt: v.lastAt, problem: v.problem }, { run: s.run, n: s.n, values: s.values, lastAt: s.at, problem: null });
  assert.deepEqual(log.alerts(), s.alerts);
  assert.deepEqual(v.values['wu/E-APPLIST.W5/stage'], { stage: 'plan to read', tip: 'abc1234' });
  assert.equal(v.heard, true);
  assert.equal(await log.refresh(), false, 'nothing new: no change');
  assert.deepEqual(snapshot(dir), before);
});

test('lines written after the first read are followed, one refresh each, with no state read', async (t) => {
  const dir = project(t);
  routerApp(dir, 'start', 'keel-1c');
  const log = new RouterLog({ root: dir });
  await log.refresh();
  const reads = log.stats.stateReads;
  routerApp(dir, 'stage', 'E-APPLIST.W5', 'building');
  assert.equal(await log.refresh(), true);
  assert.deepEqual(log.view().values, { 'wu/E-APPLIST.W5/stage': { stage: 'building' } });
  routerApp(dir, 'del', 'wu/E-APPLIST.W5/stage');
  assert.equal(await log.refresh(), true);
  assert.deepEqual(log.view().values, {});
  assert.equal(log.view().n, state(dir).n);
  assert.equal(log.stats.stateReads, reads);
});

test('an app restart (a new reader on the same folder) rebuilds the same view', async (t) => {
  const dir = project(t);
  busyLog(dir);
  const first = new RouterLog({ root: dir });
  await first.refresh();
  const again = new RouterLog({ root: dir });
  await again.refresh();
  assert.deepEqual(again.view(), first.view());
  assert.deepEqual(again.alerts(), first.alerts());
});

test('a router restart (start) drops every router value: each field is back at its default', async (t) => {
  const dir = project(t);
  busyLog(dir);
  const log = new RouterLog({ root: dir });
  await log.refresh();
  const oldRun = log.view().run;
  routerApp(dir, 'start', 'keel-1c');
  assert.equal(await log.refresh(), true);
  const v = log.view();
  assert.notEqual(v.run, oldRun);
  assert.equal(v.run, state(dir).run);
  assert.deepEqual(v.values, {});
  assert.deepEqual(log.alerts(), []);
});

test('a missing number (a line lost from the log) makes the reader read the state once, and it lands on the state', async (t) => {
  const dir = project(t);
  routerApp(dir, 'start', 'keel-1c');
  const log = new RouterLog({ root: dir });
  await log.refresh();
  const reads = log.stats.stateReads;
  routerApp(dir, 'set', 'facts', '{"a":1}');
  routerApp(dir, 'set', 'router', '{"name":"keel-1c"}');
  const file = path.join(appDir(dir), 'log.jsonl');
  const kept = readFileSync(file, 'utf8').split('\n').filter((l) => !l.includes('"key":"facts"')).join('\n');
  writeFileSync(file, kept);
  assert.equal(await log.refresh(), true);
  assert.equal(log.stats.stateReads, reads + 1);
  assert.deepEqual(log.view().values, state(dir).values);
  assert.equal(log.view().n, state(dir).n);
});

test('a half-written line waits for its end, then is applied once', async (t) => {
  const dir = project(t);
  routerApp(dir, 'start', 'keel-1c');
  const log = new RouterLog({ root: dir });
  await log.refresh();
  const s = state(dir);
  const next = JSON.stringify({ run: s.run, n: s.n + 1, at: s.at, op: 'set', key: 'facts', value: { half: true } }) + '\n';
  const file = path.join(appDir(dir), 'log.jsonl');
  appendFileSync(file, next.slice(0, 25));
  assert.equal(await log.refresh(), false);
  assert.deepEqual(log.view().values, {});
  appendFileSync(file, next.slice(25));
  assert.equal(await log.refresh(), true);
  assert.deepEqual(log.view().values, { facts: { half: true } });
  assert.equal(await log.refresh(), false);
});

test('a line with an unknown key is skipped: its value never shows and the state is not read for it', async (t) => {
  const dir = project(t);
  routerApp(dir, 'start', 'keel-1c');
  const log = new RouterLog({ root: dir });
  await log.refresh();
  const reads = log.stats.stateReads;
  const s = state(dir);
  appendFileSync(path.join(appDir(dir), 'log.jsonl'), JSON.stringify({ run: s.run, n: s.n + 1, at: s.at, op: 'set', key: 'newthing/x', value: 1 }) + '\n');
  await log.refresh();
  assert.deepEqual(log.view().values, {});
  assert.equal(log.view().n, s.n + 1);
  assert.equal(log.stats.stateReads, reads);
});

test('a log replaced by a new file (as the writer does past 2,000 lines) is read from its start', async (t) => {
  const dir = project(t);
  routerApp(dir, 'start', 'keel-1c');
  routerApp(dir, 'set', 'facts', '{"a":1}');
  const log = new RouterLog({ root: dir });
  await log.refresh();
  const s = state(dir);
  const file = path.join(appDir(dir), 'log.jsonl');
  const fresh = path.join(appDir(dir), '.log.fresh');
  writeFileSync(fresh, JSON.stringify({ run: s.run, n: s.n + 1, at: s.at, op: 'set', key: 'router', value: { name: 'keel-1c' } }) + '\n');
  renameSync(fresh, file);
  assert.equal(await log.refresh(), true);
  assert.deepEqual(log.view().values, { facts: { a: 1 }, router: { name: 'keel-1c' } });
});

test('silence: 15 minutes after the newest line the router is not heard; a new line clears it', async (t) => {
  const dir = project(t);
  routerApp(dir, 'start', 'keel-1c');
  let now = 0;
  const log = new RouterLog({ root: dir, now: () => now });
  await log.refresh();
  const at = Date.parse(state(dir).at);
  now = at + SILENCE_MS - 1;
  assert.equal(log.view().heard, true);
  now = at + SILENCE_MS;
  assert.equal(log.view().heard, false);
  routerApp(dir, 'set', 'router', '{"name":"keel-1c"}');
  await log.refresh();
  now = Date.parse(state(dir).at) + 1000;
  assert.equal(log.view().heard, true);
});

test('no log yet, or a .router/app that is a link, is no run: every router field at its default; a link says why', async (t) => {
  const dir = project(t);
  const log = new RouterLog({ root: dir });
  await log.refresh();
  assert.deepEqual(log.view(), { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null });
  const elsewhere = mk(path.join(path.dirname(dir), 'elsewhere'));
  mkdirSync(path.join(dir, '.router'));
  symlinkSync(elsewhere, appDir(dir));
  writeFileSync(path.join(elsewhere, 'state.json'), JSON.stringify({ run: 'x.20261122-000000.1', n: 1, at: '2026-11-22T00:00:00Z', values: { router: 1 }, alerts: [] }));
  await log.refresh();
  const v = log.view();
  assert.deepEqual([v.run, v.values], [null, {}]);
  assert.match(v.problem ?? '', /link/);
});

test('a state.json that is not the writer\'s shape is no run, says why, and is left where it is', async (t) => {
  const dir = project(t);
  mk(appDir(dir));
  writeFileSync(path.join(appDir(dir), 'state.json'), '{"run": 7}');
  const log = new RouterLog({ root: dir });
  await log.refresh();
  assert.equal(log.view().run, null);
  assert.match(log.view().problem ?? '', /state/);
  assert.equal(readFileSync(path.join(appDir(dir), 'state.json'), 'utf8'), '{"run": 7}');
});
