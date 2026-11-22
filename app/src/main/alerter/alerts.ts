// The Alerter (local-app.html 'The stack'): one macOS alert per alert line the router writes, never
// the same line twice, across an app restart too. What was shown is recorded in the app's own folder
// (alerts.json, written atomically by store.ts) before it is shown. One model: per run, the numbers
// shown and the app session that last looked at it, the 16 runs last looked at kept. At this session's
// first look at a run (its entry lacks this session's id: a known run too, or one the record let go),
// the lines not yet shown make one summary alert (the owner's answer of 2026-11-22), each recorded
// shown; a single such line is shown as itself. The summary says Keel was closed only on the session's
// first take (a take with no run counts); otherwise it says how many are new. After that each new line
// is its own alert. A click points the
// window at the line's work unit, or at the list when it names none. The shell hands in the sink
// (Electron's Notification, or the file sink W9's test uses) and what a click does.

import path from 'node:path';
import { randomBytes } from 'node:crypto';
import type { Focus as FocusPoint } from '../../shared/contract.ts';
import type { StoredAlert } from '../reader/log.ts';
import { load, writeAtomic } from '../store.ts';
import { plain } from '../text.ts';

export const RECORD_FILE = 'alerts.json';
/** The most numbers kept per run: the router's state keeps 20 alerts, so older numbers never come back. */
export const RECORD_CAP = 64;
/** The most runs the record keeps, the least recently looked at let go first. It is the alerter's one bound. */
export const RUNS_KEPT = 16;

/**
 * The alerter's one model: per run (least recently looked at first), the numbers shown and the app
 * session that last looked at it. "This session has looked at the run" is exactly "its entry carries
 * this session's id", so letting a run go and having looked at it can never disagree.
 */
export interface AlertRecord {
  version: 2;
  runs: Array<{ run: string; shown: number[]; session?: string }>;
}

/** One app session as decide() sees it: its id, and whether this take is its first (a take with no run included). */
export interface Session {
  id: string;
  first: boolean;
}

/** One alert as the sink shows it: a title, a body, and the work unit a click opens (null: the list). */
export interface ShownAlert {
  title: string;
  body: string;
  wu: string | null;
}

export interface AlertSink {
  show(alert: ShownAlert, onClick: () => void): void;
}

export function emptyRecord(): AlertRecord {
  return { version: 2, runs: [] };
}

const whole = (n: unknown): n is number => typeof n === 'number' && Number.isInteger(n) && n >= 1;

/** The record, or null for any shape the app does not know, version 1's included (store.ts sets it aside). */
export function parseRecord(v: unknown): AlertRecord | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const r = v as Record<string, unknown>;
  if (r.version !== 2 || !Array.isArray(r.runs)) return null;
  const runs: AlertRecord['runs'] = [];
  for (const x of r.runs as unknown[]) {
    if (typeof x !== 'object' || x === null) return null;
    const { run, shown, session } = x as Record<string, unknown>;
    if (typeof run !== 'string' || !Array.isArray(shown) || !shown.every(whole)) return null;
    if (session !== undefined && typeof session !== 'string') return null;
    runs.push({ run, shown: [...(shown as number[])], ...(session === undefined ? {} : { session }) });
  }
  return { version: 2, runs };
}

/** An alert line as an alert: its headline over its lines, every string made plain. */
export function alertOf(a: StoredAlert): ShownAlert {
  return { title: plain(a.headline, 200), body: a.lines.map((l) => plain(l, 400)).join('\n'), wu: a.wu ?? null };
}

/**
 * Why a take is a first look at its run in this app session: 'start', on the session's first take
 * (the lines were written while Keel was closed); 'look', a run first looked at later (a project
 * switched in, a router restart, a run the record had let go); null, a run this session has looked at.
 */
export type Look = 'start' | 'look' | null;

/** The one alert for `k` lines already there at a first look; its click opens the list. Only at the start was Keel closed. */
export function summaryOf(k: number, look: 'start' | 'look'): ShownAlert {
  return { title: look === 'start' ? `${k} alerts while Keel was closed` : `${k} new alerts`, body: 'The list shows each one.', wu: null };
}

/**
 * For the run's alert lines: the record after them, and what to show. A run whose entry does not
 * carry this session's id is at its first look in the session: the lines not yet shown make one
 * summary when there are two or more (owner answer 1a), titled by why (Look); after that each new
 * line is its own alert. Every take stamps the entry with the session and moves it to the newest end
 * (unchanged only when it is already there and nothing is new), so runs are let go least recently looked at.
 */
export function decide(record: AlertRecord, run: string | null, alerts: readonly StoredAlert[], session: Session): { record: AlertRecord; show: ShownAlert[] } {
  if (run === null) return { record, show: [] };
  const entry = record.runs.find((r) => r.run === run);
  const looked = entry?.session === session.id;
  const look: Look = looked ? null : session.first ? 'start' : 'look';
  const shown = entry?.shown ?? [];
  const unseen = alerts.filter((a) => !shown.includes(a.n)).sort((a, b) => a.n - b.n);
  // Nothing new on a run this session looked at: unchanged only when it is already the newest; else it moves there,
  // so the record lets go the run least recently looked at, as RUNS_KEPT says.
  if (unseen.length === 0 && looked && record.runs.at(-1)?.run === run) return { record, show: [] };
  const next = [...new Set([...shown, ...unseen.map((a) => a.n)])].sort((a, b) => a - b).slice(-RECORD_CAP);
  const runs = [...record.runs.filter((r) => r.run !== run), { run, shown: next, session: session.id }].slice(-RUNS_KEPT);
  const show = look !== null && unseen.length > 1 ? [summaryOf(unseen.length, look)] : unseen.map(alertOf);
  return { record: { version: 2, runs }, show };
}

/** Where a click points the window; seq grows on each point, so the window sees a second click on the same unit. */
export class Focus {
  private current: FocusPoint | null = null;

  point(wu: string | null): void {
    this.current = { wu, seq: (this.current?.seq ?? 0) + 1 };
  }

  get(): FocusPoint | null {
    return this.current === null ? null : { ...this.current };
  }
}

export interface AlerterOptions {
  /** The app's own folder (Electron's userData). */
  appDir: string;
  sink: AlertSink;
  /** What a click does: point the focus at `wu` (null: the list) and open the window. */
  onClick(wu: string | null): void;
  /** This app session's id; a fresh random one when not given. */
  session?: string;
}

export class Alerter {
  private record: AlertRecord | null = null;
  /** This app session's id, stamped on each run it looks at (the record holds the rest). */
  private readonly session: string;
  /** Whether this session has taken once (a take with no run counts: only the first take is the start). */
  private took = false;
  private readonly opts: AlerterOptions;

  constructor(opts: AlerterOptions) {
    this.opts = opts;
    this.session = opts.session ?? `${Date.now().toString(36)}-${randomBytes(6).toString('hex')}`;
  }

  /** Show what is new among the run's alert lines, recorded first; the numbers shown so far in this run. */
  async take(run: string | null, alerts: readonly StoredAlert[]): Promise<Set<number>> {
    const file = path.join(this.opts.appDir, RECORD_FILE);
    if (this.record === null) this.record = (await load(file, parseRecord, emptyRecord)).value;
    const first = !this.took;
    this.took = true;
    const { record, show } = decide(this.record, run, alerts, { id: this.session, first });
    if (record !== this.record) {
      try {
        await writeAtomic(file, record);
      } catch {
        // Not recorded: still shown (a missed alert costs more than one shown again after a restart).
      }
      this.record = record;
    }
    for (const a of show) {
      try {
        this.opts.sink.show(a, () => this.opts.onClick(a.wu));
      } catch {
        // A sink that fails shows nothing; the line stays in the list.
      }
    }
    return new Set(record.runs.find((r) => r.run === run)?.shown ?? []);
  }
}

/** The part of Electron's Notification the sink uses. */
export interface Clickable {
  on(event: 'click' | 'close' | 'failed', listener: () => void): unknown;
  show(): void;
}

/** The most notifications the sink holds at once. */
export const HELD_CAP = 20;

/**
 * A sink over notifications `make` builds (the shell's Electron Notification): each is held until it
 * is clicked, closed or fails, since one that is collected before its click loses the click; past
 * HELD_CAP the oldest is let go. A make or show that throws shows nothing.
 */
export function heldSink(make: (alert: ShownAlert) => Clickable): AlertSink & { held(): number } {
  const held = new Set<Clickable>();
  return {
    show(alert, onClick) {
      try {
        const n = make(alert);
        held.add(n);
        if (held.size > HELD_CAP) held.delete(held.values().next().value as Clickable);
        n.on('click', () => {
          held.delete(n);
          onClick();
        });
        n.on('close', () => held.delete(n));
        n.on('failed', () => held.delete(n));
        n.show();
      } catch {
        // Shown nothing; the line stays in the list.
      }
    },
    held: () => held.size,
  };
}
