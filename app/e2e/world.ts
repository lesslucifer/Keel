// The epic test's world (E-APPLIST.W9): a temp keel project the unpackaged app lists, written to by the
// real router scripts as the router, its watchdog and its sender write to the owner's checkout, and a
// temp app folder with the three file seams. Shared by the Playwright walk (applist.e2e.spec.ts) and its
// fast twin in keel check (test/integration/e2e-world.test.ts), so a drift in the scripts or the reader
// is caught before the slow test runs. Imports only node: built-ins (keel check's node tests reach it).
//
// The project: main (pushed to a bare upstream), epic/E-A, wu/E-A/w1-list carrying its committed plan
// pair, and wu/E-A/w2-open, each unit in a worktree of its own. scripts/ holds copies of router-app.sh,
// router-worker.sh and notify-owner.py, run as the router runs them from its checkout. The watchdog's
// facts step reads a fake claude session list: the router keel-1c and one busy background worker, w2,
// in the w2-open worktree. notify-owner.py --send runs on its test seam: a token and a paired chat in
// temp files, and an API base on a loopback port nothing listens on, so the send fails after the alert
// line is written (W3: the line goes first) and nothing leaves the Mac. Needs sh, git, jq and python3.

import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** The repository this app lives in: the router scripts come from its scripts/. */
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

export const ROUTER = 'keel-1c';
export const EPIC = 'E-A';
/** The unit whose plan is read, opened, marked sent, approved, then reviewed, signed and merged. */
export const PLAN_UNIT = { wu: 'E-A.W1', branch: 'wu/E-A/w1-list', page: 'docs/work/2026-11-22-e-a-w1-plan.html', data: 'docs/work/2026-11-22-e-a-w1-plan.yaml' } as const;
/** The unit a busy worker builds. */
export const WORKER_UNIT = { wu: 'E-A.W2', branch: 'wu/E-A/w2-open', worker: 'w2', session: '0f1e2d3c-4b5a-4968-8776-655443322110' } as const;
/** A question the router asks, by its sender. */
export const QUESTION = { name: 'merge-order', headline: 'Which unit merges first?', lines: ['W1 first, then W2.'] } as const;
/** The plan page as committed, and the edit a working copy then gets. */
export const PLAN_HTML = '<!doctype html>\n<title>E-A.W1 plan</title>\n<p>The committed plan.</p>\n';
export const EDITED_HTML = '<!doctype html>\n<title>E-A.W1 plan</title>\n<p>An edit not yet committed.</p>\n';

/** `name` on this machine's PATH, as an absolute path. */
export function onPath(name: string, pathVar: string = process.env.PATH ?? ''): string {
  for (const dir of pathVar.split(':')) {
    if (dir.startsWith('/') && existsSync(path.join(dir, name))) return path.join(dir, name);
  }
  throw new Error(`no ${name} on PATH: the epic test's world needs it`);
}

/** The whole JSON lines of a sink's text, in order; a last line not yet ended (a write in flight) is not read. */
export function parseLines(text: string): Array<Record<string, unknown>> {
  const whole = text.endsWith('\n') ? text : text.slice(0, text.lastIndexOf('\n') + 1);
  return whole
    .split('\n')
    .filter((l) => l.trim() !== '')
    .map((l) => {
      const v: unknown = JSON.parse(l);
      if (typeof v !== 'object' || v === null || Array.isArray(v)) throw new Error(`a sink line that is not a JSON object: ${l.slice(0, 80)}`);
      return v as Record<string, unknown>;
    });
}

/** A sink file's lines; none while the file is not there. */
export function sinkLines(file: string): Array<Record<string, unknown>> {
  return existsSync(file) ? parseLines(readFileSync(file, 'utf8')) : [];
}

/** `probe()` once it answers something (not undefined, null or false), checked every `everyMs`; fails naming `what` after `ms`. */
export async function waitFor<T>(what: string, ms: number, probe: () => T | undefined | null | false | Promise<T | undefined | null | false>, everyMs = 100): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const v = await probe();
    if (v !== undefined && v !== null && v !== false) return v;
    if (Date.now() >= end) throw new Error(`${what}: not seen within ${ms} ms`);
    await new Promise((r) => setTimeout(r, Math.max(1, Math.min(everyMs, end - Date.now()))));
  }
}

export interface Sent {
  code: number | null;
  stdout: string;
  stderr: string;
}

export interface World {
  /** The temp folder holding everything; removed by close(). */
  root: string;
  /** The project's main checkout: the folder the app lists. */
  project: string;
  /** The project's name as the app shows it (its folder's name). */
  name: string;
  /** The app folder (KEEL_APP_DIR). */
  appDir: string;
  alertSink: string;
  openSink: string;
  /** The worktree of each unit's branch. */
  worktrees: { plan: string; worker: string };
  /** The app's environment: the caller's, plus the four seams. */
  appEnv(silenceMs: number): Record<string, string>;
  /** Real git in the main checkout (no user config, no hooks, no signing). */
  git(...args: string[]): string;
  /** A ref's full commit id. */
  tip(ref: string): string;
  /** router-app.sh, as the router runs it: its stdout. */
  routerApp(...args: string[]): string;
  /** One watchdog tick: router-worker.sh watchdog facts keel-1c, on the fake session list; its facts block. */
  tick(): string;
  /** notify-owner.py --send <body>, on its test seam: the send fails, the lines are written. */
  send(body: string): Sent;
  /** Overwrite the plan page in its worktree (a working edit, not committed). */
  editPlanPage(html: string): void;
  /** The plan page's bytes as committed at `ref`. */
  committedPage(ref: string): Buffer;
  /** Merge `branch` into `into` with --no-ff, from the main checkout, which goes back to main. */
  merge(branch: string, into: string): void;
  close(): void;
}

/** Real git for setting the world up and changing it: no user or system config, no hooks, no signing. */
function gitIn(git: string, home: string) {
  return (cwd: string, args: string[]): string =>
    execFileSync(git, ['-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'init.defaultBranch=main', ...args], {
      cwd,
      encoding: 'utf8',
      env: { PATH: [path.dirname(git), '/usr/bin', '/bin'].join(':'), HOME: home, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LANG: 'C' },
    });
}

/** An executable file holding `body`. */
function program(file: string, body: string): string {
  writeFileSync(file, body);
  chmodSync(file, 0o755);
  return file;
}

/** Build the world under a fresh temp folder (by its real path: macOS's /var is /private/var). */
export function makeWorld(tag = 'e2e'): World {
  const root = realpathSync(mkdtempSync(path.join(os.tmpdir(), `keel-app-${tag}-`)));
  try {
    return build(root);
  } catch (e) {
    rmSync(root, { recursive: true, force: true });
    throw e;
  }
}

function build(root: string): World {
  const git = onPath('git');
  const jq = onPath('jq');
  const python = onPath('python3');
  const run = gitIn(git, root);
  const name = 'e2e-proj';
  const project = path.join(root, name);
  const appDir = path.join(root, 'Keel');
  mkdirSync(path.join(project, '.keel'), { recursive: true });
  mkdirSync(path.join(project, 'scripts'));
  mkdirSync(appDir, { mode: 0o700 });
  for (const s of ['router-app.sh', 'router-worker.sh', 'notify-owner.py']) {
    copyFileSync(path.join(REPO, 'scripts', s), path.join(project, 'scripts', s));
    chmodSync(path.join(project, 'scripts', s), 0o755);
  }
  writeFileSync(path.join(project, '.keel', 'config.toml'), 'code_paths = ["src/"]\n');
  writeFileSync(path.join(project, '.gitignore'), '.router/\nscripts/\n');
  run(project, ['init', '-q']);
  run(project, ['add', '.']);
  run(project, ['commit', '-q', '-m', 'init']);
  run(project, ['branch', '-M', 'main']);
  run(root, ['init', '-q', '--bare', 'up.git']);
  run(project, ['remote', 'add', 'origin', path.join(root, 'up.git')]);
  run(project, ['push', '-q', '-u', 'origin', 'main']);
  run(project, ['branch', `epic/${EPIC}`]);
  // The plan unit carries its plan pair; the worker's unit one commit of its own.
  run(project, ['switch', '-q', '-c', PLAN_UNIT.branch, `epic/${EPIC}`]);
  mkdirSync(path.join(project, 'docs', 'work'), { recursive: true });
  writeFileSync(path.join(project, PLAN_UNIT.page), PLAN_HTML);
  writeFileSync(path.join(project, PLAN_UNIT.data), `artifact: wu-plan\nstatus: ready\nid: ${PLAN_UNIT.wu}\nepic: ${EPIC}\npage: ${path.basename(PLAN_UNIT.page)}\n`);
  run(project, ['add', 'docs']);
  run(project, ['commit', '-q', '-m', `docs(${PLAN_UNIT.wu}): the plan pair`]);
  run(project, ['switch', '-q', '-c', WORKER_UNIT.branch, `epic/${EPIC}`]);
  run(project, ['commit', '-q', '--allow-empty', '-m', `${WORKER_UNIT.wu}: work`]);
  run(project, ['switch', '-q', 'main']);
  const worktrees = { plan: path.join(root, `${name}-w1-list`), worker: path.join(root, `${name}-w2-open`) };
  run(project, ['worktree', 'add', '-q', worktrees.plan, PLAN_UNIT.branch]);
  run(project, ['worktree', 'add', '-q', worktrees.worker, WORKER_UNIT.branch]);

  // The fake session list: the router, and one busy background worker in the w2-open worktree.
  const agents = path.join(root, 'agents.json');
  writeFileSync(
    agents,
    JSON.stringify([
      { id: 'r1', name: ROUTER, sessionId: '11111111-2222-4333-8444-555555555555', cwd: project, status: 'busy', kind: 'interactive' },
      { id: 'w2id', name: WORKER_UNIT.worker, sessionId: WORKER_UNIT.session, cwd: worktrees.worker, status: 'working', kind: 'background' },
    ]),
  );
  const claude = program(path.join(root, 'claude'), `#!/bin/sh\n[ "$1" = agents ] && [ "$2" = --json ] && exec /bin/cat '${agents}'\nexit 1\n`);
  // The sender's seam: a token, a paired chat, and a loopback API nothing listens on (the discard port).
  const telegramEnv = path.join(root, 'telegram.env');
  const access = path.join(root, 'access.json');
  writeFileSync(telegramEnv, 'TELEGRAM_BOT_TOKEN=e2e-token\n', { mode: 0o600 });
  writeFileSync(access, JSON.stringify({ allowFrom: ['4242'] }), { mode: 0o600 });

  const scriptPath = [path.dirname(jq), path.dirname(git), path.dirname(python), '/usr/bin', '/bin'].join(':');
  const scriptEnv = { PATH: scriptPath, HOME: root, LANG: 'C' };
  const script = (file: string, args: string[], extra: Record<string, string> = {}) =>
    spawnSync(file, args, { cwd: project, encoding: 'utf8', env: { ...scriptEnv, ...extra }, timeout: 60_000 });

  const world: World = {
    root,
    project,
    name,
    appDir,
    alertSink: path.join(root, 'alerts.jsonl'),
    openSink: path.join(root, 'opened.jsonl'),
    worktrees,
    appEnv(silenceMs) {
      const env: Record<string, string> = {};
      for (const [k, v] of Object.entries(process.env)) if (v !== undefined && !k.startsWith('KEEL_APP_')) env[k] = v;
      return { ...env, KEEL_APP_DIR: appDir, KEEL_APP_ALERT_SINK: world.alertSink, KEEL_APP_OPEN_SINK: world.openSink, KEEL_APP_SILENCE_MS: String(silenceMs) };
    },
    git: (...args) => run(project, args),
    tip: (ref) => run(project, ['rev-parse', ref]).trim(),
    routerApp(...args) {
      const r = script(path.join(project, 'scripts', 'router-app.sh'), args);
      if (r.status !== 0) throw new Error(`router-app.sh ${args.join(' ')} exited ${r.status}: ${r.stderr.trim()}`);
      return r.stdout;
    },
    tick() {
      const r = script(path.join(project, 'scripts', 'router-worker.sh'), ['watchdog', 'facts', ROUTER], { ROUTER_CLAUDE: claude });
      if (r.status !== 0) throw new Error(`router-worker.sh watchdog facts exited ${r.status}: ${r.stderr.trim()}`);
      return r.stdout;
    },
    send(body) {
      const r = spawnSync(python, [path.join(project, 'scripts', 'notify-owner.py'), '--send', body], {
        cwd: project,
        encoding: 'utf8',
        env: { ...scriptEnv, KEEL_NOTIFY_ENV: telegramEnv, KEEL_NOTIFY_ACCESS: access, KEEL_NOTIFY_API_BASE: 'http://127.0.0.1:9', KEEL_NOTIFY_TIMEOUT: '2' },
        timeout: 60_000,
      });
      return { code: r.status, stdout: r.stdout, stderr: r.stderr };
    },
    editPlanPage(html) {
      writeFileSync(path.join(worktrees.plan, PLAN_UNIT.page), html);
    },
    committedPage: (ref) => execFileSync(git, ['show', `${ref}:${PLAN_UNIT.page}`], { cwd: project, env: { PATH: '/usr/bin:/bin', HOME: root, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' } }),
    merge(branch, into) {
      run(project, ['switch', '-q', into]);
      try {
        run(project, ['merge', '-q', '--no-ff', '-m', `merge ${branch}`, branch]);
      } finally {
        run(project, ['switch', '-q', 'main']);
      }
    },
    close() {
      rmSync(root, { recursive: true, force: true });
    },
  };
  return world;
}
