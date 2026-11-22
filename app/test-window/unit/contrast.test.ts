// Contrast in both worlds, computed from styles.css's tokens (WCAG AA): every text colour the window
// uses reaches 4.5:1 on each ground it sits on, words on every badge included, and every button's edge
// (the black pill, the outline pill) reaches 3:1 against the ground it stands on (non-text, 1.4.11).
// A text colour a window file draws with and this file does not measure fails the last test; the
// grounds each one is drawn on are checked in test-window/integration/grounds.test.tsx.
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { expect, test } from 'vitest';
import { PAIRS } from '../support/pairs.ts';

const WINDOW = path.resolve(import.meta.dirname, '..', '..', 'src', 'window');
const css = readFileSync(path.join(WINDOW, 'styles.css'), 'utf8');
const dark = css.slice(css.indexOf('@media (prefers-color-scheme: dark)'));
const light = css.slice(0, css.indexOf('@media (prefers-color-scheme: dark)'));

function rgb(block: string, name: string): [number, number, number] {
  const m = new RegExp(`--${name}:\\s*(\\d+) (\\d+) (\\d+);`).exec(block);
  if (m === null) throw new Error(`no --${name}`);
  return [Number(m[1]), Number(m[2]), Number(m[3])];
}

function luminance([r, g, b]: [number, number, number]): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(a: [number, number, number], b: [number, number, number]): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p) as [number, number];
  return (x + 0.05) / (y + 0.05);
}


/** Button edges against the grounds they stand on: 3:1, the non-text minimum. */
const EDGES: Array<[string, string]> = [
  ['primary', 'canvas'],
  ['primary', 'card'],
  ['outline', 'canvas'],
  ['outline', 'card'],
];

test('every text colour the window uses reaches 4.5:1 on its ground, by day and by night', () => {
  for (const [world, block] of [['light', light], ['dark', dark]] as const) {
    for (const [fg, bg] of PAIRS) expect(contrast(rgb(block, fg), rgb(block, bg)), `${world}: ${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
  }
});

test('every button edge reaches 3:1 on its ground, by day and by night', () => {
  for (const [world, block] of [['light', light], ['dark', dark]] as const) {
    for (const [fg, bg] of EDGES) expect(contrast(rgb(block, fg), rgb(block, bg)), `${world}: ${fg} on ${bg}`).toBeGreaterThanOrEqual(3);
  }
});

test('each text colour a window file draws with is one this file measures', () => {
  const walk = (dir: string): string[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));
  const colours = new Set([...css.matchAll(/--([a-z-]+):\s*\d+ \d+ \d+;/g)].map((m) => m[1] as string));
  const used = new Set<string>();
  for (const f of walk(WINDOW).filter((x) => /\.(tsx?|html)$/.test(x))) {
    for (const m of readFileSync(f, 'utf8').matchAll(/(?<![\w-])text-([a-z-]+)(?![\w-])/g)) if (colours.has(m[1] as string)) used.add(m[1] as string);
  }
  expect(used.size).toBeGreaterThan(0);
  const measured = new Set(PAIRS.map(([fg]) => fg));
  expect([...used].filter((c) => !measured.has(c)).sort()).toEqual([]);
});
