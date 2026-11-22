// Keel's tokens: the colour tokens live once in tailwind.config.js and once per appearance in
// styles.css, and the config replaces Tailwind's scales instead of extending them, so a value
// that is not a token emits nothing (DEC-72). The values are the owner's chosen look, mx3a Spectrum
// Plain (2026-11-22): Mistral's type (docs/work/2026-11-22-e-applist-w8-design-mistral.md) on
// MiniMax's colour (-design-minimax.md) - a white canvas, deep black ink, a black pill main action,
// outline pill secondaries, white cards on hairlines, no gradient; its night world is the dark one.
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

test('the colour tokens are mx3a\'s: the grounds, the rules, the ink, the black action and its outline twin, three state badges and the problem colour', () => {
  expect(colourVariables().sort()).toEqual(['canvas', 'card', 'edge', 'found', 'ink', 'ink-secondary', 'line', 'missing', 'on-found', 'on-missing', 'on-primary', 'on-set', 'outline', 'primary', 'problem', 'set']);
});

/** mx3a's colours, as R G B triplets: light is its day world, dark its night world. */
const DESIGN: Record<string, { light: string; dark: string }> = {
  canvas: { light: '255 255 255', dark: '16 16 16' },
  card: { light: '255 255 255', dark: '27 27 27' },
  line: { light: '238 238 240', dark: '46 46 53' },
  edge: { light: '226 226 230', dark: '56 56 64' },
  ink: { light: '17 17 17', dark: '244 244 246' },
  'ink-secondary': { light: '88 88 96', dark: '172 172 182' },
  primary: { light: '17 17 17', dark: '244 244 246' },
  'on-primary': { light: '255 255 255', dark: '16 16 16' },
  outline: { light: '17 17 17', dark: '200 200 208' },
  set: { light: '219 232 255', dark: '30 54 120' },
  'on-set': { light: '22 64 190', dark: '196 214 255' },
  found: { light: '223 245 230', dark: '22 72 44' },
  'on-found': { light: '22 101 52', dark: '186 240 204' },
  missing: { light: '214 52 36', dark: '214 52 36' },
  'on-missing': { light: '255 255 255', dark: '255 255 255' },
  problem: { light: '190 40 40', dark: '255 138 128' },
};

test('each colour is mx3a\'s value: day for light, night for dark', () => {
  const value = (body: string, v: string) => new RegExp(`--${v}\\s*:\\s*([\\d ]+);`).exec(body)?.[1];
  for (const [v, want] of Object.entries(DESIGN)) {
    expect(value(light(), v), `light --${v}`).toBe(want.light);
    expect(value(dark(), v), `dark --${v}`).toBe(want.dark);
  }
});

test('a secondary button is an outline with no fill; its hover is a faint wash of ink, 6% by day and 10% by night', () => {
  const alpha = (body: string, v: string) => new RegExp(`--${v}\\s*:\\s*([\\d.]+);`).exec(body)?.[1];
  expect([alpha(light(), 'hover-alpha'), alpha(dark(), 'hover-alpha')]).toEqual(['0.06', '0.1']);
  const opacity = (config.theme?.opacity ?? {}) as Record<string, string>;
  expect(opacity).toEqual({ disabled: '0.45', hover: 'var(--hover-alpha)' });
});

test('type: Mistral\'s faces on the Mac - New York (ui-serif) for the screen title and section heads, the system sans for the rest, mono for paths', () => {
  const families = (config.theme?.fontFamily ?? {}) as Record<string, string[]>;
  expect(Object.keys(families).sort()).toEqual(['mono', 'sans', 'serif']);
  expect(families.serif).toEqual(['ui-serif', 'New York', 'Georgia', 'serif']);
});

test('type: Mistral\'s scale - a 34px title, 20px section heads, both tracked in - and three weights, 400, 500 and 600', () => {
  const sizes = (config.theme?.fontSize ?? {}) as Record<string, [string, Record<string, string>]>;
  expect(sizes.heading).toEqual(['34px', { lineHeight: '38px', letterSpacing: '-0.5px' }]);
  expect(sizes.section).toEqual(['20px', { lineHeight: '26px', letterSpacing: '-0.2px' }]);
  expect(Object.keys(sizes)).not.toContain('eyebrow');
  expect(config.theme?.fontWeight).toEqual({ regular: '400', ui: '500', medium: '600' });
});

test('shapes and depth: 12px cards, pills for every button and badge, a 3px brand mark, and no shadow but none', () => {
  expect(config.theme?.borderRadius).toEqual({ none: '0px', mark: '3px', card: '12px', pill: '9999px' });
  expect(config.theme?.boxShadow).toEqual({ none: 'none' });
});

test('spacing is the 8px scale, a 4px half step, mx3a\'s 14px row, and the control heights', () => {
  expect(config.theme?.spacing).toEqual({ 0: '0px', px: '1px', half: '4px', xs: '8px', sm: '12px', row: '14px', md: '16px', lg: '20px', xl: '24px', xxl: '32px', huge: '40px', badge: '22px', control: '32px' });
});

test('no gradient anywhere: no background-image and no gradient-stop scale, and no gradient in styles.css', () => {
  expect(config.theme?.backgroundImage).toEqual({});
  expect(config.theme?.gradientColorStops).toEqual({});
  expect(css()).not.toMatch(/gradient/);
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

test('no variable in styles.css is unused: each block declares exactly the colour variables and the hover strength', () => {
  const want = [...colourVariables(), 'hover-alpha'].sort();
  expect(declared(light()).sort()).toEqual(want);
  expect(declared(dark()).sort()).toEqual(want);
});

test('dark follows the Mac: one prefers-color-scheme block, color-scheme light dark, and a light and a dark value that differ for every colour but the red missing badge and its label', () => {
  expect(css().match(/prefers-color-scheme/g)?.length).toBe(1);
  expect(css()).toMatch(/color-scheme:\s*light dark/);
  const value = (body: string, v: string) => new RegExp(`--${v}\\s*:\\s*([\\d ]+);`).exec(body)?.[1];
  for (const v of colourVariables().filter((c) => !['missing', 'on-missing'].includes(c))) expect(value(light(), v), v).not.toBe(value(dark(), v));
});

test('the system font and no font file: no @font-face and no url() in styles.css, the sans stack starts at system-ui', () => {
  expect(css()).not.toMatch(/@font-face|url\(/);
  const sans = (config.theme?.fontFamily as Record<string, string[]> | undefined)?.sans ?? [];
  expect(sans[0]).toBe('system-ui');
  expect(sans).toContain('-apple-system');
});

test('the config replaces, never extends: nothing under theme.extend, and each design scale is declared whole', () => {
  expect(Object.keys((config.theme?.extend ?? {}) as object)).toEqual([]);
  for (const scale of ['colors', 'spacing', 'fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'letterSpacing', 'borderRadius', 'borderWidth', 'boxShadow', 'backgroundImage', 'gradientColorStops', 'opacity', 'maxWidth', 'screens']) {
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
  const content = 'p-md bg-canvas bg-card text-ink text-body rounded-pill font-ui hover:bg-ink/hover text-sm p-5 bg-red-500 rounded-2xl font-bold font-semibold leading-7 text-ink/50 border-2 shadow-lg bg-gradient-to-r from-primary to-ink';
  const out = await postcss([tailwindcss({ ...config, content: [{ raw: content, extension: 'html' }] })]).process('@tailwind utilities;', { from: undefined });
  const emitted = new Set([...out.css.matchAll(/\.((?:\\.|[^\s{},:>~+[\]().])+)/g)].map((m) => (m[1] as string).replace(/\\/g, '')));
  for (const cls of ['p-md', 'bg-canvas', 'bg-card', 'text-ink', 'text-body', 'rounded-pill', 'font-ui', 'hover:bg-ink/hover']) expect(emitted.has(cls), cls).toBe(true);
  for (const cls of ['text-sm', 'p-5', 'bg-red-500', 'rounded-2xl', 'font-bold', 'font-semibold', 'leading-7', 'text-ink/50', 'border-2', 'shadow-lg', 'bg-gradient-to-r', 'from-primary', 'to-ink']) expect(emitted.has(cls), cls).toBe(false);
  expect(out.css).toContain('rgb(var(--ink) / var(--hover-alpha))');
});

test('the window\'s entry imports styles.css', () => {
  expect(readFileSync(path.join(APP, 'src', 'window', 'main.tsx'), 'utf8')).toMatch(/^import '\.\/styles\.css';$/m);
});

test('the spacing-derived scales the window uses are declared with exactly their members; padding and gap draw on the spacing tokens, height on them and full', () => {
  const theme = config.theme as Record<string, unknown>;
  expect(theme.width).toEqual({ full: '100%', sm: '12px' });
  expect(theme.minWidth).toEqual({ 0: '0px' });
  expect(theme.minHeight).toEqual({ full: '100%' });
  expect(theme.margin).toEqual({ 0: '0px', auto: 'auto' });
  const ours = resolveConfig(config).theme as Record<string, unknown>;
  for (const scale of ['padding', 'gap']) expect(ours[scale], scale).toEqual(ours.spacing);
  expect(ours.height).toEqual({ ...(ours.spacing as object), full: '100%' });
});
