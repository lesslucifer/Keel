// Keel's tokens: the colour tokens live once in tailwind.config.js and once per appearance in
// styles.css, and the config replaces Tailwind's scales instead of extending them, so a value
// that is not a token emits nothing (DEC-72). The values follow the owner's design file
// (docs/work/2026-11-22-e-applist-w8-design.md, Mercury): its day world is the light appearance,
// its night world the dark one.
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

test('the colour tokens are the design file\'s: canvas, card, line, ink, ink-secondary, primary, on-primary, tint - no status green, amber or red', () => {
  expect(colourVariables().sort()).toEqual(['canvas', 'card', 'ink', 'ink-secondary', 'line', 'on-primary', 'primary', 'tint']);
});

/** The design file's colours, as R G B triplets: light is its day world, dark its night world. */
const DESIGN: Record<string, { light: string; dark: string }> = {
  canvas: { light: '251 252 253', dark: '23 23 33' },
  card: { light: '237 236 251', dark: '30 30 42' },
  line: { light: '195 195 204', dark: '112 112 125' },
  ink: { light: '40 37 59', dark: '237 237 243' },
  'ink-secondary': { light: '85 78 123', dark: '195 195 204' },
  primary: { light: '82 102 235', dark: '82 102 235' },
  'on-primary': { light: '255 255 255', dark: '255 255 255' },
  tint: { light: '82 102 235', dark: '156 180 232' },
};

test('each colour is the design file\'s value: day for light, night for dark', () => {
  const value = (body: string, v: string) => new RegExp(`--${v}\\s*:\\s*([\\d ]+);`).exec(body)?.[1];
  for (const [v, want] of Object.entries(DESIGN)) {
    expect(value(light(), v), `light --${v}`).toBe(want.light);
    expect(value(dark(), v), `dark --${v}`).toBe(want.dark);
  }
});

test('the translucent pill\'s strength flips with the world: indigo at 10% by day, periwinkle at 20% by night', () => {
  const alpha = (body: string, v: string) => new RegExp(`--${v}\\s*:\\s*([\\d.]+);`).exec(body)?.[1];
  expect([alpha(light(), 'tint-alpha'), alpha(dark(), 'tint-alpha')]).toEqual(['0.1', '0.2']);
  expect(Number(alpha(light(), 'tint-hover-alpha'))).toBeGreaterThan(0.1);
  expect(Number(alpha(dark(), 'tint-hover-alpha'))).toBeGreaterThan(0.2);
  const opacity = (config.theme?.opacity ?? {}) as Record<string, string>;
  expect(opacity).toEqual({ disabled: '0.45', tint: 'var(--tint-alpha)', 'tint-hover': 'var(--tint-hover-alpha)' });
});

test('type is light: every weight is 360 to 500, nothing above 500', () => {
  const weights = Object.values((config.theme?.fontWeight ?? {}) as Record<string, string>).map(Number).sort((a, b) => a - b);
  expect(weights).toEqual([360, 400, 420, 480, 500]);
});

test('shapes and depth: capsule radii from the design file, and no shadow but none', () => {
  expect(config.theme?.borderRadius).toEqual({ none: '0px', xs: '4px', sm: '8px', md: '12px', lg: '16px', xl: '32px', pill: '40px', full: '9999px' });
  expect(config.theme?.boxShadow).toEqual({ none: 'none' });
});

test('spacing is the design file\'s 8px scale, plus a 4px half step and the control heights', () => {
  expect(config.theme?.spacing).toEqual({ 0: '0px', px: '1px', half: '4px', xs: '8px', sm: '12px', md: '16px', lg: '20px', xl: '24px', xxl: '32px', huge: '40px', badge: '22px', control: '32px' });
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

test('no variable in styles.css is unused: each block declares exactly the colour variables and the two pill strengths', () => {
  const want = [...colourVariables(), 'tint-alpha', 'tint-hover-alpha'].sort();
  expect(declared(light()).sort()).toEqual(want);
  expect(declared(dark()).sort()).toEqual(want);
});

test('dark follows the Mac: one prefers-color-scheme block, color-scheme light dark, and a light and a dark value that differ for every colour but the indigo action and its label', () => {
  expect(css().match(/prefers-color-scheme/g)?.length).toBe(1);
  expect(css()).toMatch(/color-scheme:\s*light dark/);
  const value = (body: string, v: string) => new RegExp(`--${v}\\s*:\\s*([\\d ]+);`).exec(body)?.[1];
  for (const v of colourVariables().filter((c) => !['primary', 'on-primary'].includes(c))) expect(value(light(), v), v).not.toBe(value(dark(), v));
});

test('the system font and no font file: no @font-face and no url() in styles.css, the sans stack starts at system-ui', () => {
  expect(css()).not.toMatch(/@font-face|url\(/);
  const sans = (config.theme?.fontFamily as Record<string, string[]> | undefined)?.sans ?? [];
  expect(sans[0]).toBe('system-ui');
  expect(sans).toContain('-apple-system');
});

test('the config replaces, never extends: nothing under theme.extend, and each design scale is declared whole', () => {
  expect(Object.keys((config.theme?.extend ?? {}) as object)).toEqual([]);
  for (const scale of ['colors', 'spacing', 'fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'letterSpacing', 'borderRadius', 'borderWidth', 'boxShadow', 'opacity', 'maxWidth', 'screens']) {
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
  const content = 'p-md bg-canvas bg-card text-ink text-body rounded-pill font-ui bg-tint/tint text-sm p-5 bg-red-500 rounded-2xl font-bold font-semibold leading-7 text-ink/50 border-2 shadow-lg';
  const out = await postcss([tailwindcss({ ...config, content: [{ raw: content, extension: 'html' }] })]).process('@tailwind utilities;', { from: undefined });
  const emitted = new Set([...out.css.matchAll(/\.((?:\\.|[^\s{},:>~+[\]().])+)/g)].map((m) => (m[1] as string).replace(/\\/g, '')));
  for (const cls of ['p-md', 'bg-canvas', 'bg-card', 'text-ink', 'text-body', 'rounded-pill', 'font-ui', 'bg-tint/tint']) expect(emitted.has(cls), cls).toBe(true);
  for (const cls of ['text-sm', 'p-5', 'bg-red-500', 'rounded-2xl', 'font-bold', 'font-semibold', 'leading-7', 'text-ink/50', 'border-2', 'shadow-lg']) expect(emitted.has(cls), cls).toBe(false);
  expect(out.css).toContain('rgb(var(--tint) / var(--tint-alpha))');
});

test('the window\'s entry imports styles.css', () => {
  expect(readFileSync(path.join(APP, 'src', 'window', 'main.tsx'), 'utf8')).toMatch(/^import '\.\/styles\.css';$/m);
});

test('the spacing-derived scales the window uses are declared with exactly their members; padding, gap and height draw on the spacing tokens', () => {
  const theme = config.theme as Record<string, unknown>;
  expect(theme.width).toEqual({ full: '100%' });
  expect(theme.minWidth).toEqual({ 0: '0px' });
  expect(theme.minHeight).toEqual({ full: '100%' });
  expect(theme.margin).toEqual({ 0: '0px', auto: 'auto' });
  const ours = resolveConfig(config).theme as Record<string, unknown>;
  for (const scale of ['padding', 'gap', 'height']) expect(ours[scale], scale).toEqual(ours.spacing);
});
