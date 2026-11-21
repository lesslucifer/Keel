// The fixed commands: the only git the main part runs. Each is a named shape with typed slots; the
// line always starts with the locked flags (no fsmonitor, no optional locks); a diff, show or log -p
// shape gets --no-ext-diff --no-textconv; git runs with no shell, stdin closed, and an environment
// built from nothing but the login PATH and five account variables, so no inherited GIT_* reaches it.
// capture() is the one way the main part spawns anything (login-path.ts uses it too).

import { spawn } from 'node:child_process';
import path from 'node:path';

export const LOCKED: readonly string[] = Object.freeze(['-c', 'core.fsmonitor=false', '--no-optional-locks']);
export const NO_EXTERNAL: readonly string[] = Object.freeze(['--no-ext-diff', '--no-textconv']);

/** The variables a child takes from the app's environment, besides PATH (which is the login PATH). */
export const ENV_KEYS: readonly string[] = Object.freeze(['HOME', 'USER', 'LOGNAME', 'LANG', 'TMPDIR']);

/** A git read's timeout. */
export const READ_TIMEOUT_MS = 10_000;

/** The most output kept from each stream; the rest is dropped. */
export const OUTPUT_CAP = 1024 * 1024;

type Token = string | { readonly slot: 'project' };
const PROJECT: Token = Object.freeze({ slot: 'project' as const });

/** The W7 commands, each a git line with its slots. */
export const COMMANDS: Readonly<Record<string, readonly Token[]>> = Object.freeze(
  Object.assign(Object.create(null) as Record<string, readonly Token[]>, {
    toplevel: Object.freeze(['-C', PROJECT, 'rev-parse', '--show-toplevel']),
    'common-dir': Object.freeze(['-C', PROJECT, 'rev-parse', '--path-format=absolute', '--git-common-dir']),
    'has-config': Object.freeze(['-C', PROJECT, 'cat-file', '-e', 'HEAD:.keel/config.toml']),
  }),
);

export type CommandName = 'toplevel' | 'common-dir' | 'has-config';

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

function projectOk(v: unknown): v is string {
  return typeof v === 'string' && path.isAbsolute(v) && !CONTROL.test(v);
}

/** The locked flags, then `shape`, with --no-ext-diff --no-textconv right after a diff, show or log -p subcommand. */
export function lock(shape: readonly string[]): string[] {
  let i = 0;
  while (i < shape.length && shape[i] === '-C') i += 2;
  const sub = shape[i];
  const rest = shape.slice(i + 1);
  const patch = sub === 'diff' || sub === 'show' || (sub === 'log' && rest.some((a) => a === '-p' || a === '--patch' || a === '-u'));
  if (!patch) return [...LOCKED, ...shape];
  return [...LOCKED, ...shape.slice(0, i + 1), ...NO_EXTERNAL, ...rest];
}

/** The full git argv for a named command and its slots; throws on an unknown name or a bad slot. */
export function argv(name: string, slots: Record<string, string>): string[] {
  if (!Object.hasOwn(COMMANDS, name)) throw new Error(`unknown fixed command: ${JSON.stringify(name)}`);
  const shape = COMMANDS[name] as readonly Token[];
  const wanted = [...new Set(shape.flatMap((t) => (typeof t === 'string' ? [] : [t.slot])))].sort();
  const given = Object.keys(slots).sort();
  if (wanted.length !== given.length || wanted.some((k, i) => k !== given[i])) throw new Error(`${name} takes the slots ${wanted.join(', ')}`);
  if (!projectOk(slots.project)) throw new Error(`${name}: the project must be an absolute path with no control character`);
  return lock(shape.map((t) => (typeof t === 'string' ? t : (slots[t.slot] as string))));
}

/** A child's environment: PATH from `pathEntries`, and only ENV_KEYS (set and not empty) from `source`. */
export function childEnv(pathEntries: readonly string[], source: Record<string, string | undefined> = process.env): Record<string, string> {
  const env: Record<string, string> = { PATH: pathEntries.join(':') };
  for (const k of ENV_KEYS) {
    const v = source[k];
    if (typeof v === 'string' && v !== '') env[k] = v;
  }
  return env;
}

export interface Captured {
  /** The exit code; null when it was killed or never started. */
  code: number | null;
  stdout: string;
  stderr: string;
  timedOut: boolean;
  /** Why it never started (ENOENT, EACCES, ...), else null. */
  startError: string | null;
}

/** How long the pipes may stay open once the program itself has exited, for its last output to arrive. */
export const EXIT_GRACE_MS = 200;

/** The longest the answer waits, after the timeout's group kill, for every process in the group to be gone. */
export const GROUP_GONE_MS = 2_000;

/** Whether no process is left in the process group `pgid`. EPERM is not gone: macOS answers it while a killed member is still exiting. */
function groupGone(pgid: number): boolean {
  try {
    process.kill(-pgid, 0);
    return false;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code !== 'EPERM';
  }
}

/**
 * Run `file` with `args` and exactly `env`: never a shell, stdin closed, cwd /, each stream capped.
 * The answer comes from the program's own exit, never from the pipes alone: a background job or a
 * process that left the group may hold them open for ever. Once the program has exited by itself,
 * the pipes get EXIT_GRACE_MS for its last output, then are closed and it is answered with its code;
 * nothing is killed after that exit (what it left running is the owner's). If the program has not
 * exited at the timeout, its whole process group - a shell's children too - is killed, and the
 * answer waits (at most GROUP_GONE_MS) until the group is gone. Never throws for the program's own failure.
 */
export function capture(file: string, args: readonly string[], env: Record<string, string>, timeoutMs: number): Promise<Captured> {
  return new Promise((resolve) => {
    const out: Buffer[] = [];
    const err: Buffer[] = [];
    const size = { out: 0, err: 0 };
    let timedOut = false;
    let settled = false;
    let grace: NodeJS.Timeout | undefined;
    const child = spawn(file, args, { shell: false, stdio: ['ignore', 'pipe', 'pipe'], env, cwd: '/', detached: true });
    const done = (code: number | null, startError: string | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(grace);
      // Whatever still holds the pipes, the app stops reading them.
      child.stdout.destroy();
      child.stderr.destroy();
      resolve({ code, startError, timedOut, stdout: Buffer.concat(out).toString('utf8'), stderr: Buffer.concat(err).toString('utf8') });
    };
    child.stdout.on('data', (b: Buffer) => {
      if (size.out < OUTPUT_CAP) out.push(b.subarray(0, OUTPUT_CAP - size.out));
      size.out += b.length;
    });
    child.stderr.on('data', (b: Buffer) => {
      if (size.err < OUTPUT_CAP) err.push(b.subarray(0, OUTPUT_CAP - size.err));
      size.err += b.length;
    });
    const timer = setTimeout(() => {
      timedOut = true;
      try {
        if (child.pid !== undefined) process.kill(-child.pid, 'SIGKILL');
      } catch {
        child.kill('SIGKILL');
      }
    }, timeoutMs);
    child.on('error', (e: NodeJS.ErrnoException) => done(null, e.code ?? e.message));
    child.on('exit', (code) => {
      if (timedOut) {
        // Killed with its group: answer once no process is left in it (a killed member may still be exiting).
        const pgid = child.pid;
        const until = Date.now() + GROUP_GONE_MS;
        const check = () => {
          if (pgid === undefined || groupGone(pgid) || Date.now() >= until) done(null, null);
          else setTimeout(check, 10);
        };
        check();
        return;
      }
      // Exited by itself: no group kill from here on; the pipes get a short grace, then its own code answers.
      clearTimeout(timer);
      grace = setTimeout(() => setImmediate(() => done(code, null)), EXIT_GRACE_MS);
    });
    // After a timeout only the exit's wait above answers: the pipes close as the group dies, before every member is gone.
    child.on('close', (code) => {
      if (!timedOut) done(code, null);
    });
  });
}

export interface RunOptions {
  /** The PATH entries git runs with (the login PATH). */
  path: readonly string[];
  timeoutMs?: number;
  /** Where the account variables come from; the app's own environment by default. */
  source?: Record<string, string | undefined>;
}

/** Run a named fixed command with `git` (an absolute path). Never throws for git's own failure. */
export async function run(git: string, name: CommandName, slots: Record<string, string>, opts: RunOptions): Promise<Captured> {
  if (typeof git !== 'string' || !path.isAbsolute(git)) throw new Error('git must be an absolute path');
  return capture(git, argv(name, slots), childEnv(opts.path, opts.source), opts.timeoutMs ?? READ_TIMEOUT_MS);
}
