// The Alerter (local-app.html 'The stack'): one macOS alert per alert line the router writes, never
// the same line twice, across an app restart too. What was shown is recorded in the app's own folder
// (alerts.json: the run and the numbers shown, written atomically by store.ts) before it is shown.
// At the app's start, lines written while it was closed make one summary alert (the owner's answer
// of 2026-11-22), each recorded shown; a single such line is shown as itself. After that each new
// line is its own alert. A new run (the router restarted) starts a new record. A click points the
// window at the line's work unit, or at the list when it names none. The shell hands in the sink
// (Electron's Notification, or the file sink W9's test uses) and what a click does.

import path from 'node:path';
import type { Focus as FocusPoint } from '../../shared/contract.ts';
import type { StoredAlert } from '../reader/log.ts';
import { load, writeAtomic } from '../store.ts';
import { plain } from '../text.ts';

export const RECORD_FILE = 'alerts.json';
/** The most numbers the record keeps: the router's state keeps 20 alerts, so older numbers never come back. */
export const RECORD_CAP = 64;

export interface AlertRecord {
  version: 1;
  run: string | null;
  shown: number[];
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
  return { version: 1, run: null, shown: [] };
}

/** The record, or null for any shape the app does not know (store.ts sets it aside). */
export function parseRecord(v: unknown): AlertRecord | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const r = v as Record<string, unknown>;
  if (r.version !== 1 || !(r.run === null || typeof r.run === 'string') || !Array.isArray(r.shown)) return null;
  if (!r.shown.every((n) => typeof n === 'number' && Number.isInteger(n) && n >= 1)) return null;
  return { version: 1, run: r.run, shown: [...(r.shown as number[])] };
}

/** An alert line as an alert: its headline over its lines, every string made plain. */
export function alertOf(a: StoredAlert): ShownAlert {
  return { title: plain(a.headline, 200), body: a.lines.map((l) => plain(l, 400)).join('\n'), wu: a.wu ?? null };
}

/** The one alert for `k` lines written while the app was closed; its click opens the list. */
export function summaryOf(k: number): ShownAlert {
  return { title: `${k} alerts while Keel was closed`, body: 'The list shows each one.', wu: null };
}

/** For the run's alert lines: the record after them, and what to show (`first`: the app's first look since it started). */
export function decide(record: AlertRecord, run: string | null, alerts: readonly StoredAlert[], first: boolean): { record: AlertRecord; show: ShownAlert[] } {
  if (run === null) return { record, show: [] };
  const shown = record.run === run ? record.shown : [];
  const unseen = alerts.filter((a) => !shown.includes(a.n)).sort((a, b) => a.n - b.n);
  if (unseen.length === 0) return { record: record.run === run ? record : { version: 1, run, shown: [] }, show: [] };
  const next = [...new Set([...shown, ...unseen.map((a) => a.n)])].sort((a, b) => a - b).slice(-RECORD_CAP);
  const show = first && unseen.length > 1 ? [summaryOf(unseen.length)] : unseen.map(alertOf);
  return { record: { version: 1, run, shown: next }, show };
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
  private started = false;
  private readonly opts: AlerterOptions;

  constructor(opts: AlerterOptions) {
    this.opts = opts;
  }

  /** Show what is new among the run's alert lines, recorded first; the numbers shown so far in this run. */
  async take(run: string | null, alerts: readonly StoredAlert[]): Promise<Set<number>> {
    const file = path.join(this.opts.appDir, RECORD_FILE);
    if (this.record === null) this.record = (await load(file, parseRecord, emptyRecord)).value;
    const first = !this.started;
    this.started = true;
    const { record, show } = decide(this.record, run, alerts, first);
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
    return new Set(run !== null && record.run === run ? record.shown : []);
  }
}
