// Text contrast in both worlds (WCAG AA, 4.5:1 for body text), computed from styles.css's tokens:
// every text colour the window uses passes on every ground it sits on. Indigo fails as small text on
// a card, so no window file draws text in the primary colour (review round 2, browser.tsx:23).
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { expect, test } from 'vitest';

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

const PAIRS: Array<[string, string]> = [
  ['ink', 'canvas'],
  ['ink', 'card'],
  ['ink-secondary', 'canvas'],
  ['ink-secondary', 'card'],
  ['on-primary', 'primary'],
  ['canvas', 'ink'],
];

test('every text colour the window uses reaches 4.5:1 on its ground, by day and by night', () => {
  for (const [world, block] of [['light', light], ['dark', dark]] as const) {
    for (const [fg, bg] of PAIRS) expect(contrast(rgb(block, fg), rgb(block, bg)), `${world}: ${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
  }
});

test('indigo is below 4.5:1 as text on a card, so no window file draws text in the primary colour', () => {
  expect(contrast(rgb(light, 'primary'), rgb(light, 'card'))).toBeLessThan(4.5);
  const walk = (dir: string): string[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));
  const users = walk(WINDOW).filter((f) => /\.(tsx?|html)$/.test(f) && /\btext-primary\b/.test(readFileSync(f, 'utf8')));
  expect(users.map((f) => path.relative(WINDOW, f))).toEqual([]);
});
