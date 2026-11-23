// Open's pure parts (E-APPLIST.W6): the tree read of a tip's docs/, the files a copy takes, a data file's
// head (its roles as keel reads them, its id and page), the page a role and an id name, and the one kind of
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

export interface DataHead {
  artifact: string[];
  id: string | null;
  page: string | null;
}

/** The roles keel knows (verify_doc.rs ROLES), and the two pairs a one-part epic may join (its combination_problem). */
const ROLES: readonly string[] = ['epic-plan', 'wu-plan', 'wu-result', 'epic-result'];
const PAIRS: readonly string[] = ['epic-plan wu-plan', 'epic-result wu-result'];

/** A field's value as the head reads it: text, a list (an item that is not text is null), or anything else. */
type Value = { kind: 'text'; text: string } | { kind: 'list'; items: Array<string | null> } | { kind: 'other' };

/** A value written with no comment after it: ` #` outside quotes starts one. */
function uncommented(v: string): string {
  let quote: string | null = null;
  for (let i = 0; i < v.length; i++) {
    const c = v[i] as string;
    if (quote !== null) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === '#' && (i === 0 || /\s/.test(v[i - 1] as string))) return v.slice(0, i).trimEnd();
  }
  return v.trimEnd();
}

/** Whether a value starts with an anchor or an alias (keel refuses a data file holding either: verify_doc.rs ANCHORS). */
const anchored = (v: string) => /^[&*]\S/.test(v.trim());

/** One scalar's text: plain, or in quotes; null when it is no text (an anchor, an alias, a collection, an unclosed quote, a mapping). */
function scalarText(raw: string): string | null {
  const t = raw.trim();
  if (t === '' || anchored(t) || /^[[{|>!%@`]/.test(t)) return null;
  const q = t[0];
  if (q === '"' || q === "'") {
    if (t.length < 2 || t[t.length - 1] !== q) return null;
    const inner = t.slice(1, -1);
    return q === "'" ? inner.replace(/''/g, "'") : inner;
  }
  return /:(\s|$)/.test(t) ? null : t;
}

/** A flow list's items, `[a, "b"]`: each item's text, or null; null for the whole when it is not one list. */
function flowList(t: string): Array<string | null> | null {
  if (!t.startsWith('[') || !t.endsWith(']')) return null;
  const inner = t.slice(1, -1);
  if (/[[\]{}]/.test(inner)) return null;
  if (inner.trim() === '') return [];
  const parts = inner.split(',');
  if (parts.length > 1 && (parts.at(-1) as string).trim() === '') parts.pop();
  return parts.map(scalarText);
}

const indentOf = (line: string) => line.length - line.trimStart().length;
const content = (line: string) => line.trim() !== '' && !line.trimStart().startsWith('#');

/**
 * A data file's documents, each its top-level fields as the head reads them (null: a document that is no
 * mapping), and whether anything in the file is not YAML keel would load or holds an anchor (then no role).
 * Read as keel's loader reads it (epic review round 1, docs.ts:79): a byte-order mark read past; documents
 * split at ---; a value as text (plain or quoted, its continuation lines joined), a flow list or a block
 * list; a first line further in sets the mapping's indent, and a line left of it is not YAML.
 */
function documents(text: string): { docs: Array<Map<string, Value> | null>; refused: boolean } {
  const lines = text.replace(/^\uFEFF/, '').split(/\r?\n/);
  const docs: Array<Map<string, Value> | null> = [];
  let refused = false;
  let doc: Map<string, Value> | null = new Map();
  let started = false;
  let base = 0;
  let key: string | null = null;
  let pending: { inline: string; block: string[] } | null = null;
  const settle = () => {
    if (pending === null || key === null || doc === null) return;
    const { inline, block } = pending;
    pending = null;
    if (inline === '') {
      if (block.length === 0) return void doc.set(key, { kind: 'other' });
      if (!block.every((l) => /^-(\s|$)/.test(l.trimStart()))) return void doc.set(key, { kind: 'other' });
      return void doc.set(key, { kind: 'list', items: block.map((l) => scalarText(uncommented(l.trimStart().slice(1)))) });
    }
    if (inline.startsWith('[')) {
      const items = block.length === 0 ? flowList(inline) : null;
      if (items === null) refused = true;
      return void doc.set(key, items === null ? { kind: 'other' } : { kind: 'list', items });
    }
    if (/^[|>]/.test(inline)) return void doc.set(key, block.length === 0 ? { kind: 'other' } : { kind: 'text', text: block.map((l) => l.trim()).join(' ') });
    const first = scalarText(inline);
    if (first === null) return void doc.set(key, { kind: 'other' });
    const quoted = inline.startsWith('"') || inline.startsWith("'");
    if (quoted && block.length > 0) return void doc.set(key, { kind: 'other' });
    doc.set(key, { kind: 'text', text: [first, ...block.map((l) => l.trim())].join(' ') });
  };
  const close = () => {
    settle();
    if (started) docs.push(doc);
    doc = new Map();
    started = false;
    key = null;
  };
  for (const line of lines) {
    if (/^(---|\.\.\.)(\s|$)/.test(line)) {
      close();
      continue;
    }
    if (!content(line)) continue;
    const at = indentOf(line);
    const body = uncommented(line.trim());
    if (anchored(body.replace(/^(-\s+)+/, '').replace(/^[^\s"'[{][^:]*:\s*/, '')) || /[[,]\s*[&*]\S/.test(body)) refused = true;
    if (!started) {
      started = true;
      base = at;
    }
    if (at < base) {
      refused = true;
      continue;
    }
    if (doc === null) continue;
    const field = at === base ? /^([A-Za-z_][A-Za-z0-9_-]*)\s*:(?:\s+(.*))?$/.exec(body) : null;
    if (field !== null) {
      settle();
      key = field[1] as string;
      pending = { inline: field[2] ?? '', block: [] };
      continue;
    }
    if (pending !== null && (at > base || (/^-(\s|$)/.test(body) && pending.inline === ''))) {
      pending.block.push(line);
      continue;
    }
    // A sequence or a scalar at the top: no mapping. Anything else at the mapping's indent: not YAML keel loads.
    if (key === null && (/^-(\s|$)/.test(body) || !body.includes(':'))) doc = null;
    else if (!/^["'][^"']*["']\s*:/.test(body)) refused = true;
  }
  close();
  return { docs, refused };
}

/** The roles a value names, as keel's data_roles reads them: text split on white space, a list's items whole; none on any role problem. */
function rolesOf(v: Value | undefined): string[] {
  if (v === undefined || v.kind === 'other') return [];
  const roles = v.kind === 'text' ? v.text.split(/\s+/).filter((r) => r !== '') : v.items;
  if (roles.length === 0 || roles.some((r) => r === null || !ROLES.includes(r))) return [];
  const set = [...new Set(roles as string[])].sort();
  return set.length === 1 || PAIRS.includes(set.join(' ')) ? (roles as string[]) : [];
}

/**
 * A data file's roles, exactly as keel verify-doc's data_roles gives them (src/verify_doc.rs): the document
 * holding artifact, its text split on white space or its list; none for a file keel refuses or a role
 * problem. Both read one case table, tests/data/data-roles.json (epic review round 1, docs.ts:79).
 */
export function dataRoles(text: string): string[] {
  const { docs, refused } = documents(text);
  if (refused) return [];
  return rolesOf(docs.find((d) => d?.has('artifact'))?.get('artifact'));
}

/** A data file's roles (dataRoles), and the id and page of the document holding artifact; nested keys are not read. */
export function dataHead(text: string): DataHead {
  const { docs } = documents(text);
  const doc = docs.find((d) => d?.has('artifact')) ?? docs.find((d) => d !== null) ?? null;
  const textOf = (k: string): string | null => {
    const v = doc?.get(k);
    return v?.kind === 'text' ? v.text.trim() : null;
  };
  return { artifact: dataRoles(text), id: textOf('id'), page: textOf('page') };
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
