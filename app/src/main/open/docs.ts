// Open's pure parts (E-APPLIST.W6): the tree read of a tip's docs/, the files a copy takes, a data file's
// head (its top-level artifact, id and page lines), the page a role and an id name, and the one kind of
// URL the app hands the browser: a file URL of a file inside a folder it allows.

import path from 'node:path';
import { pathToFileURL } from 'node:url';
import type { ItemKind } from '../../shared/contract.ts';
import { OUTPUT_CAP } from '../commands.ts';

export interface TreeEntry {
  mode: string;
  type: string;
  oid: string;
  /** The blob's size in bytes; null for a submodule. */
  size: number | null;
  path: string;
}

/** The most bytes one copy of a tip's docs/ holds. */
export const COPY_CAP = 8 * 1024 * 1024;

/** The largest data file whose head is read. */
export const HEAD_CAP = 64 * 1024;

const ROW = /^([0-7]{6}) ([a-z]+) ([0-9a-f]{40}|[0-9a-f]{64}) +([0-9]+|-)\t([\s\S]+)$/;
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** git ls-tree -r -z -l: each entry, its mode, type, id, size and path; a row that does not parse is dropped. */
export function parseTree(out: string): TreeEntry[] {
  const list: TreeEntry[] = [];
  for (const rec of out.split('\0')) {
    const m = ROW.exec(rec);
    if (m === null) continue;
    list.push({ mode: m[1] as string, type: m[2] as string, oid: m[3] as string, size: m[4] === '-' ? null : Number(m[4]), path: m[5] as string });
  }
  return list;
}

/** Whether a tree path is a plain file path under docs/: no empty, '.' or '..' part, no control character. */
function docsPath(p: string): boolean {
  if (!p.startsWith('docs/') || CONTROL.test(p)) return false;
  return p.split('/').every((part) => part !== '' && part !== '.' && part !== '..');
}

/** The files a copy takes: regular files under docs/ (no link, no submodule), each within the output cap, all within COPY_CAP. */
export function copyPlan(entries: readonly TreeEntry[]): { files: TreeEntry[]; skipped: string[] } {
  const files: TreeEntry[] = [];
  const skipped: string[] = [];
  let total = 0;
  for (const e of entries) {
    const regular = e.type === 'blob' && (e.mode === '100644' || e.mode === '100755') && e.size !== null;
    if (!regular || !docsPath(e.path) || (e.size as number) > OUTPUT_CAP || total + (e.size as number) > COPY_CAP) {
      skipped.push(e.path);
      continue;
    }
    total += e.size as number;
    files.push(e);
  }
  return { files, skipped };
}

export interface DataHead {
  artifact: string[];
  id: string | null;
  page: string | null;
}

/** A scalar with its quotes taken off; null for an anchor or an alias, which the app does not follow. */
function scalar(v: string): string | null {
  const t = v.trim();
  if (t === '' || t.startsWith('&') || t.startsWith('*')) return null;
  const q = /^"([^"]*)"$/.exec(t) ?? /^'([^']*)'$/.exec(t);
  return q === null ? t : (q[1] as string);
}

/** A data file's top-level artifact (one, or a [list]), id and page; nested keys are not read. */
export function dataHead(text: string): DataHead {
  const head: DataHead = { artifact: [], id: null, page: null };
  const seen = new Set<string>();
  for (const line of text.split(/\r?\n/)) {
    const m = /^(artifact|id|page):(.*)$/.exec(line);
    if (m === null || seen.has(m[1] as string)) continue;
    seen.add(m[1] as string);
    const raw = (m[2] as string).trim();
    if (m[1] === 'artifact') {
      const list = /^\[(.*)\]$/.exec(raw);
      const parts = list === null ? [raw] : (list[1] as string).split(',');
      const values = parts.map(scalar);
      head.artifact = values.every((x): x is string => x !== null) ? values : [];
    } else if (m[1] === 'id') head.id = scalar(raw);
    else head.page = scalar(raw);
  }
  return head;
}

/** The page a role and an id name: beside the data file whose head matches, a plain .html name; the newest by path when several match. */
export function findPage(heads: ReadonlyArray<{ path: string; head: DataHead }>, role: string, id: string): string | null {
  const matches = heads
    .filter((h) => h.head.artifact.includes(role) && h.head.id === id && h.head.page !== null && /^[^/\\\0]+\.html$/.test(h.head.page) && !h.head.page.startsWith('.'))
    .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  const last = matches.at(-1);
  return last === undefined ? null : path.posix.join(path.posix.dirname(last.path), last.head.page as string);
}

/** The role an item kind opens: a plan its WU plan, a result its WU result, an epic result its epic result. */
export function roleOf(kind: ItemKind): string | null {
  return kind === 'plan' ? 'wu-plan' : kind === 'result' ? 'wu-result' : kind === 'epic-result' ? 'epic-result' : null;
}

/** A file URL for `file` when it is an absolute path strictly inside one of `roots`; else null. */
export function fileUrlInside(file: string, roots: readonly string[]): string | null {
  if (!path.isAbsolute(file) || CONTROL.test(file)) return null;
  const resolved = path.resolve(file);
  if (resolved !== file) return null;
  const inside = roots.some((r) => {
    const root = path.resolve(r);
    return resolved.startsWith(root + path.sep) && resolved.length > root.length + 1;
  });
  return inside ? pathToFileURL(resolved).href : null;
}
