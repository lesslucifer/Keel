// A temp main checkout whose .router/app/ the real scripts/router-app.sh writes (copied in, as the
// router runs it from its checkout). Needs sh, git and jq, as the Rust suite's router tests do.
import { copyFileSync, chmodSync, readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { tempDir, realGit, sh, mk } from './fs.ts';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const SCRIPT = path.join(REPO, 'scripts', 'router-app.sh');

/** `name` on this machine's PATH. */
export function onPath(name: string): string {
  for (const dir of (process.env.PATH ?? '').split(':')) {
    if (dir.startsWith('/') && existsSync(path.join(dir, name))) return path.join(dir, name);
  }
  throw new Error(`no ${name} on PATH: the router's update log needs it`);
}

/** A temp git checkout holding a copy of router-app.sh, under a fresh temp folder; returns its path. */
export function routerProject(t: { after: (fn: () => void) => void }, tag = 'router'): string {
  const dir = mk(path.join(tempDir(t, tag), 'proj'));
  sh(realGit(), dir, ['init', '-q']);
  mk(path.join(dir, 'scripts'));
  copyFileSync(SCRIPT, path.join(dir, 'scripts', 'router-app.sh'));
  chmodSync(path.join(dir, 'scripts', 'router-app.sh'), 0o755);
  return dir;
}

/** Run the project's router-app.sh with `args`, as the router or the watchdog does; its stdout. */
export function routerApp(dir: string, ...args: string[]): string {
  return execFileSync(path.join(dir, 'scripts', 'router-app.sh'), args, {
    encoding: 'utf8',
    env: { PATH: [path.dirname(onPath('jq')), path.dirname(realGit()), '/usr/bin', '/bin'].join(':'), HOME: dir, LANG: 'C' },
  });
}

export const appDir = (dir: string): string => path.join(dir, '.router', 'app');

/** The writer's state.json as it stands. */
export function routerState(dir: string): { run: string; n: number; at: string; values: Record<string, unknown>; alerts: unknown[] } {
  return JSON.parse(readFileSync(path.join(appDir(dir), 'state.json'), 'utf8'));
}
