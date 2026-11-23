// Open's pure parts (E-APPLIST.W6): the tree read of a tip's docs/, the files a copy takes, keel doc-roles'
// answer (a data file's roles, id and page as keel reads them: the app reads no YAML itself, W12 review
// round 1, owner ruling 2026-11-23), the page a role and an id name, and the one kind of
// URL the app hands the browser: a file URL of a file inside a folder it allows.

import path from 'node:path';
import { pathToFileURL } from 'node:url';
import type { ItemKind } from '../../shared/contract.ts';
import { DOC_ROLES_BATCH, OUTPUT_CAP, type Captured } from '../commands.ts';

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

/**
 * The most bytes the temp folder of data files handed to keel holds at a time (epic review round 2, docs.ts:24).
 * At least OUTPUT_CAP, so every data file the copy takes fits in a batch; the app never caps a data file's size
 * itself: keel judges it (its bound, 1 MiB, equals OUTPUT_CAP, the most a copy reads of one file).
 */
export const HEADS_DISK_CAP = 4 * 1024 * 1024;

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

/**
 * The files a copy takes, in the order it copies them: regular files under docs/ (no link, no submodule),
 * each within the output cap; the paths named in `first` (the data file, then the page) before the rest,
 * the rest by path. The copy cap is charged on what is written (open.ts), so a file the copy drops
 * (a binary) never counts, and the page is never crowded out (review round 1, docs.ts:52).
 */
export function copyPlan(entries: readonly TreeEntry[], first: readonly string[] = []): { files: TreeEntry[]; skipped: string[] } {
  const files: TreeEntry[] = [];
  const skipped: string[] = [];
  for (const e of entries) {
    const regular = e.type === 'blob' && (e.mode === '100644' || e.mode === '100755') && e.size !== null;
    if (!regular || !docsPath(e.path) || (e.size as number) > OUTPUT_CAP) skipped.push(e.path);
    else files.push(e);
  }
  const rank = (p: string) => (first.includes(p) ? first.indexOf(p) : first.length);
  files.sort((x, y) => rank(x.path) - rank(y.path) || (x.path < y.path ? -1 : x.path > y.path ? 1 : 0));
  return { files, skipped };
}

/**
 * The data files (.yaml) among a copy's `files`, in their order, in batches for keel doc-roles: each batch at
 * most `maxFiles` files and, when it holds more than one, at most `maxBytes` bytes; a file over `maxBytes`
 * (never one the copy takes, which are within OUTPUT_CAP) goes alone. Every data file is in exactly one batch:
 * none is dropped for its size (epic review round 2, docs.ts:24).
 */
export function headBatches(files: readonly TreeEntry[], maxBytes = HEADS_DISK_CAP, maxFiles = DOC_ROLES_BATCH): TreeEntry[][] {
  const batches: TreeEntry[][] = [];
  let batch: TreeEntry[] = [];
  let bytes = 0;
  for (const f of files) {
    if (!f.path.endsWith('.yaml')) continue;
    const size = f.size ?? 0;
    if (batch.length > 0 && (batch.length >= maxFiles || bytes + size > maxBytes)) {
      batches.push(batch);
      batch = [];
      bytes = 0;
    }
    batch.push(f);
    bytes += size;
  }
  if (batch.length > 0) batches.push(batch);
  return batches;
}

export interface DataHead {
  artifact: string[];
  id: string | null;
  page: string | null;
}

/** The answer keel doc-roles gives (src/main.rs doc_roles), the one form the app reads. */
export const DOC_ROLES_SCHEMA = 'doc-roles/v1';

/** Why keel gave no doc-roles answer to read, or null: it did not start or answer, its answer was cut, or it is a keel with no doc-roles. */
export function docRolesWhy(r: Captured): string | null {
  if (r.startError !== null) return `keel could not start (${r.startError})`;
  if (r.timedOut) return "keel did not answer (the docs' roles)";
  if (r.cut) return "keel's answer was cut (the docs' roles)";
  if (r.code !== 0) return 'this keel cannot read a doc\'s roles (no keel doc-roles): update keel';
  return null;
}

/** Whether keel gave an id or a page as JSON text or null: any other type is no doc-roles/v1 answer. */
const textOrNull = (v: unknown): v is string | null => v === null || typeof v === 'string';

/** Whether the app can use an id or a page keel gave: null, or text that is not empty and holds no control character. */
const usable = (v: string | null): boolean => v === null || (v !== '' && !CONTROL.test(v));

/**
 * keel doc-roles' answer: each asked file's head (its roles, id and page as keel reads them), or null when
 * the answer is not doc-roles/v1 naming exactly the asked files, each once, with roles that are text and
 * an id and a page that are text or null. A file whose id or page the app cannot use (empty, or holding a
 * control character, which keel accepts in a draft or an epic's free-text id) gives no head, alone: the
 * other files keep theirs (W12 review round 2, docs.ts:83).
 */
export function parseDocRoles(out: string, asked: readonly string[]): Map<string, DataHead> | null {
  let v: unknown;
  try {
    v = JSON.parse(out);
  } catch {
    return null;
  }
  if (typeof v !== 'object' || v === null || (v as { schema?: unknown }).schema !== DOC_ROLES_SCHEMA) return null;
  const list = (v as { files?: unknown }).files;
  if (!Array.isArray(list) || list.length !== asked.length) return null;
  const heads = new Map<string, DataHead>();
  for (const [i, f] of list.entries()) {
    const want = asked[i];
    if (want === undefined || typeof f !== 'object' || f === null) return null;
    const { path: p, roles, id, page } = f as Record<string, unknown>;
    if (p !== want || !Array.isArray(roles) || !roles.every((r): r is string => typeof r === 'string') || !textOrNull(id) || !textOrNull(page)) return null;
    if (usable(id) && usable(page)) heads.set(want, { artifact: [...roles], id, page });
  }
  return heads;
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
