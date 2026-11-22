// The router's update log, read (local-app.html 'Two sources'; the writer is scripts/router-app.sh,
// E-APPLIST.W1). Two files in the project's main checkout, .router/app/: log.jsonl, one numbered
// JSON line per change, and state.json, every value as it stands with the number of the last line
// folded in. The app only reads them, and never follows a link there (the writer refuses links too).
//
// Applying a line: a new run drops every router value and reads the state; the next number is
// applied; a number seen is skipped; a missing number reads the state once; a half-written line
// waits for its end; a line with an unknown key moves the number and changes nothing; anything that
// is not a line is skipped. On start the state is read once, then the log is followed by whole lines
// from its start (every line the state already holds is a number seen). A log shorter than what was
// read, or a new file (the writer replaces it by a rename past 2,000 lines and at start), is read
// from its start. Heard: the newest line's own time is under SILENCE_MS old on the app's clock (the
// watchdog writes a heartbeat each tick, every 5 minutes, so 15 minutes is three silent ticks).

import { lstat, open, readFile } from 'node:fs/promises';
import path from 'node:path';
import type { RouterView } from '../../shared/contract.ts';
import { codePoints } from '../text.ts';

/** Three silent watchdog ticks. */
export const SILENCE_MS = 15 * 60 * 1000;

/** The alerts the writer keeps in its state, and so the reader. */
export const ALERTS_KEPT = 20;

/** The most of the log or the state the reader reads at once: the writer keeps 2,000 lines of at most 1,024 bytes. */
export const READ_CAP = 4 * 1024 * 1024;

const WU = 'E-[A-Z0-9]{1,16}\\.W[0-9]{1,3}';
const ID = '[a-z0-9][a-z0-9-]{0,40}';
/** The keys router-app.sh accepts, its key_ok. */
export const KEY = new RegExp(`^(?:router|facts|wu/${WU}/(?:stage|conflict)|epic/E-[A-Z0-9]{1,16}/paused|question/${ID}|worker/${ID}(?:/report)?)$`);
export const WU_ID = new RegExp(`^${WU}$`);
/** A run id as router-app.sh mints it: <router-name>.<YYYYMMDD-HHMMSS>.<pid>. */
const RUN = /^[a-z0-9][a-z0-9-]{0,31}\.[0-9]{8}-[0-9]{6}\.[0-9]{1,10}$/;
const LINK = /^keel:\/\/[A-Za-z0-9._/-]{1,200}$/;

/** An alert as the writer keeps it: its number, its time, and its value (wu and link only when given). */
export interface StoredAlert {
  n: number;
  at: string;
  headline: string;
  lines: string[];
  wu?: string;
  link?: string;
}

export type AlertValue = Omit<StoredAlert, 'n' | 'at'>;

interface Head {
  run: string;
  n: number;
  at: string;
}

/** One line of the log, as the reader takes it; 'skip' is a well-formed line it does not know (a newer writer's). */
export type Line =
  | (Head & { op: 'start' })
  | (Head & { op: 'set'; key: string; value: unknown })
  | (Head & { op: 'del'; key: string })
  | (Head & { op: 'alert'; value: AlertValue })
  | (Head & { op: 'skip' });

/** What the reader holds: the run, the last number folded in and its time, the values, the alerts. */
export interface Fold {
  run: string | null;
  n: number;
  at: string | null;
  values: Record<string, unknown>;
  alerts: StoredAlert[];
}

export function emptyFold(): Fold {
  return { run: null, n: 0, at: null, values: {}, alerts: [] };
}

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** A string of `lo` to `hi` code points: the writers count code points (router-app.sh's jq length, notify-owner.py's len). */
const str = (v: unknown, lo: number, hi: number): v is string => typeof v === 'string' && codePoints(v) >= lo && codePoints(v) <= hi;
const count = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v) && v >= 1;

/** An alert's value as router-app.sh's alert_ok takes it, or null. */
export function alertValue(v: unknown): AlertValue | null {
  if (!isObject(v)) return null;
  if (Object.keys(v).some((k) => !['headline', 'lines', 'wu', 'link'].includes(k))) return null;
  if (!str(v.headline, 1, 80)) return null;
  if (!Array.isArray(v.lines) || v.lines.length < 1 || v.lines.length > 2 || !v.lines.every((l) => str(l, 1, 200))) return null;
  if ('wu' in v && !(typeof v.wu === 'string' && WU_ID.test(v.wu))) return null;
  if ('link' in v && !(typeof v.link === 'string' && LINK.test(v.link))) return null;
  const out: AlertValue = { headline: v.headline, lines: [...(v.lines as string[])] };
  if (typeof v.wu === 'string') out.wu = v.wu;
  if (typeof v.link === 'string') out.link = v.link;
  return out;
}

function head(v: Record<string, unknown>): Head | null {
  if (typeof v.run !== 'string' || !RUN.test(v.run) || !count(v.n) || typeof v.at !== 'string') return null;
  return { run: v.run, n: v.n, at: v.at };
}

/** One line of text as a Line, or null for anything that is not one. */
export function parseLine(text: string): Line | null {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  if (!isObject(v)) return null;
  const h = head(v);
  if (h === null) return null;
  switch (v.op) {
    case 'start':
      return { ...h, op: 'start' };
    case 'set':
      if (typeof v.key !== 'string' || !('value' in v) || v.value === null) return null;
      return KEY.test(v.key) ? { ...h, op: 'set', key: v.key, value: v.value } : { ...h, op: 'skip' };
    case 'del':
      if (typeof v.key !== 'string') return null;
      return KEY.test(v.key) ? { ...h, op: 'del', key: v.key } : { ...h, op: 'skip' };
    case 'alert': {
      const value = alertValue(v.value);
      return value === null ? null : { ...h, op: 'alert', value };
    }
    default:
      return typeof v.op === 'string' ? { ...h, op: 'skip' } : null;
  }
}

/** state.json as the writer folds it, unknown keys and bad alerts dropped; null for any other shape. */
export function parseState(v: unknown): Fold | null {
  if (!isObject(v)) return null;
  const h = head(v);
  if (h === null || !isObject(v.values) || !Array.isArray(v.alerts)) return null;
  const values: Record<string, unknown> = {};
  for (const [k, x] of Object.entries(v.values)) if (KEY.test(k) && x !== null) values[k] = x;
  const alerts: StoredAlert[] = [];
  for (const a of v.alerts) {
    if (!isObject(a) || !count(a.n) || typeof a.at !== 'string') continue;
    const { n, at, ...rest } = a;
    const value = alertValue(rest);
    if (value !== null) alerts.push({ n, at, ...value });
  }
  return { run: h.run, n: h.n, at: h.at, values, alerts: alerts.slice(-ALERTS_KEPT) };
}

/** Apply one line to `fold`: the new fold, and whether the state must be read (a new run, a missing number). */
export function applyLine(fold: Fold, line: Line): { fold: Fold; needState: boolean } {
  if (line.run !== fold.run) return { fold: emptyFold(), needState: true };
  if (line.n <= fold.n) return { fold, needState: false };
  if (line.n !== fold.n + 1) return { fold, needState: true };
  const next: Fold = { ...fold, n: line.n, at: line.at };
  switch (line.op) {
    case 'start':
      next.values = {};
      next.alerts = [];
      break;
    case 'set':
      next.values = { ...fold.values, [line.key]: line.value };
      break;
    case 'del': {
      const values = { ...fold.values };
      delete values[line.key];
      next.values = values;
      break;
    }
    case 'alert':
      next.alerts = [...fold.alerts, { n: line.n, at: line.at, ...line.value }].slice(-ALERTS_KEPT);
      break;
    case 'skip':
      break;
  }
  return { fold: next, needState: false };
}

/** Apply `lines` in order from `fold` until one needs the state: the fold so far, and whether one did (gap). */
export function followLines(fold: Fold, lines: readonly Line[]): { fold: Fold; gap: boolean } {
  let f = fold;
  for (const line of lines) {
    const r = applyLine(f, line);
    if (r.needState) return { fold: f, gap: true };
    f = r.fold;
  }
  return { fold: f, gap: false };
}

/**
 * Apply `lines` in order from `start`. On the first need for the state, `readState` is called once
 * (null: no state, every value at its default) and the lines are followed again on it from the
 * start, every line it holds a number seen, as the reader reads the log again from its start; a
 * second need is left for the next read (needState true), so one read never loops.
 */
export function foldLines(start: Fold, lines: readonly Line[], readState: () => Fold | null): { fold: Fold; needState: boolean; stateRead: boolean } {
  const first = followLines(start, lines);
  if (!first.gap) return { fold: first.fold, needState: false, stateRead: false };
  const again = followLines(readState() ?? emptyFold(), lines);
  return { fold: again.fold, needState: again.gap, stateRead: true };
}

/** The whole lines at the start of `buf` and the bytes they take; a trailing half line is left. */
export function wholeLines(buf: Buffer): { lines: string[]; used: number } {
  const end = buf.lastIndexOf(0x0a);
  if (end < 0) return { lines: [], used: 0 };
  return { lines: buf.subarray(0, end).toString('utf8').split('\n'), used: end + 1 };
}

/** Whether a line at `at` (else, when it does not parse, at the file time `fileMs`) is under `silenceMs` old at `now`. */
export function heardAt(at: string | null, now: number, fileMs: number | null, silenceMs: number = SILENCE_MS): boolean {
  if (at === null) return false;
  const parsed = Date.parse(at);
  const t = Number.isFinite(parsed) ? parsed : fileMs;
  return t !== null && now - t < silenceMs;
}

export interface RouterLogOptions {
  /** The project's main checkout. */
  root: string;
  now?: () => number;
  silenceMs?: number;
}

type Read = { kind: 'missing' } | { kind: 'link' } | { kind: 'ok'; bytes: Buffer; ino: number; size: number; mtimeMs: number } | { kind: 'error'; why: string };

/** lstat `file` and read it from `from`, at most READ_CAP; a link, or anything but a regular file, is not read. */
async function readFrom(file: string, from: number): Promise<Read> {
  let st;
  try {
    st = await lstat(file);
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'ENOENT' ? { kind: 'missing' } : { kind: 'error', why: `${path.basename(file)} unreadable` };
  }
  if (st.isSymbolicLink()) return { kind: 'link' };
  if (!st.isFile()) return { kind: 'error', why: `${path.basename(file)} is not a file` };
  const start = Math.min(from, st.size);
  const length = Math.min(st.size - start, READ_CAP);
  const bytes = Buffer.alloc(length);
  try {
    const fh = await open(file, 'r');
    try {
      let got = 0;
      while (got < length) {
        const { bytesRead } = await fh.read(bytes, got, length - got, start + got);
        if (bytesRead === 0) break;
        got += bytesRead;
      }
      return { kind: 'ok', bytes: bytes.subarray(0, got), ino: st.ino, size: st.size, mtimeMs: st.mtimeMs };
    } finally {
      await fh.close();
    }
  } catch {
    return { kind: 'error', why: `${path.basename(file)} unreadable` };
  }
}

/** Whether `p` is a link (a missing path is not). */
async function isLink(p: string): Promise<boolean> {
  try {
    return (await lstat(p)).isSymbolicLink();
  } catch {
    return false;
  }
}

/** The reader of one project's update log: refresh() reads what is new, view() and alerts() show the fold. */
export class RouterLog {
  readonly stats = { stateReads: 0 };
  private fold: Fold = emptyFold();
  private problem: string | null = null;
  private needState = true;
  private offset = 0;
  private ino: number | null = null;
  private fileMs: number | null = null;
  private readonly dir: string;
  private readonly now: () => number;
  private readonly silenceMs: number;

  constructor(opts: RouterLogOptions) {
    this.dir = path.join(opts.root, '.router', 'app');
    this.now = opts.now ?? Date.now;
    this.silenceMs = opts.silenceMs ?? SILENCE_MS;
  }

  /** Read what is new; whether the fold, or the problem, changed. */
  async refresh(): Promise<boolean> {
    const before = JSON.stringify([this.fold, this.problem]);
    await this.read();
    return JSON.stringify([this.fold, this.problem]) !== before;
  }

  private async read(): Promise<void> {
    if ((await isLink(path.dirname(this.dir))) || (await isLink(this.dir))) return this.stop('.router or .router/app is a link: not read');
    let stateRead = false;
    if (this.needState) {
      if ((await this.readState()) === 'stop') return;
      stateRead = true;
    }
    for (;;) {
      const log = await readFrom(path.join(this.dir, 'log.jsonl'), this.offset);
      if (log.kind === 'missing') return;
      if (log.kind === 'link') return this.stop('log.jsonl is a link: not read');
      if (log.kind === 'error') return this.stop(log.why);
      if (log.size < this.offset || (this.offset !== 0 && log.ino !== this.ino)) {
        // A shorter file, or a new one: read it again from its start.
        this.ino = log.ino;
        this.offset = 0;
        continue;
      }
      this.ino = log.ino;
      this.fileMs = log.mtimeMs;
      const { lines, used } = wholeLines(log.bytes);
      const { fold, gap } = followLines(this.fold, lines.map(parseLine).filter((l): l is Line => l !== null));
      this.fold = fold;
      if (!gap) {
        this.offset += used;
        return;
      }
      if (stateRead) {
        // A second gap after the state was read: left for the next refresh, which reads the state again.
        this.needState = true;
        this.offset = 0;
        return;
      }
      // A new run or a missing number: read the state once, then follow the log again from its start.
      if ((await this.readState()) === 'stop') return;
      stateRead = true;
    }
  }

  /** Read state.json into the fold; 'stop' when it is a link or not the writer's shape. */
  private async readState(): Promise<'ok' | 'stop'> {
    this.stats.stateReads += 1;
    const file = path.join(this.dir, 'state.json');
    const st = await readFrom(file, 0);
    this.needState = false;
    this.offset = 0;
    if (st.kind === 'missing') {
      this.fold = emptyFold();
      this.problem = null;
      return 'ok';
    }
    if (st.kind === 'link') return this.stopped('state.json is a link: not read');
    if (st.kind === 'error') return this.stopped(st.why);
    let parsed: Fold | null = null;
    try {
      parsed = parseState(JSON.parse(st.bytes.toString('utf8')));
    } catch {
      parsed = null;
    }
    if (parsed === null) return this.stopped('state.json is not the update log\'s state');
    this.fold = parsed;
    this.problem = null;
    this.fileMs = st.mtimeMs;
    return 'ok';
  }

  private stopped(why: string): 'stop' {
    this.stop(why);
    return 'stop';
  }

  /** Nothing is read: every router field at its default, and why. The next refresh starts again from the state. */
  private stop(why: string): void {
    this.fold = emptyFold();
    this.problem = why;
    this.needState = true;
    this.offset = 0;
    this.ino = null;
  }

  /** The router's part of the view, heard judged at this moment. */
  view(): RouterView {
    const f = this.fold;
    return {
      run: f.run,
      n: f.n,
      values: structuredClone(f.values),
      heard: f.run !== null && heardAt(f.at, this.now(), this.fileMs, this.silenceMs),
      lastAt: f.at,
      problem: this.problem,
    };
  }

  /** The router's last alerts, oldest first, as its state keeps them. */
  alerts(): StoredAlert[] {
    return structuredClone(this.fold.alerts);
  }
}
