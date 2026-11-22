// The view waiting.get answers: the one listed project's git facts, the router's values, its alerts
// and where a click points the window. ViewReader rebuilds it on each read (the watcher's), from the
// project list, the git reader and the update log's reader, and says whether it changed; the view
// is read from memory, so waiting.get runs nothing. W6: the list, drawn from those facts by the item rules
// (src/shared/items.ts) with the owner's sent marks, on every read.

import type { Focus, RouterAlert, View } from '../../shared/contract.ts';
import type { RunOptions } from '../commands.ts';
import { countsOf, itemsOf } from '../../shared/items.ts';
import { GitReader, noFacts } from './git.ts';
import { RouterLog, type StoredAlert } from './log.ts';
import { projectTargets, type WatchTarget } from '../watcher/watch.ts';

/** What waiting.get answers before anything is read: no project, no git, no router run, not heard. */
export function emptyView(): View {
  return {
    project: null,
    git: null,
    router: { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null },
    alerts: [],
    focus: null,
    scheme: null,
    readAt: null,
    changedAt: null,
    items: [],
    counts: { yours: 0, notYours: 0, sent: 0 },
  };
}

export interface ListedProject {
  id: string;
  name: string;
  /** Its main checkout. */
  path: string;
}

export interface ViewReaderParts {
  /** The one listed project, or null (projects.json in the app's folder). */
  project(): Promise<ListedProject | null>;
  /** The git to read with (set in Keel, or found on the PATH), or null when none is usable. */
  git(): Promise<string | null>;
  runOptions: Omit<RunOptions, 'before245' | 'timeoutMs'>;
  now?: () => number;
  silenceMs?: number;
  /** Shows what is new among the run's alert lines; answers the numbers shown so far (alerter/alerts.ts). */
  alerter?: { take(run: string | null, alerts: readonly StoredAlert[]): Promise<Set<number>> };
  /** Where a click or a link points the window. */
  focus?: () => Focus | null;
  /** The app macOS opens keel:// links with. */
  scheme?: () => string | null;
  /** W6: the item ids the owner marked sent for this project (items/sent.ts); none when not given. */
  sent?: (project: string) => Promise<ReadonlySet<string>>;
}

/** A stored alert as the view shows it; whether it was shown is the alerter's to say. */
export function viewAlert(a: StoredAlert, shown: boolean): RouterAlert {
  return { n: a.n, at: a.at, headline: a.headline, lines: [...a.lines], wu: a.wu ?? null, link: a.link ?? null, shown };
}

/** The view without the times it was read and last changed: two reads that found the same are no change. */
function same(a: View, b: View): boolean {
  return JSON.stringify({ ...a, readAt: null, changedAt: null }) === JSON.stringify({ ...b, readAt: null, changedAt: null });
}

export class ViewReader {
  private current: View = emptyView();
  private root: string | null = null;
  private git: GitReader | null = null;
  private log: RouterLog | null = null;
  private readonly parts: ViewReaderParts;
  private readonly now: () => number;

  constructor(parts: ViewReaderParts) {
    this.parts = parts;
    this.now = parts.now ?? Date.now;
  }

  /** The view as last read; a copy. */
  view(): View {
    return structuredClone(this.current);
  }

  /** What the watcher watches: the listed project's git folder and update log folder, or nothing. */
  targets(): WatchTarget[] {
    return this.root === null ? [] : projectTargets(this.root);
  }

  /** The listed project's update log reader, or null (the alerter reads its alerts). */
  routerLog(): RouterLog | null {
    return this.log;
  }

  /** Read it all again; whether the view changed. */
  async read(): Promise<boolean> {
    const p = await this.parts.project();
    let next: View;
    if (p === null) {
      this.root = null;
      this.git = null;
      this.log = null;
      // No project: still the alerter's take, with no run, so the session's first take is spent here, not at a later Add.
      await this.parts.alerter?.take(null, []);
      next = { ...emptyView(), focus: this.parts.focus?.() ?? null, scheme: this.parts.scheme?.() ?? null, changedAt: this.current.changedAt };
    } else {
      if (p.path !== this.root || this.git === null || this.log === null) {
        this.root = p.path;
        this.git = new GitReader(p.path, this.parts.runOptions);
        this.log = new RouterLog({ root: p.path, now: this.now, ...(this.parts.silenceMs === undefined ? {} : { silenceMs: this.parts.silenceMs }) });
      }
      const git = await this.parts.git();
      const facts = git === null ? noFacts('git was not found: choose it in the settings') : await this.git.read(git);
      await this.log.refresh();
      const router = this.log.view();
      const stored = this.log.alerts();
      const shown = this.parts.alerter === undefined ? new Set<number>() : await this.parts.alerter.take(router.run, stored);
      const sent = this.parts.sent === undefined ? new Set<string>() : await this.parts.sent(p.id);
      const items = itemsOf({ git: facts, router, sent });
      next = {
        project: { id: p.id, name: p.name },
        git: facts,
        router,
        alerts: stored.map((a) => viewAlert(a, shown.has(a.n))),
        focus: this.parts.focus?.() ?? null,
        scheme: this.parts.scheme?.() ?? null,
        readAt: new Date(this.now()).toISOString(),
        changedAt: this.current.changedAt,
        items,
        counts: countsOf(items),
      };
    }
    const changed = !same(next, this.current);
    // The last change: when a read found the view changed (the first read included), as the clock says.
    this.current = changed ? { ...next, changedAt: new Date(this.now()).toISOString() } : next;
    return changed;
  }
}
