// The Watcher (local-app.html 'The stack'): it watches the project's git folder (branch and notes
// refs, packed refs, each worktree's HEAD) and the router's .router/app/, waits a second for the
// burst, then reads again; it reads once a minute anyway, so a missed file event costs a minute at
// most, never a stale view (it never trusts a file watch alone). Reads never overlap: a change during
// a read makes one more read after it. The watches are armed after each read, for the targets the
// read leaves (the project it found, or none). A folder that cannot be watched yet (no .router/app before
// the router's first start) is armed again at the next minute. Timers and the file watch are handed
// in, so its timing is tested without a clock.

import { watch as fsWatch } from 'node:fs';
import path from 'node:path';

export const DEBOUNCE_MS = 1000;
export const REREAD_MS = 60_000;

export interface Timers {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  setInterval(fn: () => void, ms: number): unknown;
  clearInterval(handle: unknown): void;
}

/** Watch `dir`; `onEvent` gets the changed file's path below it (null when the system names none); `onGone` when the watch ends by itself. */
export type WatchFn = (dir: string, recursive: boolean, onEvent: (file: string | null) => void, onGone?: () => void) => { close(): void };

export interface WatchTarget {
  dir: string;
  recursive: boolean;
  /** Whether a change to `file` (below `dir`) is a reason to read. */
  relevant(file: string | null): boolean;
}

/** The git files whose change can change the view: a ref (branches, notes), packed refs, HEAD, a worktree's HEAD. */
export function gitRelevant(file: string | null): boolean {
  if (file === null) return true;
  const f = file.split(path.sep).join('/');
  return f === 'HEAD' || f === 'packed-refs' || f.startsWith('refs/') || /^worktrees\/[^/]+\/HEAD$/.test(f);
}

/** What to watch for a project's main checkout: its git folder, and the router's update log folder. */
export function projectTargets(root: string): WatchTarget[] {
  return [
    { dir: path.join(root, '.git'), recursive: true, relevant: gitRelevant },
    { dir: path.join(root, '.router', 'app'), recursive: false, relevant: () => true },
  ];
}

/** Node's fs.watch (FSEvents on macOS), never keeping the app alive by itself. */
export const nodeWatch: WatchFn = (dir, recursive, onEvent, onGone) => {
  const w = fsWatch(dir, { recursive, persistent: false }, (_kind, file) => onEvent(file === null ? null : String(file)));
  w.on('error', () => {
    w.close();
    onGone?.();
  });
  return { close: () => w.close() };
};

const realTimers: Timers = {
  setTimeout: (fn, ms) => setTimeout(fn, ms).unref(),
  clearTimeout: (h) => clearTimeout(h as NodeJS.Timeout),
  setInterval: (fn, ms) => setInterval(fn, ms).unref(),
  clearInterval: (h) => clearInterval(h as NodeJS.Timeout),
};

export interface WatcherOptions {
  targets(): WatchTarget[] | Promise<WatchTarget[]>;
  /** Read again (and push when the view changed); a throw is caught and the next change reads again. */
  read(): Promise<void>;
  watch?: WatchFn;
  timers?: Timers;
  debounceMs?: number;
  rereadMs?: number;
}

export class Watcher {
  private readonly opts: Required<WatcherOptions>;
  private readonly armed = new Map<string, { close(): void }>();
  private debounce: unknown = null;
  private interval: unknown = null;
  private reading: Promise<void> | null = null;
  private again = false;
  private stopped = false;

  constructor(opts: WatcherOptions) {
    this.opts = { watch: nodeWatch, timers: realTimers, debounceMs: DEBOUNCE_MS, rereadMs: REREAD_MS, ...opts };
  }

  /** Start the minute's re-read, and read once (which arms the watches). */
  async start(): Promise<void> {
    this.interval = this.opts.timers.setInterval(() => void this.kick(), this.opts.rereadMs);
    await this.read();
  }

  /** Read now (a project added or removed, the minute's tick); every read arms the watches for the targets as they then are. */
  kick(): Promise<void> {
    return this.stopped ? Promise.resolve() : this.read();
  }

  stop(): void {
    this.stopped = true;
    for (const w of this.armed.values()) w.close();
    this.armed.clear();
    if (this.debounce !== null) this.opts.timers.clearTimeout(this.debounce);
    if (this.interval !== null) this.opts.timers.clearInterval(this.interval);
    this.debounce = null;
    this.interval = null;
  }

  private async arm(): Promise<void> {
    const targets = await this.opts.targets();
    if (this.stopped) return;
    const keys = new Map(targets.map((t) => [`${t.recursive ? 'r' : 'f'}:${t.dir}`, t]));
    for (const [key, w] of this.armed) {
      if (!keys.has(key)) {
        w.close();
        this.armed.delete(key);
      }
    }
    for (const [key, t] of keys) {
      if (this.armed.has(key)) continue;
      try {
        const handle = this.opts.watch(t.dir, t.recursive, (file) => {
          if (t.relevant(file)) this.schedule();
        }, () => this.armed.delete(key));
        this.armed.set(key, handle);
      } catch {
        // Not there yet (no .router/app before the router's first start): the next minute tries again.
      }
    }
  }

  private schedule(): void {
    if (this.stopped) return;
    if (this.debounce !== null) this.opts.timers.clearTimeout(this.debounce);
    this.debounce = this.opts.timers.setTimeout(() => {
      this.debounce = null;
      void this.read();
    }, this.opts.debounceMs);
  }

  /** One read at a time; a call during a read asks for one more after it, and waits for that. */
  private read(): Promise<void> {
    if (this.reading !== null) {
      this.again = true;
      return this.reading;
    }
    this.reading = (async () => {
      do {
        this.again = false;
        if (this.stopped) break;
        try {
          await this.opts.read();
        } catch {
          // A failed read leaves the view as it was; the next change or minute reads again.
        }
        // After each read: the read may have found the project (or lost it), and so what to watch.
        await this.arm().catch(() => undefined);
      } while (this.again);
    })().finally(() => {
      this.reading = null;
    });
    return this.reading;
  }
}
