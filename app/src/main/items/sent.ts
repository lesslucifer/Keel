// The owner's sent marks (docs/local-app.html 'The hand-off': Mark sent; E-APPLIST.W6): a plan or a
// question he answered by a message moves to Sent, waiting for a sign. Kept in the app's own folder
// (sent.json, written atomically and owner-only by store.ts), per project, so a mark survives a restart.
// An item id carries its version (a tip, a hash of the question), so a mark never reaches a moved plan or
// a new question; each write drops this project's marks whose item is no longer listed, and the record
// keeps at most SENT_CAP marks, the newest.

import path from 'node:path';
import { ITEM_ID_SOURCE, PROJECT_ID_SOURCE } from '../../shared/contract.ts';
import { load, writeAtomic } from '../store.ts';

export const SENT_FILE = 'sent.json';
/** The most marks the record keeps, the oldest let go first. */
export const SENT_CAP = 200;

export interface SentRecord {
  version: 1;
  marks: Array<{ project: string; item: string }>;
}

const PROJECT = new RegExp(PROJECT_ID_SOURCE);
const ITEM = new RegExp(ITEM_ID_SOURCE);

export function emptySent(): SentRecord {
  return { version: 1, marks: [] };
}

/** The record, or null for any shape the app does not know (store.ts sets it aside). */
export function parseSent(v: unknown): SentRecord | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const r = v as Record<string, unknown>;
  if (r.version !== 1 || !Array.isArray(r.marks)) return null;
  const marks: SentRecord['marks'] = [];
  for (const m of r.marks as unknown[]) {
    if (typeof m !== 'object' || m === null) return null;
    const { project, item } = m as Record<string, unknown>;
    if (typeof project !== 'string' || !PROJECT.test(project) || typeof item !== 'string' || !ITEM.test(item)) return null;
    marks.push({ project, item });
  }
  return { version: 1, marks };
}

/** The item ids marked sent for `project`. */
export function marked(record: SentRecord, project: string): Set<string> {
  return new Set(record.marks.filter((m) => m.project === project).map((m) => m.item));
}

/**
 * The record after marking `item` sent (or taking the mark back) for `project`: this project's marks
 * whose item is not in `listed` are dropped, a mark is held once, and the newest SENT_CAP are kept.
 */
export function withMark(record: SentRecord, project: string, item: string, sent: boolean, listed: ReadonlySet<string>): SentRecord {
  const kept = record.marks.filter((m) => m.project !== project || (m.item !== item && listed.has(m.item)));
  const marks = sent ? [...kept, { project, item }] : kept;
  return { version: 1, marks: marks.slice(-SENT_CAP) };
}

/** The marks on disk, in the app's folder. */
export class SentMarks {
  private readonly file: string;

  constructor(appDir: string) {
    this.file = path.join(appDir, SENT_FILE);
  }

  private async record(): Promise<SentRecord> {
    return (await load(this.file, parseSent, emptySent)).value;
  }

  /** The ids marked sent for `project`. */
  async get(project: string): Promise<Set<string>> {
    return marked(await this.record(), project);
  }

  /** Mark `item` sent (or not) for `project`; `listed` is what the list holds now (the markable ids). */
  async set(project: string, item: string, sent: boolean, listed: ReadonlySet<string>): Promise<void> {
    await writeAtomic(this.file, withMark(await this.record(), project, item, sent, listed));
  }
}
