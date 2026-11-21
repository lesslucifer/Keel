// The PATH, read once at start from the owner's login shell: git's hooks call keel by PATH, and an
// app opened from the Finder gets only launchd's short PATH. The one place a shell runs: a fixed
// script with no value from the app in it. A hung, failing or silent shell falls back to the system
// PATH plus the set tools' folders, and the answer says which and why (state.get shows it).

import os from 'node:os';
import path from 'node:path';
import type { LoginPathState } from '../shared/contract.ts';
import { capture } from './commands.ts';

export const START = '__KEEL_PATH_BEGIN__';
export const END = '__KEEL_PATH_END__';
export const SCRIPT = `printf '%s%s%s\\n' '${START}' "$PATH" '${END}'`;
export const SYSTEM_PATH: readonly string[] = Object.freeze(['/usr/bin', '/bin', '/usr/sbin', '/sbin']);
export const LOGIN_TIMEOUT_MS = 5_000;

/** The text between the first START and the first END after it, or null. */
export function between(out: string): string | null {
  const s = out.indexOf(START);
  if (s < 0) return null;
  const e = out.indexOf(END, s + START.length);
  return e < 0 ? null : out.slice(s + START.length, e);
}

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** A PATH's absolute entries in order, each once; empty, relative and control-character entries dropped. */
export function entries(p: string): string[] {
  const out: string[] = [];
  for (const e of p.split(':')) {
    if (e === '' || !path.isAbsolute(e) || CONTROL.test(e) || out.includes(e)) continue;
    out.push(e);
  }
  return out;
}

/** The fallback: the system PATH, then the set tools' folders (absolute, each once), with the reason. */
export function fallback(toolDirs: readonly string[], reason: string): LoginPathState {
  return { entries: entries([...SYSTEM_PATH, ...toolDirs].join(':')), source: 'fallback', reason };
}

/** The account's login shell when it is an absolute path, else /bin/zsh. */
export function shellOf(info: { shell: string | null } = os.userInfo()): string {
  return typeof info.shell === 'string' && path.isAbsolute(info.shell) ? info.shell : '/bin/zsh';
}

export interface LoginOptions {
  shell?: string;
  toolDirs?: readonly string[];
  timeoutMs?: number;
  source?: Record<string, string | undefined>;
}

/** Read the PATH from the login shell (-ilc, TERM=dumb, stdin closed, the timeout), or the fallback. */
export async function readLoginPath(opts: LoginOptions = {}): Promise<LoginPathState> {
  const shell = opts.shell ?? shellOf();
  const toolDirs = opts.toolDirs ?? [];
  const timeoutMs = opts.timeoutMs ?? LOGIN_TIMEOUT_MS;
  const source = opts.source ?? process.env;
  const env: Record<string, string> = { SHELL: shell, TERM: 'dumb' };
  for (const k of ['HOME', 'USER', 'LOGNAME']) {
    const v = source[k];
    if (typeof v === 'string' && v !== '') env[k] = v;
  }
  const r = await capture(shell, ['-ilc', SCRIPT], env, timeoutMs);
  if (r.startError !== null) return fallback(toolDirs, `the login shell could not start (${r.startError})`);
  if (r.timedOut) return fallback(toolDirs, `the login shell did not answer within ${timeoutMs / 1000} s`);
  if (r.code !== 0) return fallback(toolDirs, `the login shell exited ${r.code}`);
  const text = between(r.stdout);
  if (text === null) return fallback(toolDirs, 'the login shell printed no PATH');
  const found = entries(text);
  if (found.length === 0) return fallback(toolDirs, "the login shell's PATH has no absolute folder");
  return { entries: found, source: 'login-shell', reason: null };
}
