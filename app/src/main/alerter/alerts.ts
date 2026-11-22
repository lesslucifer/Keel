// The Alerter (local-app.html 'The stack'): one macOS alert per alert line the router writes, never
// the same line twice, across an app restart too. What was shown is recorded in the app's own folder
// (alerts.json: per run, the numbers shown, written atomically by store.ts) before it is shown. At
// this app session's first look at a run (any run: one an earlier session knew too; a take with no
// run spends nothing), the lines not yet shown make one summary alert (the owner's answer of
// 2026-11-22), each recorded shown; a single such line is shown as itself. Its title says Keel was
// closed only at the session's first look at any run; otherwise it says how many are new. After that
// each new line is its own alert. The record keeps the last few runs, so switching back alerts nothing. A click points the
// window at the line's work unit, or at the list when it names none. The shell hands in the sink
// (Electron's Notification, or the file sink W9's test uses) and what a click does.

import path from 'node:path';
import type { Focus as FocusPoint } from '../../shared/contract.ts';
import type { StoredAlert } from '../reader/log.ts';
import { load, writeAtomic } from '../store.ts';
import { plain } from '../text.ts';

export const RECORD_FILE = 'alerts.json';
/** The most numbers kept per run: the router's state keeps 20 alerts, so older numbers never come back. */
export const RECORD_CAP = 64;
/** The most runs the record keeps: a project switched away from and back keeps its run's record. */
export const RUNS_KEPT = 8;

/** What was shown, per run (oldest first): a run's numbers mean nothing in another run. */
export interface AlertRecord {
  version: 2;
  runs: Array<{ run: string; shown: number[] }>;
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
    const { run, shown } = x as Record<string, unknown>;
    if (typeof run !== 'string' || !Array.isArray(shown) || !shown.every(whole)) return null;
    runs.push({ run, shown: [...(shown as number[])] });
  }
  return { version: 2, runs };
}

/** An alert line as an alert: its headline over its lines, every string made plain. */
export function alertOf(a: StoredAlert): ShownAlert {
  return { title: plain(a.headline, 200), body: a.lines.map((l) => plain(l, 400)).join('\n'), wu: a.wu ?? null };
}

/**
 * Why a take is a first look at its run in this app session: 'start', the session's first look at
 * any run (the lines were written while Keel was closed); 'look', a run first looked at while Keel
 * runs (a project switched in, a router restart); null, a run this session has looked at already.
 */
export type Look = 'start' | 'look' | null;

/** The one alert for `k` lines already there at a first look; its click opens the list. Only at the start was Keel closed. */
export function summaryOf(k: number, look: 'start' | 'look'): ShownAlert {
  return { title: look === 'start' ? `${k} alerts while Keel was closed` : `${k} new alerts`, body: 'The list shows each one.', wu: null };
}

/**
 * For the run's alert lines: the record after them, and what to show. At a first look at the run in
 * this app session (`look`), the lines not yet shown make one summary when there are two or more
 * (owner answer 1a), whether or not an earlier session knew the run; after that, each new line is
 * its own alert.
 */
export function decide(record: AlertRecord, run: string | null, alerts: readonly StoredAlert[], look: Look): { record: AlertRecord; show: ShownAlert[] } {
  if (run === null) return { record, show: [] };
  const entry = record.runs.find((r) => r.run === run);
  const shown = entry?.shown ?? [];
  const unseen = alerts.filter((a) => !shown.includes(a.n)).sort((a, b) => a.n - b.n);
  if (unseen.length === 0 && entry !== undefined) return { record, show: [] };
  const next = [...new Set([...shown, ...unseen.map((a) => a.n)])].sort((a, b) => a - b).slice(-RECORD_CAP);
  const runs = [...record.runs.filter((r) => r.run !== run), { run, shown: next }].slice(-RUNS_KEPT);
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
}

export class Alerter {
  private record: AlertRecord | null = null;
  /** The runs this app session has looked at; a take with no run spends nothing. */
  private readonly looked = new Set<string>();
  private readonly opts: AlerterOptions;

  constructor(opts: AlerterOptions) {
    this.opts = opts;
  }

  /** Show what is new among the run's alert lines, recorded first; the numbers shown so far in this run. */
  async take(run: string | null, alerts: readonly StoredAlert[]): Promise<Set<number>> {
    const file = path.join(this.opts.appDir, RECORD_FILE);
    if (this.record === null) this.record = (await load(file, parseRecord, emptyRecord)).value;
    let look: Look = null;
    if (run !== null && !this.looked.has(run)) {
      look = this.looked.size === 0 ? 'start' : 'look';
      this.looked.add(run);
    }
    const { record, show } = decide(this.record, run, alerts, look);
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
