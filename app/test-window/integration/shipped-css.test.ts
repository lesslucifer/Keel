// Review round 3 (emitted.js:28): the CSS check judged styles.css's emitted class names alone, so a
// second stylesheet (a side-effect import, an @import, a <link>) and raw declarations under element
// selectors shipped while lint exited 0. Now the check finds every stylesheet the shipped entry pulls
// in through Vite's own module graph (middleware mode: no port, no build) and reads index.html for
// <link>, <style> and style=; anything but styles.css is refused. styles.css is judged at source,
// declaration by declaration: once compiled, a raw div{margin:7px} and an @apply of a token look
// alike. And it fails closed: content matching nothing, or Tailwind emitting no class, is refused.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test } from 'vitest';
import { contentProblems, emittedClasses, graphProblems, htmlProblems, shippedCssProblems, stylesheetProblems } from '../../eslint-rules/emitted.js';
import { SOURCE_GLOBS } from '../../window-sources.js';

const APP = path.resolve(import.meta.dirname, '..', '..');
const WINDOW = path.join(APP, 'src', 'window');

test('the real styles.css passes the source judgement', () => {
  expect(stylesheetProblems(readFileSync(path.join(WINDOW, 'styles.css'), 'utf8'))).toEqual([]);
});

const REFUSED: Array<[string, string, RegExp]> = [
  ['an element selector with a raw value', 'div { margin: 7px; }', /margin/],
  ['a class selector with a raw colour', '.rogue { color: hotpink; }', /color/],
  ['an @apply of an arbitrary value under an element', 'h2 { @apply p-[13px]; }', /p-\[13px\]/],
  ['an @apply of a default-scale class', 'h1 { @apply text-sm; }', /text-sm/],
  ['an @import', "@import './rogue2.css';", /@import/],
  ['a font face', '@font-face { font-family: x; src: url(x.woff); }', /@font-face/],
  ['a token valued with a raw colour', ':root { --canvas: hotpink; }', /--canvas/],
  ['a media query other than the dark one', '@media (min-width: 100px) { :root { --canvas: 1 2 3; } }', /min-width/],
  ['a layer other than Tailwind\'s three', '@layer rogue { p { @apply p-xs; } }', /rogue/],
  ['an !important on an @apply', 'p { @apply p-xs !important; }', /!important/],
  // Review round 4 (emitted.js:191) and its siblings: each context holds exactly its own shape.
  ['an @apply on body inside the dark block', '@media (prefers-color-scheme: dark) { body { @apply bg-primary text-ink-secondary; } }', /body/],
  ['an @apply on :root inside the dark block', '@media (prefers-color-scheme: dark) { :root { @apply text-ink; } }', /@apply/],
  ['color-scheme inside the dark block', '@media (prefers-color-scheme: dark) { :root { color-scheme: dark; } }', /color-scheme/],
  ['a media query nested in the dark block', '@media (prefers-color-scheme: dark) { @media (prefers-color-scheme: dark) { :root { --ink: 1 2 3; } } }', /@media/],
  ['an @apply on an element other than the page roots', '@layer base { section { @apply bg-primary; } }', /section/],
  ['an @apply outside @layer base', 'body { @apply bg-canvas; }', /body/],
  ['an @apply in another Tailwind layer', '@layer utilities { body { @apply p-xs; } }', /utilities/],
  ['a token not in the config', ':root { --tw-bg-opacity: 1; }', /--tw-bg-opacity/],
  ['a token rule on another selector', ':root, body { --ink: 1 2 3; }', /:root, body/],
  ['color-scheme outside :root', '@layer base { html { color-scheme: dark; } }', /color-scheme/],
  ['an @apply on :root at the top level', ':root { @apply text-ink; }', /@apply/],
];

for (const [what, css, named] of REFUSED) {
  test(`styles.css refuses ${what}`, () => {
    const problems = stylesheetProblems(css);
    expect(problems.length, css).toBeGreaterThan(0);
    expect(problems.join('\n')).toMatch(named);
  });
}

test('styles.css accepts the tokens, the dark block, Tailwind\'s directives and @apply of legal classes', () => {
  const css = "@tailwind base;\n@tailwind utilities;\n:root { color-scheme: light dark; --ink: 1 2 3; --hover-alpha: 0.06; }\n@media (prefers-color-scheme: dark) { :root { --ink: 4 5 6; } }\n@layer base { body { @apply bg-canvas text-ink; } }";
  expect(stylesheetProblems(css)).toEqual([]);
});

test('the real index.html loads main.tsx alone and carries no stylesheet', () => {
  expect(htmlProblems(readFileSync(path.join(WINDOW, 'index.html'), 'utf8'))).toEqual([]);
});

test('index.html refuses a <link>, a <style>, a style= attribute and a second script', () => {
  const page = readFileSync(path.join(WINDOW, 'index.html'), 'utf8');
  const plant = (html: string) => page.replace('</head>', `${html}\n  </head>`);
  expect(htmlProblems(plant('<link rel="stylesheet" href="./rogue3.css" />')).join('\n')).toMatch(/<link/);
  expect(htmlProblems(plant('<style>h1{font-size:77px}</style>')).join('\n')).toMatch(/<style/);
  expect(htmlProblems(page.replace('<div id="root">', '<div id="root" style="margin:7px">')).join('\n')).toMatch(/style=/);
  expect(htmlProblems(plant('<script type="module" src="./other.tsx"></script>')).join('\n')).toMatch(/script/);
});

test('the shipped graph, as Vite resolves it from main.tsx, pulls in styles.css and no other stylesheet', async () => {
  expect(await graphProblems()).toEqual([]);
}, 20_000);

test('a second stylesheet in the graph is refused: a side-effect import, a ?inline import, a stylesheet named for new URL', async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'keel-css-'));
  try {
    writeFileSync(path.join(dir, 'index.html'), '<!doctype html><script type="module" src="./main.ts"></script>');
    writeFileSync(path.join(dir, 'styles.css'), 'p { color: black; }');
    writeFileSync(path.join(dir, 'rogue.css'), '.rogue { color: hotpink; } div { margin: 7px; }');
    writeFileSync(path.join(dir, 'inline.css'), 'h1 { font-size: 77px; }');
    writeFileSync(path.join(dir, 'main.ts'), "import './styles.css';\nimport './rogue.css';\nimport './more.ts';\n");
    writeFileSync(path.join(dir, 'more.ts'), "import css from './inline.css?inline';\nexport const url = new URL('./linked.css', import.meta.url);\nexport default css;\n");
    const problems = await graphProblems({ root: dir, configFile: false, entry: '/main.ts', allowed: path.join(dir, 'styles.css'), namer: path.join(dir, 'main.ts') });
    const text = problems.join('\n');
    expect(text).toMatch(/rogue\.css/);
    expect(text).toMatch(/inline\.css/);
    expect(text).toMatch(/linked\.css/);
    expect(problems.filter((line) => /loads \S*styles\.css;/.test(line) || /names the stylesheet \.\/styles\.css;/.test(line))).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}, 20_000);

test('fail closed: content globs that match no file, or that miss a module the shipped page loads, are refused', () => {
  expect(contentProblems(SOURCE_GLOBS, [path.join(WINDOW, 'main.tsx')])).toEqual([]);
  expect(contentProblems(['src/window/**/*.nothing'], []).join('\n')).toMatch(/match no file/);
  expect(contentProblems(['src/window/ui/*.tsx'], [path.join(WINDOW, 'main.tsx')]).join('\n')).toMatch(/main\.tsx/);
});

test('fail closed: Tailwind emitting no class at all is refused', async () => {
  expect(await emittedClasses({ content: [{ raw: '', extension: 'html' }] })).toEqual([]);
  expect((await shippedCssProblems({ content: [{ raw: '', extension: 'html' }] })).join('\n')).toMatch(/no class/);
}, 20_000);

test('the whole check on the real tree finds nothing', async () => {
  expect(await shippedCssProblems()).toEqual([]);
}, 20_000);
