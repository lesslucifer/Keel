// Browser mode's ?scheme= override (fake/scheme.css) holds the shipped tokens, not a copy that drifts:
// its forced light block declares exactly styles.css's light values and its forced dark block exactly
// the dark ones, and the shipped page never imports it.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { expect, test } from 'vitest';

const WINDOW = path.resolve(import.meta.dirname, '..', '..', 'src', 'window');
const styles = readFileSync(path.join(WINDOW, 'styles.css'), 'utf8');
const scheme = readFileSync(path.join(WINDOW, 'fake', 'scheme.css'), 'utf8');

/** The `--name: value;` declarations of the first block opened by `opener`, sorted. */
function vars(text: string, opener: string): string[] {
  const start = text.indexOf(opener);
  if (start < 0) throw new Error(`no ${opener}`);
  const body = text.slice(text.indexOf('{', start + opener.length - 1) + 1, text.indexOf('}', start));
  return [...body.matchAll(/(--[a-z-]+):\s*([^;]+);/g)].map((m) => `${m[1]}: ${m[2]}`).sort();
}

const shippedLight = () => vars(styles, ':root {');
const shippedDark = () => vars(styles.slice(styles.indexOf('@media (prefers-color-scheme: dark)')), ':root {');

test('the forced light world is the shipped light world, value for value', () => {
  expect(shippedLight().length).toBeGreaterThan(10);
  expect(vars(scheme, ":root[data-scheme='light'] {")).toEqual(shippedLight());
});

test('the forced dark world is the shipped dark world, value for value', () => {
  expect(vars(scheme, ":root[data-scheme='dark'] {")).toEqual(shippedDark());
});

test('each forced world sets its color-scheme, so the browser\'s own controls follow it', () => {
  expect(scheme).toMatch(/:root\[data-scheme='light'\] \{\s*color-scheme: light;/);
  expect(scheme).toMatch(/:root\[data-scheme='dark'\] \{\s*color-scheme: dark;/);
});

test('only browser mode imports the override', () => {
  expect(readFileSync(path.join(WINDOW, 'browser.tsx'), 'utf8')).toMatch(/^import '\.\/fake\/scheme\.css';$/m);
  for (const f of ['main.tsx', 'App.tsx', 'index.html']) expect(readFileSync(path.join(WINDOW, f), 'utf8'), f).not.toMatch(/scheme\.css/);
});
