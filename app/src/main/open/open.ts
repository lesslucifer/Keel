// Open (docs/local-app.html 'The hand-off': "Open shows the committed bytes"; E-APPLIST.W6). A plan or a
// work unit's result: the tip's docs/ is copied from git into the app's folder (open/<project>/<tip>/,
// through the fixed reads only, regular files only, capped), the page is found through its data file at
// that tip, and its file URL goes to the owner's browser, so he reads what keel hashes and the links
// between docs still work. An epic result is untracked (docs/.archive/ in the epic's folder), so it opens
// in place, found through its data file there. Nothing is written into the project. The opener (Electron's
// shell.openExternal, or W9's file sink) is handed in.

import { mkdir, readdir, readFile, rename, rm, stat, lstat, utimes, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
import type { Change, GitFacts, Item } from '../../shared/contract.ts';
import { run, OUTPUT_CAP, type Captured, type RunOptions } from '../commands.ts';
import { copyPlan, dataHead, fileUrlInside, findPage, parseTree, roleOf, HEAD_CAP, type DataHead } from './docs.ts';

/** The most copies kept per project; the least recently opened go first. */
export const KEPT_COPIES = 8;

const DONE: Change = Object.freeze({ done: true });
const notDone = (why: string): Change => ({ done: false, why });
const COMPLETE = '.complete';

export interface OpenParts {
  /** The app's own folder. */
  appDir: string;
  /** The listed project: its id and main checkout. */
  project: { id: string; path: string };
  /** The git to read with (absolute). */
  git: string;
  runOptions: Omit<RunOptions, 'before245' | 'timeoutMs'>;
  /** Hand a file URL to the owner's browser. */
  openUrl(url: string): Promise<void>;
}

/** Why a read gave no whole answer, or null. */
function failed(r: Captured, what: string): string | null {
  if (r.startError !== null) return `git could not start (${r.startError})`;
  if (r.timedOut) return `git did not answer (${what})`;
  if (r.code !== 0) return `git could not read ${what} (exit ${r.code ?? 'killed'})`;
  if (r.cut) return `git's answer was cut (${what})`;
  return null;
}

async function exists(p: string): Promise<boolean> {
  try {
    await lstat(p);
    return true;
  } catch {
    return false;
  }
}

/** Every data file under `dir`/docs (a copy), with its head. */
async function headsIn(dir: string): Promise<Array<{ path: string; head: DataHead }>> {
  const out: Array<{ path: string; head: DataHead }> = [];
  const walk = async (rel: string): Promise<void> => {
    for (const name of (await readdir(path.join(dir, rel))).sort()) {
      const r = path.posix.join(rel, name);
      const st = await lstat(path.join(dir, r));
      if (st.isDirectory()) await walk(r);
      else if (st.isFile() && name.endsWith('.yaml') && st.size <= HEAD_CAP) out.push({ path: r, head: dataHead(await readFile(path.join(dir, r), 'utf8')) });
    }
  };
  if (await exists(path.join(dir, 'docs'))) await walk('docs');
  return out;
}

/** Keep the KEPT_COPIES copies opened last (by their folder's time), `keep` among them. */
async function prune(base: string, keep: string): Promise<void> {
  const names = (await readdir(base)).filter((n) => !n.startsWith('.') && n !== keep);
  const timed = await Promise.all(names.map(async (n) => ({ n, t: (await stat(path.join(base, n))).mtimeMs })));
  timed.sort((a, b) => b.t - a.t);
  for (const { n } of timed.slice(KEPT_COPIES - 1)) await rm(path.join(base, n), { recursive: true, force: true });
}

/** The copy of `tip`'s docs/ in the app folder, made once (all or nothing), or why not. */
async function copyOf(parts: OpenParts, tip: string): Promise<string | Change> {
  const base = path.join(parts.appDir, 'open', parts.project.id);
  const final = path.join(base, tip);
  await mkdir(base, { recursive: true, mode: 0o700 });
  if (await exists(path.join(final, COMPLETE))) {
    const now = new Date();
    await utimes(final, now, now);
    return final;
  }
  const read = (name: 'ls-docs' | 'blob', object: string) => run(parts.git, name, { project: parts.project.path, object }, parts.runOptions);
  const tree = await read('ls-docs', tip);
  const why = failed(tree, 'the docs at the tip');
  if (why !== null) return notDone(why);
  const { files } = copyPlan(parseTree(tree.stdout));
  const tmp = path.join(base, `.tmp-${process.pid}-${randomBytes(4).toString('hex')}`);
  await mkdir(tmp, { mode: 0o700 });
  try {
    for (const f of files) {
      const blob = await read('blob', f.oid);
      // Only a file read whole and as text is copied: a cut answer, or bytes that are not UTF-8, would not be the committed file.
      if (failed(blob, f.path) !== null || blob.stdout.length > OUTPUT_CAP || Buffer.byteLength(blob.stdout, 'utf8') !== f.size || blob.stdout.includes('�')) continue;
      const target = path.join(tmp, f.path);
      await mkdir(path.dirname(target), { recursive: true, mode: 0o700 });
      await writeFile(target, blob.stdout, { encoding: 'utf8', mode: 0o600, flag: 'wx' });
    }
    await writeFile(path.join(tmp, COMPLETE), `${tip}\n`, { mode: 0o600, flag: 'wx' });
    if (await exists(final)) await rm(final, { recursive: true, force: true });
    await rename(tmp, final);
  } catch (e) {
    await rm(tmp, { recursive: true, force: true });
    throw e;
  }
  await prune(base, tip);
  return final;
}

/** An epic result, in place: the epic's worktree's docs/.archive/, the page its data file names. */
async function openArchive(item: Item, facts: GitFacts, parts: OpenParts): Promise<Change> {
  const branch = item.branch ?? '';
  const wt = facts.worktrees.find((w) => w.branch === branch);
  if (wt === undefined) return notDone(`${branch} has no worktree here, so its result in docs/.archive cannot be found`);
  const dir = path.join(wt.path, 'docs', '.archive');
  const heads: Array<{ path: string; head: DataHead }> = [];
  let names: string[] = [];
  try {
    names = (await readdir(dir)).sort();
  } catch {
    return notDone(`${branch}'s worktree has no docs/.archive`);
  }
  for (const name of names) {
    if (!name.endsWith('.yaml')) continue;
    const st = await lstat(path.join(dir, name));
    if (st.isFile() && st.size <= HEAD_CAP) heads.push({ path: name, head: dataHead(await readFile(path.join(dir, name), 'utf8')) });
  }
  const page = findPage(heads, 'epic-result', item.epic ?? '');
  const file = page === null ? null : path.join(dir, page);
  const st = file === null ? null : await lstat(file).catch(() => null);
  const url = file === null || st === null || !st.isFile() ? null : fileUrlInside(file, [dir]);
  if (url === null) return notDone(`no epic-result page for ${item.epic ?? '?'} in docs/.archive of ${branch}'s worktree`);
  await parts.openUrl(url);
  return DONE;
}

/** Open `item` (a plan, a work unit's result, an epic result) in the owner's browser, as the facts stand now. */
export async function openItem(item: Item, facts: GitFacts | null, parts: OpenParts): Promise<Change> {
  const role = roleOf(item.kind);
  if (role === null) return notDone('this item has nothing to open');
  if (facts === null) return notDone('git was not read');
  if (item.open === 'archive') return openArchive(item, facts, parts);
  const id = item.wu;
  const branch = facts.branches.find((b) => b.name === item.branch);
  if (id === null || branch === undefined || item.tip === null || !branch.tip.startsWith(item.tip)) {
    return notDone('its tip moved since the list was drawn: the list shows it as it is now');
  }
  const copy = await copyOf(parts, branch.tip);
  if (typeof copy !== 'string') return copy;
  const page = findPage(await headsIn(copy), role, id);
  if (page === null) return notDone(`no ${role} page for ${id} at its tip`);
  const file = path.join(copy, page);
  const url = (await exists(file)) ? fileUrlInside(file, [path.join(parts.appDir, 'open')]) : null;
  if (url === null) return notDone(`the ${role} page for ${id} could not be copied as text`);
  await parts.openUrl(url);
  return DONE;
}
