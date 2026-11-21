// The fixed commands: the only git the main part runs. Each is a named shape with typed slots; the
// line always starts with the locked flags (no fsmonitor, no optional locks, no lazy fetch: in a
// partial clone a read never fetches a missing object into the project's .git). A git older than
// 2.45 has no --no-lazy-fetch and refuses the line (exit 129): such a git runs the line without it
// (LOCKED_BEFORE_2_45), and only on a project with no promisor setting, so it has nothing to fetch
// (checkProject decides); a git older than 2.15 (no --no-optional-locks) is refused. A diff, show or
// log -p shape gets --no-ext-diff --no-textconv. git runs with no shell, stdin closed, and an
// environment built from nothing but the login PATH, LC_ALL=C (its messages in English, whatever the
// Mac's language: the app reads one of them) and five account variables, so no inherited GIT_* reaches it.
// capture() is the one way the main part spawns anything (login-path.ts uses it too).

import { spawn, type ChildProcess } from 'node:child_process';
import { Socket } from 'node:net';
import path from 'node:path';

export const LOCKED: readonly string[] = Object.freeze(['-c', 'core.fsmonitor=false', '--no-optional-locks', '--no-lazy-fetch']);

/** The locked flags a git older than 2.45 takes: no --no-lazy-fetch, which it does not know. */
export const LOCKED_BEFORE_2_45: readonly string[] = Object.freeze(['-c', 'core.fsmonitor=false', '--no-optional-locks']);

/** The first git with --no-lazy-fetch (2.45.0); an older one may read only a project that is not a partial clone. */
export const MIN_GIT = '2.45';

/** The oldest git the app runs: --no-optional-locks came in 2.15; every other option and output it reads is older. */
export const OLDEST_GIT = '2.15';

/** The settings that make a repository a partial clone (git config names, lower-cased as --get-regexp matches them). */
export const PROMISOR_KEYS = '^(extensions\\.partialclone|remote\\..*\\.(promisor|partialclonefilter))$';

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
    // No --path-format (git 2.31): checkProject resolves a relative answer against the project, as git means it.
    'common-dir': Object.freeze(['-C', PROJECT, 'rev-parse', '--git-common-dir']),
    // ls-tree, not cat-file -e: the mode shows a regular file, not a folder, a link or a submodule.
    'has-config': Object.freeze(['-C', PROJECT, 'ls-tree', '--full-tree', 'HEAD', '--', '.keel/config.toml']),
    // Run only by a git older than 2.45, to learn whether the project is a partial clone; reads no object.
    'promisor-config': Object.freeze(['-C', PROJECT, 'config', '--get-regexp', PROMISOR_KEYS]),
  }),
);

export type CommandName = 'toplevel' | 'common-dir' | 'has-config' | 'promisor-config';

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

function projectOk(v: unknown): v is string {
  return typeof v === 'string' && path.isAbsolute(v) && !CONTROL.test(v);
}

/**
 * The locked flags (LOCKED_BEFORE_2_45 for a git older than 2.45), then `shape`, with --no-ext-diff
 * --no-textconv right after a diff, show or log -p subcommand.
 */
export function lock(shape: readonly string[], before245 = false): string[] {
  const locked = before245 ? LOCKED_BEFORE_2_45 : LOCKED;
  let i = 0;
  while (i < shape.length && shape[i] === '-C') i += 2;
  const sub = shape[i];
  const rest = shape.slice(i + 1);
  const patch = sub === 'diff' || sub === 'show' || (sub === 'log' && rest.some((a) => a === '-p' || a === '--patch' || a === '-u'));
  if (!patch) return [...locked, ...shape];
  return [...locked, ...shape.slice(0, i + 1), ...NO_EXTERNAL, ...rest];
}

/** The full git argv for a named command and its slots; throws on an unknown name or a bad slot. */
export function argv(name: string, slots: Record<string, string>, before245 = false): string[] {
  if (!Object.hasOwn(COMMANDS, name)) throw new Error(`unknown fixed command: ${JSON.stringify(name)}`);
  const shape = COMMANDS[name] as readonly Token[];
  const wanted = [...new Set(shape.flatMap((t) => (typeof t === 'string' ? [] : [t.slot])))].sort();
  const given = Object.keys(slots).sort();
  if (wanted.length !== given.length || wanted.some((k, i) => k !== given[i])) throw new Error(`${name} takes the slots ${wanted.join(', ')}`);
  if (!projectOk(slots.project)) throw new Error(`${name}: the project must be an absolute path with no control character`);
  return lock(
    shape.map((t) => (typeof t === 'string' ? t : (slots[t.slot] as string))),
    before245,
  );
}

/** A child's environment: PATH from `pathEntries`, and only ENV_KEYS (set and not empty) from `source`. */
export function childEnv(pathEntries: readonly string[], source: Record<string, string | undefined> = process.env): Record<string, string> {
  const env: Record<string, string> = { PATH: pathEntries.join(':'), LC_ALL: 'C' };
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
 * the pipes get EXIT_GRACE_MS for its last output, then it is answered with its code; nothing is
 * killed after that exit (what it left running is the owner's), and the pipes stay open and are
 * drained, unread, until whatever holds them closes them, so a job that writes later is not killed
 * by SIGPIPE. If the program has not exited at the timeout, its whole process group - a shell's
 * children too - is killed, and the answer waits (at most GROUP_GONE_MS) until the group is gone.
 * A program that cannot start (ENOENT, EACCES, EMFILE, ...) is answered with the reason. Never
 * throws or rejects for the program's own failure.
 */
export function capture(file: string, args: readonly string[], env: Record<string, string>, timeoutMs: number): Promise<Captured> {
  return new Promise((resolve) => {
    const out: Buffer[] = [];
    const err: Buffer[] = [];
    const size = { out: 0, err: 0 };
    let timedOut = false;
    let exited = false;
    let settled = false;
    let timer: NodeJS.Timeout | undefined;
    let grace: NodeJS.Timeout | undefined;
    let child: ChildProcess | undefined;
    const done = (code: number | null, startError: string | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(grace);
      // The pipes stay open and keep draining (data after this is dropped); they no longer keep the app's loop alive.
      for (const stream of [child?.stdout, child?.stderr]) {
        if (stream instanceof Socket) stream.unref();
      }
      child?.unref();
      resolve({ code, startError, timedOut, stdout: Buffer.concat(out).toString('utf8'), stderr: Buffer.concat(err).toString('utf8') });
    };
    try {
      child = spawn(file, args, { shell: false, stdio: ['ignore', 'pipe', 'pipe'], env, cwd: '/', detached: true });
    } catch (e) {
      done(null, (e as NodeJS.ErrnoException).code ?? String(e));
      return;
    }
    const started = child;
    // First, before anything that can throw: when spawn fails (EMFILE, ENFILE) the child has no pipes and only this event says why.
    started.on('error', (e: NodeJS.ErrnoException) => done(null, e.code ?? e.message));
    if (!started.stdout || !started.stderr) return;
    started.stdout.on('data', (b: Buffer) => {
      if (settled) return;
      if (size.out < OUTPUT_CAP) out.push(b.subarray(0, OUTPUT_CAP - size.out));
      size.out += b.length;
    });
    started.stderr.on('data', (b: Buffer) => {
      if (settled) return;
      if (size.err < OUTPUT_CAP) err.push(b.subarray(0, OUTPUT_CAP - size.err));
      size.err += b.length;
    });
    timer = setTimeout(() => {
      // One more turn first: an exit that arrived while the loop was blocked is read before anything is killed.
      setImmediate(() => {
        if (exited || settled) return;
        timedOut = true;
        try {
          if (started.pid !== undefined) process.kill(-started.pid, 'SIGKILL');
        } catch {
          started.kill('SIGKILL');
        }
      });
    }, timeoutMs);
    started.on('exit', (code) => {
      exited = true;
      if (timedOut) {
        // Killed with its group: answer once no process is left in it (a killed member may still be exiting).
        const pgid = started.pid;
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
    started.on('close', (code) => {
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
  /** A git older than 2.45: run with LOCKED_BEFORE_2_45. Only checkProject sets it, after its promisor check. */
  before245?: boolean;
}

/** The locked flags an older git may not know, newest first. */
const NEWER_FLAGS = ['--no-lazy-fetch', '--no-optional-locks'] as const;

/**
 * The locked flag git refused, if it did: exit 129 (usage) with the flag named in stderr. Only the
 * flag is matched, in any case and in any language around it: a git before 2.45 translates its
 * "unknown option" line (LC_ALL=C makes it English, but a git built without that locale, or one
 * that ignores it, may not), and git 2.15-2.16 capitalised it.
 */
export function refusedFlag(r: Captured): (typeof NEWER_FLAGS)[number] | null {
  if (r.code !== 129) return null;
  return NEWER_FLAGS.find((flag) => new RegExp(`${flag}(?![\\w-])`, 'i').test(r.stderr)) ?? null;
}

/** Run a named fixed command with `git` (an absolute path). Never throws for git's own failure. */
export async function run(git: string, name: CommandName, slots: Record<string, string>, opts: RunOptions): Promise<Captured> {
  if (typeof git !== 'string' || !path.isAbsolute(git)) throw new Error('git must be an absolute path');
  return capture(git, argv(name, slots, opts.before245 === true), childEnv(opts.path, opts.source), opts.timeoutMs ?? READ_TIMEOUT_MS);
}
