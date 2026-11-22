// Keel's tokens: the colour tokens live once in tailwind.config.js and once per appearance in
// styles.css, and the config replaces Tailwind's scales instead of extending them, so a value
// that is not a token emits nothing (DEC-72).
import { readFileSync } from 'node:fs';
import path from 'node:path';
import postcss from 'postcss';
import tailwindcss from 'tailwindcss';
import resolveConfig from 'tailwindcss/resolveConfig.js';
import { expect, test } from 'vitest';
import config from '../../tailwind.config.js';

const APP = path.resolve(import.meta.dirname, '..', '..');
const css = () => readFileSync(path.join(APP, 'src', 'window', 'styles.css'), 'utf8');

/** The custom properties a CSS block declares, by name, from `body` (the text between its braces). */
function declared(body: string): string[] {
  return [...body.matchAll(/--([a-z][a-z0-9-]*)\s*:/g)].map((m) => m[1] as string);
}

/** The body of the first block opened by `opener` in `text`, braces balanced. */
function block(text: string, opener: RegExp): string {
  const m = opener.exec(text);
  if (m === null) return '';
  let depth = 0;
  const start = m.index + m[0].length;
  for (let i = start - 1; i < text.length; i += 1) {
    if (text[i] === '{') depth += 1;
    else if (text[i] === '}') {
      depth -= 1;
      if (depth === 0) return text.slice(start, i);
    }
  }
  return '';
}

const light = () => block(css().replace(/@media[^{]*\{[\s\S]*$/, ''), /:root\s*\{/);
const dark = () => block(block(css(), /@media\s*\(prefers-color-scheme:\s*dark\)\s*\{/), /:root\s*\{/);

/** The colour tokens drawn from a variable, by variable name. */
function colourVariables(): string[] {
  const colours = (config.theme?.colors ?? {}) as Record<string, unknown>;
  return Object.values(colours).flatMap((v) => {
    const m = typeof v === 'string' ? /^rgb\(var\(--([a-z0-9-]+)\) \/ <alpha-value>\)$/.exec(v) : null;
    return m === null ? [] : [m[1] as string];
  });
}

test('the colour tokens are the meanings screen 6 and W3 draw: canvas, surface, line, ink, ink-muted, accent, on-accent, ok, wait, fail', () => {
  expect(colourVariables().sort()).toEqual(['accent', 'canvas', 'fail', 'ink', 'ink-muted', 'line', 'ok', 'on-accent', 'surface', 'wait']);
});

test('every colour token is a variable or a CSS keyword: no colour value lives in the config itself', () => {
  const colours = (config.theme?.colors ?? {}) as Record<string, unknown>;
  for (const [name, v] of Object.entries(colours)) {
    if (['transparent', 'current'].includes(name)) continue;
    expect(v, name).toMatch(/^rgb\(var\(--[a-z0-9-]+\) \/ <alpha-value>\)$/);
  }
});

test('every colour variable has an R G B triplet in the light block and again in the dark block', () => {
  for (const [name, body] of [['light', light()], ['dark', dark()]] as const) {
    for (const v of colourVariables()) expect(body, `${name} --${v}`).toMatch(new RegExp(`--${v}\\s*:\\s*\\d{1,3} \\d{1,3} \\d{1,3}\\s*;`));
  }
});

test('no variable in styles.css is unused: each block declares exactly the colour variables', () => {
  expect(declared(light()).sort()).toEqual(colourVariables().sort());
  expect(declared(dark()).sort()).toEqual(colourVariables().sort());
});

test('dark follows the Mac: one prefers-color-scheme block, color-scheme light dark, and a light and a dark value that differ for every colour', () => {
  expect(css().match(/prefers-color-scheme/g)?.length).toBe(1);
  expect(css()).toMatch(/color-scheme:\s*light dark/);
  const value = (body: string, v: string) => new RegExp(`--${v}\\s*:\\s*([\\d ]+);`).exec(body)?.[1];
  for (const v of colourVariables()) expect(value(light(), v), v).not.toBe(value(dark(), v));
});

test('the system font and no font file: no @font-face and no url() in styles.css, the sans stack starts at system-ui', () => {
  expect(css()).not.toMatch(/@font-face|url\(/);
  const sans = (config.theme?.fontFamily as Record<string, string[]> | undefined)?.sans ?? [];
  expect(sans[0]).toBe('system-ui');
  expect(sans).toContain('-apple-system');
});

test('the config replaces, never extends: nothing under theme.extend, and each design scale is declared whole', () => {
  expect(Object.keys((config.theme?.extend ?? {}) as object)).toEqual([]);
  for (const scale of ['colors', 'spacing', 'fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'borderRadius', 'borderWidth', 'opacity', 'maxWidth', 'screens']) {
    expect(Object.hasOwn(config.theme ?? {}, scale), scale).toBe(true);
  }
});

test('no declared scale equals Tailwind\'s default for it', () => {
  const stock = resolveConfig({ content: [] }).theme as Record<string, unknown>;
  const ours = resolveConfig(config).theme as Record<string, unknown>;
  for (const scale of Object.keys(config.theme ?? {})) {
    if (scale === 'extend') continue;
    expect(JSON.stringify(ours[scale]), scale).not.toBe(JSON.stringify(stock[scale]));
  }
});

test('Tailwind emits Keel\'s tokens and nothing from its own default scales', async () => {
  const content = 'p-2 bg-canvas text-ink text-body rounded-control font-medium text-sm p-5 bg-red-500 rounded-lg font-bold leading-7 text-ink/50 border-2';
  const out = await postcss([tailwindcss({ ...config, content: [{ raw: content, extension: 'html' }] })]).process('@tailwind utilities;', { from: undefined });
  const emitted = new Set([...out.css.matchAll(/\.((?:\\.|[^\s{},:>~+[\]().])+)/g)].map((m) => (m[1] as string).replace(/\\/g, '')));
  for (const cls of ['p-2', 'bg-canvas', 'text-ink', 'text-body', 'rounded-control', 'font-medium']) expect(emitted.has(cls), cls).toBe(true);
  for (const cls of ['text-sm', 'p-5', 'bg-red-500', 'rounded-lg', 'font-bold', 'leading-7', 'text-ink/50', 'border-2']) expect(emitted.has(cls), cls).toBe(false);
});

test('the window\'s entry imports styles.css', () => {
  expect(readFileSync(path.join(APP, 'src', 'window', 'main.tsx'), 'utf8')).toMatch(/^import '\.\/styles\.css';$/m);
});
