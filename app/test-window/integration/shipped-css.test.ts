// Review round 3 (emitted.js:28): the CSS check judged styles.css's emitted class names alone, so a
// second stylesheet (a side-effect import, an @import, a <link>) and raw declarations under element
// selectors shipped while lint exited 0. Now the check finds every stylesheet the shipped entry pulls
// in through Vite's own module graph (middleware mode: no port, no build) and reads index.html for
// <link>, <style> and style=; anything but styles.css is refused. styles.css is judged at source,
// declaration by declaration: once compiled, a raw div{margin:7px} and an @apply of a token look
// alike. And it fails closed: content matching nothing, or Tailwind emitting no class, is refused.
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test } from 'vitest';
import { configProblems, contentProblems, emittedClasses, graphProblems, htmlProblems, packageProblems, shippedCssProblems, shippedSources, sourceProblems, stylesheetProblems } from '../../eslint-rules/emitted.js';
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

// Review round 4 (emitted.js:130): a template dynamic import puts Vite's virtual
// '\0vite/dynamic-import-helper.js' in the graph; the walk read it as a file and crashed lint.
test('a template dynamic import is honest: the walk skips virtual ids and finds nothing', async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'keel-css-'));
  try {
    mkdirSync(path.join(dir, 'ui'));
    writeFileSync(path.join(dir, 'index.html'), '<!doctype html><script type="module" src="./main.ts"></script>');
    writeFileSync(path.join(dir, 'styles.css'), 'p { color: black; }');
    writeFileSync(path.join(dir, 'ui', 'a.ts'), 'export const a = 1;\n');
    writeFileSync(path.join(dir, 'main.ts'), "import './styles.css';\nconst n = 'a';\nexport const load = () => import(`./ui/${n}.ts`);\n");
    expect(await graphProblems({ root: dir, configFile: false, entry: '/main.ts', allowed: path.join(dir, 'styles.css'), namer: path.join(dir, 'main.ts') })).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}, 20_000);

// Review round 4 (emitted.js:97): the walk shared node_modules/.vite with vitest and crashed with
// ERR_OUTDATED_OPTIMIZED_DEP when both ran at once. Each walk now owns a fresh cache, removed after.
test('each walk uses its own cache directory outside app/, fresh per run and removed after', async () => {
  const dirs: string[] = [];
  expect(await graphProblems({ onCacheDir: (d: string) => dirs.push(d) })).toEqual([]);
  expect(await graphProblems({ onCacheDir: (d: string) => dirs.push(d) })).toEqual([]);
  expect(dirs.length).toBe(2);
  expect(dirs[0]).not.toBe(dirs[1]);
  for (const d of dirs) {
    expect(realpathSync(path.dirname(d))).toBe(realpathSync(os.tmpdir()));
    expect(d.startsWith(APP)).toBe(false);
    expect(existsSync(d)).toBe(false);
  }
}, 40_000);

// Review round 4 (emitted.js:105): a CommonJS package's require('./x.css') never shows in Vite's dev
// graph, yet the build ships it. The rule for packages is simpler and complete: the shipped sources
// may import only packages listed in package.json's dependencies, and no package in their installed
// closure (dependencies, optional and peer, followed) may hold a stylesheet file at all, however it is
// reached (require, import, ?inline, ?raw).
function fakeApp(deps: Record<string, string>, packages: Record<string, Record<string, string>>): string {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'keel-pkg-'));
  writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'probe', dependencies: deps }));
  for (const [name, files] of Object.entries(packages)) {
    for (const [file, text] of Object.entries(files)) {
      const at = path.join(dir, 'node_modules', name, file);
      mkdirSync(path.dirname(at), { recursive: true });
      writeFileSync(at, text);
    }
  }
  return dir;
}

test('a stylesheet anywhere in a runtime dependency\'s closure is refused: a CommonJS require, a nested dependency', () => {
  const dir = fakeApp({ 'rogue-cjs': '1.0.0' }, {
    'rogue-cjs': { 'package.json': JSON.stringify({ name: 'rogue-cjs', main: 'index.js', dependencies: { 'deep-dep': '1.0.0' } }), 'index.js': "require('./x.css');\n", 'x.css': 'div { margin: 7px; }' },
    'deep-dep': { 'package.json': JSON.stringify({ name: 'deep-dep' }), 'lib/y.scss': 'p { color: red; }' },
  });
  try {
    const text = packageProblems(dir).join('\n');
    expect(text).toMatch(/rogue-cjs.*x\.css/);
    expect(text).toMatch(/deep-dep.*y\.scss/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a symlinked node_modules (a copy, a pnpm-style store) is followed: a nested dependency is still found and judged', () => {
  const dir = fakeApp({ host: '1.0.0' }, {
    host: { 'package.json': JSON.stringify({ name: 'host', dependencies: { inner: '1.0.0' } }), 'index.js': '' },
    inner: { 'package.json': JSON.stringify({ name: 'inner' }), 'z.css': 'p { color: red; }' },
  });
  const store = mkdtempSync(path.join(os.tmpdir(), 'keel-store-'));
  try {
    renameSync(path.join(dir, 'node_modules'), path.join(store, 'node_modules'));
    symlinkSync(path.join(store, 'node_modules'), path.join(dir, 'node_modules'));
    const problems = packageProblems(dir);
    expect(problems.join('\n')).toMatch(/inner.*z\.css/);
    expect(problems.join('\n')).not.toMatch(/not installed/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(store, { recursive: true, force: true });
  }
});

test('the real runtime dependencies (react, react-dom and their closure) hold no stylesheet', () => {
  expect(packageProblems()).toEqual([]);
});

test('a shipped source may import only a runtime dependency: a dev dependency or an unlisted package is refused, a type-only import is not', () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'keel-src-'));
  try {
    const file = path.join(dir, 'a.tsx');
    writeFileSync(file, "import { createRoot } from 'react-dom/client';\nimport type { Plugin } from 'vite';\nimport x from 'rogue-cjs';\nconst y = require('@scope/pkg/sub');\nconst z = await import('left-pad');\nexport { x, y, z, createRoot };\n");
    const text = sourceProblems([file]).join('\n');
    expect(text).toMatch(/rogue-cjs/);
    expect(text).toMatch(/@scope\/pkg/);
    expect(text).toMatch(/left-pad/);
    expect(text).not.toMatch(/react-dom|vite/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a shipped source may name no stylesheet and inject no style: require, ?raw, new URL, a style element, a constructed sheet', () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'keel-src-'));
  try {
    const file = path.join(dir, 'b.tsx');
    writeFileSync(file, "const a = require('./a.css');\nimport raw from './b.css?raw';\nconst u = new URL('./c.css', import.meta.url);\nconst s = document.createElement('style');\nconst t = new CSSStyleSheet();\ndocument.adoptedStyleSheets = [t];\nexport const E = () => <style>{raw}</style>;\nexport { a, u, s };\n");
    const text = sourceProblems([file]).join('\n');
    for (const named of [/a\.css/, /b\.css\?raw/, /c\.css/, /createElement\('style'\)/, /CSSStyleSheet/, /adoptedStyleSheets/, /<style/]) expect(text).toMatch(named);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the shipped sources are every file under src/window but browser mode\'s, and src/shared; the only stylesheet among them is styles.css', () => {
  const files = shippedSources().map((f: string) => path.relative(APP, f));
  expect(files).toContain(path.join('src', 'window', 'main.tsx'));
  expect(files).toContain(path.join('src', 'window', 'ui', 'Badge.tsx'));
  expect(files).toContain(path.join('src', 'shared', 'contract.ts'));
  expect(files.filter((f: string) => f.includes('browser') || f.includes(`${path.sep}fake${path.sep}`))).toEqual([]);
  expect(files.filter((f: string) => /\.(css|scss|less)$/.test(f))).toEqual([path.join('src', 'window', 'styles.css')]);
  expect(sourceProblems(shippedSources())).toEqual([]);
});

test('the renderer config adds no plugin and no CSS step the check has not judged', async () => {
  expect(await configProblems()).toEqual([]);
  const dir = mkdtempSync(path.join(os.tmpdir(), 'keel-cfg-'));
  try {
    const cfg = path.join(dir, 'vite.config.mjs');
    writeFileSync(cfg, "export default { plugins: [{ name: 'rogue:inject-style', transformIndexHtml: (h) => h }], css: { postcss: { plugins: [] }, preprocessorOptions: { scss: { additionalData: 'p{}' } } } };\n");
    const text = (await configProblems(cfg)).join('\n');
    expect(text).toMatch(/rogue:inject-style/);
    expect(text).toMatch(/preprocessorOptions/);
    expect(text).toMatch(/postcss/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}, 20_000);
