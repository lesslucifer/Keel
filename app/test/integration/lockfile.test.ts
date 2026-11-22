// The app installs with pnpm alone (E-APPLIST.W10): the committed lockfile holds exactly package.json's pins, and no
// script reaches for npm. Read from the real files; node: built-ins only, so keel check runs it with nothing installed.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const APP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

interface Manifest {
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  scripts?: Record<string, string>;
}

const manifest = (): Manifest => JSON.parse(readFileSync(path.join(APP, 'package.json'), 'utf8')) as Manifest;

/** The root importer of a pnpm v9 lockfile, per section: name -> resolved version (a peer suffix "(…)" cut off). */
function importer(lock: string): Record<string, Record<string, string>> {
  const lines = lock.split('\n');
  const start = lines.indexOf('importers:');
  assert.ok(start >= 0, 'no importers: section');
  const out: Record<string, Record<string, string>> = {};
  let section = '';
  let name = '';
  let inRoot = false;
  for (const line of lines.slice(start + 1)) {
    if (/^\S/.test(line)) break;
    if (/^ {2}\S/.test(line)) { inRoot = line === '  .:'; continue; }
    if (!inRoot) continue;
    let m: RegExpExecArray | null;
    if ((m = /^ {4}(\w+):$/.exec(line))) { section = m[1] as string; out[section] = {}; continue; }
    if ((m = /^ {6}'?([^']+?)'?:$/.exec(line))) { name = m[1] as string; continue; }
    if ((m = /^ {8}version: (\S+)$/.exec(line))) (out[section] as Record<string, string>)[name] = (m[1] as string).replace(/\(.*$/, '');
  }
  return out;
}

test('pnpm-lock.yaml\'s root importer resolves exactly package.json\'s dependencies and devDependencies, each at its pin', () => {
  const pkg = manifest();
  const got = importer(readFileSync(path.join(APP, 'pnpm-lock.yaml'), 'utf8'));
  assert.deepEqual(got.dependencies ?? {}, pkg.dependencies ?? {});
  assert.deepEqual(got.devDependencies ?? {}, pkg.devDependencies ?? {});
  assert.deepEqual(Object.keys(got).sort(), ['dependencies', 'devDependencies']);
});

test('the importer reader cuts a peer suffix and reads a quoted scoped name', () => {
  const lock = [
    "lockfileVersion: '9.0'", '', 'importers:', '', '  .:', '    devDependencies:',
    "      '@a/b':", '        specifier: 1.2.3', '        version: 1.2.3(@c/d@4.5.6)',
    '      e:', '        specifier: 7.8.9', '        version: 7.8.9', '', 'packages:', '', '  e@7.8.9:', '',
  ].join('\n');
  assert.deepEqual(importer(lock), { devDependencies: { '@a/b': '1.2.3', e: '7.8.9' } });
});

test('no package.json script calls npm or npx: the app runs under pnpm alone', () => {
  const scripts = manifest().scripts ?? {};
  assert.ok(Object.keys(scripts).length > 0, 'no scripts');
  for (const [name, cmd] of Object.entries(scripts)) assert.doesNotMatch(cmd, /(^|[\s;&|(])(npm|npx)(\s|$)/, name);
});
