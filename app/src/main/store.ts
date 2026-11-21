// The app folder's files: read and parsed whole, written atomically (a temp file in the same
// folder, synced, then renamed over), owner-only. A file that does not read or parse is renamed
// aside with its UTC time and the defaults load, so a half write never stops the app.

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

/** Write `value` as JSON to `file`: its folder made owner-only if new, the file owner-only, all or nothing. */
export async function writeAtomic(file: string, value: unknown): Promise<void> {
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
 * Broken: renamed aside (never over an earlier one) and the defaults; `setAside` names where.
 */
export async function load<T>(file: string, parse: (v: unknown) => T | null, defaults: () => T, now: Date = new Date()): Promise<{ value: T; setAside: string | null }> {
  const r = await readJson(file);
  if (r.kind === 'missing') return { value: defaults(), setAside: null };
  if (r.kind === 'ok') {
    const v = parse(r.value);
    if (v !== null) return { value: v, setAside: null };
  }
  const base = asideName(file, now);
  let aside = base;
  for (let i = 1; await exists(aside); i++) aside = `${base}-${i}`;
  await rename(file, aside);
  return { value: defaults(), setAside: aside };
}
