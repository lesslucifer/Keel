// The app folder's files: read and parsed whole, written atomically (a temp file in the same
// folder, synced, then renamed over), owner-only. A file that does not read or parse is renamed
// aside with its UTC time and the defaults load, so a half write never stops the app. Loads and writes
// of one file run one at a time, whoever asks (the handlers' queue, the reader, Open): two loads never
// both set one file aside, and a load never sets aside a file a write has just replaced (epic review
// round 1, store.ts:82).

import { open, readFile, rename, mkdir, unlink, lstat } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import path from 'node:path';

/** The largest file the app reads back. */
export const FILE_CAP = 1024 * 1024;

export type ReadResult = { kind: 'missing' } | { kind: 'ok'; value: unknown } | { kind: 'broken'; why: string };

/** `file` plus .broken- and the UTC time, to the second. */
export function asideName(file: string, now: Date): string {
  const stamp = now.toISOString().replace(/\.\d{3}Z$/, 'Z').replace(/[-:]/g, '');
  return `${file}.broken-${stamp}`;
}

export async function readJson(file: string): Promise<ReadResult> {
  let bytes: Buffer;
  try {
    bytes = await readFile(file);
  } catch (e) {
    const code = (e as NodeJS.ErrnoException).code;
    return code === 'ENOENT' ? { kind: 'missing' } : { kind: 'broken', why: `unreadable (${code ?? 'error'})` };
  }
  if (bytes.length > FILE_CAP) return { kind: 'broken', why: 'too large' };
  try {
    return { kind: 'ok', value: JSON.parse(bytes.toString('utf8')) as unknown };
  } catch {
    return { kind: 'broken', why: 'not JSON' };
  }
}

/** Per file (its resolved path): the last load or write asked for, settled; its entry goes once nothing waits. */
const turns = new Map<string, Promise<void>>();

/** Run `job` once every load and write of `file` asked for before it has settled. */
function inTurn<T>(file: string, job: () => Promise<T>): Promise<T> {
  const key = path.resolve(file);
  const next = (turns.get(key) ?? Promise.resolve()).then(job);
  const settled = next.then(
    () => undefined,
    () => undefined,
  );
  turns.set(key, settled);
  void settled.then(() => {
    if (turns.get(key) === settled) turns.delete(key);
  });
  return next;
}

/** Write `value` as JSON to `file`: its folder made owner-only if new, the file owner-only, all or nothing; in turn with the file's loads. */
export function writeAtomic(file: string, value: unknown): Promise<void> {
  return inTurn(file, () => io.writeNow(file, value));
}

async function writeNow(file: string, value: unknown): Promise<void> {
  const dir = path.dirname(file);
  await mkdir(dir, { recursive: true, mode: 0o700 });
  const tmp = path.join(dir, `.${path.basename(file)}.${process.pid}.${randomBytes(4).toString('hex')}.tmp`);
  const fh = await open(tmp, 'wx', 0o600);
  try {
    await fh.writeFile(`${JSON.stringify(value, null, 2)}\n`, 'utf8');
    await fh.sync();
  } catch (e) {
    await fh.close();
    await unlink(tmp).catch(() => undefined);
    throw e;
  }
  await fh.close();
  try {
    await rename(tmp, file);
  } catch (e) {
    await unlink(tmp).catch(() => undefined);
    throw e;
  }
}

/** The read load() runs and the write writeAtomic() runs in its turn, on one object so a test can hold or watch them with a controlled promise. */
export const io = { readJson, writeNow };

async function exists(p: string): Promise<boolean> {
  try {
    await lstat(p);
    return true;
  } catch {
    return false;
  }
}

/**
 * Load `file` through `parse` (null: a shape the app does not know). Missing: the defaults.
 * Broken: renamed aside (never over an earlier one) and the defaults; `setAside` names where. Gone
 * before it was set aside: as missing. In turn with the file's other loads and writes.
 */
export function load<T>(file: string, parse: (v: unknown) => T | null, defaults: () => T, now: Date = new Date()): Promise<{ value: T; setAside: string | null }> {
  return inTurn(file, () => loadNow(file, parse, defaults, now));
}

async function loadNow<T>(file: string, parse: (v: unknown) => T | null, defaults: () => T, now: Date): Promise<{ value: T; setAside: string | null }> {
  const r = await io.readJson(file);
  if (r.kind === 'missing') return { value: defaults(), setAside: null };
  if (r.kind === 'ok') {
    const v = parse(r.value);
    if (v !== null) return { value: v, setAside: null };
  }
  const base = asideName(file, now);
  let aside = base;
  for (let i = 1; await exists(aside); i++) aside = `${base}-${i}`;
  try {
    await rename(file, aside);
  } catch (e) {
    // Gone since it was read (another process, or the owner): it reads as missing.
    if ((e as NodeJS.ErrnoException).code === 'ENOENT') return { value: defaults(), setAside: null };
    throw e;
  }
  return { value: defaults(), setAside: aside };
}
