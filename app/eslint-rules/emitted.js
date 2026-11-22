// @ts-check
// The guarantee that only Keel's vocabulary reaches the shipped CSS, in four parts, all in memory (no
// build, no port):
// 1. One stylesheet, in five checks that each see what the others cannot:
//    - Vite's own module graph, walked from main.tsx in middleware mode with a private cache, lists
//      the stylesheets the page's local modules load (a side-effect import, ?inline, ?raw, ?url);
//    - the shipped sources (every file under src/window but browser mode's, and src/shared) may hold
//      no stylesheet file but styles.css, name none (an import, a require, new URL, a file reached
//      only through a template dynamic import), put none on the page at run time (a style element,
//      a constructed sheet), and import only packages listed in package.json's dependencies; the
//      graph may load no local file outside them;
//    - no package in the installed closure of those dependencies holds a stylesheet file at all, so a
//      CommonJS require('./x.css') inside one, which a dev graph never shows, is refused too;
//    - index.html carries no <link>, <style> or style= and loads main.tsx alone;
//    - the renderer config adds no plugin the check has not judged, and no CSS step but Tailwind.
//    Anything but src/window/styles.css is refused.
// 2. styles.css judged at source against an exact grammar: Tailwind's three directives; one :root
//    rule of tokens (variables the config reads, as numbers) and color-scheme; the dark media query
//    holding only :root rules of tokens; @layer base holding only @apply of legal classes on html,
//    body and #root. Nothing else (no raw property, no other selector, @import, font, media or
//    layer). It must be judged before Tailwind runs: compiled, a raw div{margin:7px} and an @apply
//    of a token look alike.
// 3. The emitted classes. Tailwind writes CSS only for whole class names it finds written out in its
//    content files, however a string is built later, so styles.css through Tailwind's PostCSS plugin
//    with the shipped config (as vite.renderer.config.ts runs it), over the one content list, must
//    emit only classes the closed vocabulary holds. What Tailwind writes for a legal class (and its
//    own preflight) is valued from the closed tokens, so the selector is what is judged.
// 4. Fail closed: content globs matching no file, or missing a module the page loads, and Tailwind
//    emitting no class at all, are refused rather than passed unjudged.
// Route: review round 3 chose Vite's resolver over a renderer build (the owner keeps builds out of the
// inner loop); round 4 showed a dev graph misses a package's CommonJS require, so packages are judged
// whole instead (the simplest complete rule: no runtime dependency may hold a stylesheet at all).

import { existsSync, globSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import postcss from 'postcss';
import tailwindcss from 'tailwindcss';
import { createServer, loadConfigFromFile } from 'vite';
import tailwindConfig from '../tailwind.config.js';
import { SOURCE_GLOBS } from '../window-sources.js';
import { classify } from './classes.js';

const APP = path.join(import.meta.dirname, '..');
const WINDOW = path.join(APP, 'src', 'window');
const STYLES = path.join(WINDOW, 'styles.css');
const INDEX = path.join(WINDOW, 'index.html');
const ENTRY = path.join(WINDOW, 'main.tsx');

/** A stylesheet by its file name or import id, a query allowed (x.css, x.scss?inline). */
const SHEET = /\.(?:css|less|sass|scss|styl|stylus|pcss|postcss|sss)(?:\?[^'"`]*)?$/;
/** A quoted stylesheet name in a source file. */
const NAMED_SHEET = /['"`]([^'"`\n]*\.(?:css|less|sass|scss|styl|stylus|pcss|postcss|sss)(?:\?[^'"`\n]*)?)['"`]/g;

/** The class names in a CSS selector, unescaped (.hover\:bg-ink\/hover:hover -> hover:bg-ink/hover). */
function classesIn(/** @type {string} */ selector) {
  return [...selector.matchAll(/\.((?:\\.|[^\s{},:>~+[\]().#*="'|^$])+)/g)].map((m) => /** @type {string} */ (m[1]).replace(/\\(.)/g, '$1'));
}

/**
 * Every class Tailwind emits for the shipped stylesheet, sorted; `content` replaces the content list (a test's probe).
 * @param {{ content?: unknown[] }} [options]
 */
export async function emittedClasses(options = {}) {
  const config = options.content === undefined ? tailwindConfig : { ...tailwindConfig, content: options.content };
  const out = await postcss([tailwindcss(/** @type {any} */ (config))]).process(readFileSync(STYLES, 'utf8'), { from: STYLES });
  const found = new Set();
  out.root.walkRules((rule) => {
    for (const cls of classesIn(rule.selector)) found.add(cls);
  });
  return [...found].sort();
}

/**
 * The emitted classes outside Keel's closed vocabulary, sorted: empty when the guarantee holds.
 * @param {{ content?: unknown[] }} [options]
 */
export async function offVocabulary(options = {}) {
  return (await emittedClasses(options)).filter((cls) => !classify(cls).legal);
}

// ---- 1. One stylesheet ----

/** The packages the window may import at run time: package.json's dependencies (and optional ones). */
function runtimeDependencies(/** @type {string} */ app = APP) {
  const pkg = JSON.parse(readFileSync(path.join(app, 'package.json'), 'utf8'));
  return new Set([...Object.keys(pkg.dependencies ?? {}), ...Object.keys(pkg.optionalDependencies ?? {})]);
}

/**
 * The modules Vite loads for the shipped page, walked from `entry` through its own resolver in
 * middleware mode (no port, no build, no dependency scan): the stylesheets (ids, queries kept) and the
 * local files (real paths). A stylesheet's own dependencies are not followed: Tailwind registers its
 * content files there, which the page never loads. A package (node_modules, or Vite's pre-bundled deps)
 * is not walked: packages are judged by packageProblems, which sees what a dev graph cannot (a
 * CommonJS require of a stylesheet, review round 4). A virtual id (\0..., or one with no file on disk,
 * like Vite's dynamic-import helper) is walked through but is no source. Each walk owns a fresh cache
 * directory under the OS temp dir, removed after: a shared node_modules/.vite let a parallel vitest
 * run re-optimise the deps under it (ERR_OUTDATED_OPTIMIZED_DEP, review round 4).
 * @param {{ root?: string, configFile?: string | false, entry?: string, onCacheDir?: (dir: string) => void }} [options]
 */
async function walkShipped(options = {}) {
  const cacheDir = mkdtempSync(path.join(os.tmpdir(), 'keel-vite-'));
  options.onCacheDir?.(cacheDir);
  try {
    return await walkIn(cacheDir, options);
  } finally {
    rmSync(cacheDir, { recursive: true, force: true });
  }
}

/**
 * @param {string} cacheDir
 * @param {{ root?: string, configFile?: string | false, entry?: string }} options
 */
async function walkIn(cacheDir, options) {
  const server = await createServer({
    cacheDir,
    configFile: options.configFile ?? path.join(APP, 'vite.renderer.config.ts'),
    ...(options.root === undefined ? {} : { root: options.root }),
    logLevel: 'silent',
    server: { middlewareMode: true, hmr: false, ws: false, preTransformRequests: false },
    optimizeDeps: { noDiscovery: true, entries: [] },
  });
  const caches = [cacheDir, realpathSync(cacheDir)];
  const isPackage = (/** @type {string | null | undefined} */ file) =>
    typeof file === 'string' && (file.includes(`${path.sep}node_modules${path.sep}`) || caches.some((c) => file.startsWith(c + path.sep)));
  try {
    /** @type {string[]} */
    const stylesheets = [];
    /** @type {string[]} */
    const sources = [];
    const seen = new Set();
    const queue = [options.entry ?? '/main.tsx'];
    while (queue.length > 0) {
      const url = /** @type {string} */ (queue.shift());
      if (seen.has(url)) continue;
      seen.add(url);
      await server.transformRequest(url);
      const mod = await server.moduleGraph.getModuleByUrl(url);
      const id = mod?.id ?? url;
      if (SHEET.test(id.split('?')[0] ?? '') || SHEET.test(id)) {
        stylesheets.push(id);
        continue;
      }
      const real = !id.startsWith('\0') && typeof mod?.file === 'string' && !mod.file.includes('\0') && existsSync(mod.file);
      if (real) sources.push(realpathSync(/** @type {string} */ (mod?.file)));
      for (const next of mod?.importedModules ?? []) {
        if (isPackage(next.file) || isPackage(next.id)) continue;
        queue.push(next.url);
      }
    }
    return { stylesheets, sources };
  } finally {
    await server.close();
  }
}

/** Every stylesheet id in the graph but `allowed`. */
function stylesheetsBeyond(/** @type {{ stylesheets: string[] }} */ graph, /** @type {string} */ allowed) {
  return graph.stylesheets
    .filter((id) => id !== allowed)
    .map((id) => `design/css: the shipped page loads ${path.relative(APP, id)}; only ${path.relative(APP, allowed)} may carry its CSS`);
}

/** The package a bare specifier names (react-dom/client -> react-dom, @scope/pkg/sub -> @scope/pkg), or null. */
function packageOf(/** @type {string} */ spec) {
  if (/^(?:\.|\/|[a-z]+:|\0|virtual:)/i.test(spec)) return null;
  const parts = spec.split('/');
  return spec.startsWith('@') ? parts.slice(0, 2).join('/') : /** @type {string} */ (parts[0]);
}

/** Every import, re-export, dynamic import and require in `text`, with whether it is type-only. */
function specifiersIn(/** @type {string} */ text) {
  /** @type {Array<{ spec: string, typeOnly: boolean }>} */
  const out = [];
  for (const m of text.matchAll(/\bfrom\s*(['"])([^'"\n]+)\1/g)) {
    const before = text.slice(0, m.index);
    const start = Math.max(before.lastIndexOf('import'), before.lastIndexOf('export'));
    out.push({ spec: /** @type {string} */ (m[2]), typeOnly: start >= 0 && /^(?:import|export)\s+type\b/.test(text.slice(start)) });
  }
  for (const m of text.matchAll(/\b(?:import\s*\(\s*|require\s*\(\s*|import\s+)(['"])([^'"\n]+)\1/g)) out.push({ spec: /** @type {string} */ (m[2]), typeOnly: false });
  return out;
}

/** Ways to put a stylesheet on the page at run time, which no CSS check can see. */
const INJECTIONS = [
  { pattern: /\bcreateElement\(\s*['"`](style|link)['"`]/g, says: (/** @type {RegExpMatchArray} */ m) => `createElement('${m[1]}')` },
  { pattern: /\bCSSStyleSheet\b/g, says: () => 'CSSStyleSheet' },
  { pattern: /\badoptedStyleSheets\b/g, says: () => 'adoptedStyleSheets' },
  { pattern: /\binsertRule\b/g, says: () => 'insertRule' },
  { pattern: /\.styleSheets\b/g, says: () => 'document.styleSheets' },
  { pattern: /<(style|link)\b/g, says: (/** @type {RegExpMatchArray} */ m) => `<${m[1]}>` },
];

/**
 * What the shipped sources (`files`) do beyond importing styles.css once from main.tsx: a stylesheet
 * file among them, a stylesheet named (import, require, ?inline, ?raw, new URL, any quoted name), a
 * style put on the page at run time, or a package imported that is not a runtime dependency.
 * @param {string[]} files
 * @param {{ allowed?: string, namer?: string, packages?: boolean }} [options]
 */
export function sourceProblems(files, options = {}) {
  // Vite reports real paths (macOS: /tmp is /private/tmp), so both sides are compared real.
  const allowed = realpathSync(options.allowed ?? STYLES);
  const namer = realpathSync(options.namer ?? ENTRY);
  const deps = options.packages === false ? null : runtimeDependencies();
  /** @type {string[]} */
  const problems = [];
  for (const given of files) {
    const file = realpathSync(given);
    const rel = path.relative(APP, file);
    if (SHEET.test(file)) {
      if (file !== allowed) problems.push(`design/css: ${rel} is a stylesheet among the shipped sources; only ${path.relative(APP, allowed)} may be one`);
      continue;
    }
    // A declaration file (env.d.ts: declare module '*.css') is types only; nothing in it runs or ships.
    if (file.endsWith('.d.ts')) continue;
    const text = readFileSync(file, 'utf8');
    const names = [...text.matchAll(NAMED_SHEET)].map((m) => /** @type {string} */ (m[1]));
    const own = file === namer ? names.filter((n) => path.resolve(path.dirname(file), n) === allowed) : [];
    const extra = file === namer && own.length === 1 ? names.filter((n) => n !== own[0]) : names;
    for (const name of extra) problems.push(`design/css: ${rel} names the stylesheet ${name}; only ${path.relative(APP, namer)} may, and only ${path.relative(APP, allowed)}`);
    for (const { pattern, says } of INJECTIONS) {
      for (const m of text.matchAll(pattern)) problems.push(`design/css: ${rel} puts a style on the page at run time (${says(m)}); the page's only CSS is styles.css`);
    }
    if (deps === null) continue;
    for (const { spec, typeOnly } of specifiersIn(text)) {
      const pkg = packageOf(spec);
      if (pkg === null || typeOnly || deps.has(pkg)) continue;
      problems.push(`design/css: ${rel} imports ${pkg}, which is not a runtime dependency (package.json dependencies); only those are judged for stylesheets`);
    }
  }
  return problems;
}

/** Every file under `dir`, recursively, skipping directories `skip` names. */
function filesUnder(/** @type {string} */ dir, /** @type {(entry: string) => boolean} */ skip = () => false) {
  /** @type {string[]} */
  const out = [];
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const at = path.join(dir, e.name);
    if (skip(at)) continue;
    if (e.isDirectory()) out.push(...filesUnder(at, skip));
    else if (e.isFile()) out.push(at);
  }
  return out;
}

/** Browser mode's files, which the shipped page never loads (node guards in app/test/unit/guards.test.ts). */
const BROWSER_ONLY = [path.join(WINDOW, 'browser.tsx'), path.join(WINDOW, 'browser.html'), path.join(WINDOW, 'fake')];

/** The shipped sources: every file under src/window but browser mode's, and every file under src/shared. */
export function shippedSources() {
  const skip = (/** @type {string} */ at) => BROWSER_ONLY.includes(at);
  return [...filesUnder(WINDOW, skip), ...filesUnder(path.join(APP, 'src', 'shared'))].map((f) => realpathSync(f)).sort();
}

/**
 * Every stylesheet file in the installed closure of the runtime dependencies (dependencies, optional
 * and peer ones, followed through each package.json), however it would be reached; and a runtime
 * dependency that is not installed (nothing could be judged).
 * @param {string} [app]
 */
export function packageProblems(app = APP) {
  /** @type {string[]} */
  const problems = [];
  const root = realpathSync(app);
  /** @type {Array<{ name: string, from: string, required: boolean }>} */
  const queue = [...runtimeDependencies(root)].map((name) => ({ name, from: root, required: true }));
  const seen = new Set();
  // Node's own lookup: up from the requiring package through each node_modules, ending at the app's
  // root or at the real home of its node_modules when that is a link (a copy, a pnpm-style store).
  const modules = path.join(root, 'node_modules');
  const stops = new Set([root, existsSync(modules) ? path.dirname(realpathSync(modules)) : root]);
  const locate = (/** @type {string} */ name, /** @type {string} */ from) => {
    for (let d = from; ; d = path.dirname(d)) {
      const at = path.join(d, 'node_modules', name);
      if (existsSync(path.join(at, 'package.json'))) return realpathSync(at);
      if (stops.has(d) || d === path.dirname(d)) break;
    }
    const top = path.join(modules, name);
    return existsSync(path.join(top, 'package.json')) ? realpathSync(top) : null;
  };
  while (queue.length > 0) {
    const { name, from, required } = /** @type {{ name: string, from: string, required: boolean }} */ (queue.shift());
    const dir = locate(name, from);
    if (dir === null) {
      if (required) problems.push(`design/css: the runtime dependency ${name} is not installed, so it could not be judged`);
      continue;
    }
    if (seen.has(dir)) continue;
    seen.add(dir);
    for (const f of filesUnder(dir, (at) => path.basename(at) === 'node_modules')) {
      if (SHEET.test(f)) problems.push(`design/css: runtime dependency ${name} holds the stylesheet ${path.relative(dir, f)}; a package may bring no CSS to the page`);
    }
    const pkg = JSON.parse(readFileSync(path.join(dir, 'package.json'), 'utf8'));
    for (const d of Object.keys(pkg.dependencies ?? {})) queue.push({ name: d, from: dir, required: true });
    for (const d of [...Object.keys(pkg.optionalDependencies ?? {}), ...Object.keys(pkg.peerDependencies ?? {})]) queue.push({ name: d, from: dir, required: false });
  }
  return problems;
}

/** The renderer config's plugins: plugin-react's and Keel's dev CSP, nothing else. Another needs judging first. */
const ALLOWED_PLUGINS = new Set([
  'vite:react-babel',
  'vite:react:refresh-wrapper',
  'vite:react:config-post',
  'vite:react-refresh-fbm',
  'vite:react-refresh',
  'vite:react-virtual-preamble',
  'keel:dev-csp',
]);

/**
 * What the renderer config adds to the page's CSS beyond Tailwind: a plugin not judged (one could
 * inject a style into the page or a module), a CSS option but postcss, a PostCSS plugin but Tailwind.
 * @param {string} [configFile]
 */
export async function configProblems(configFile = path.join(APP, 'vite.renderer.config.ts')) {
  const loaded = await loadConfigFromFile({ command: 'build', mode: 'production' }, configFile, undefined, 'silent');
  const config = loaded?.config ?? {};
  /** @type {string[]} */
  const problems = [];
  /** @type {(p: any) => any[]} */
  const flat = (p) => (Array.isArray(p) ? p.flatMap(flat) : p ? [p] : []);
  for (const plugin of flat(config.plugins)) {
    if (!ALLOWED_PLUGINS.has(plugin.name)) problems.push(`design/css: the renderer config adds the plugin ${plugin.name}, which the CSS check has not judged`);
  }
  const css = /** @type {Record<string, any>} */ (config.css ?? {});
  for (const key of Object.keys(css)) if (key !== 'postcss') problems.push(`design/css: the renderer config sets css.${key}; only css.postcss, with Tailwind alone`);
  const post = (css.postcss?.plugins ?? []).map((/** @type {any} */ p) => p?.postcssPlugin ?? p?.name ?? String(p));
  if (post.length !== 1 || post[0] !== 'tailwindcss') problems.push(`design/css: the renderer config's postcss plugins are [${post.join(', ')}]; Tailwind alone`);
  return problems;
}

/**
 * The graph part of the check, for the real page or a probe project: a stylesheet Vite loads beyond
 * `allowed`, and in the local files it loads a stylesheet named or injected (packages are left to
 * packageProblems).
 * @param {{ root?: string, configFile?: string | false, entry?: string, allowed?: string, namer?: string, onCacheDir?: (dir: string) => void }} [options]
 */
export async function graphProblems(options = {}) {
  const graph = await walkShipped(options);
  const allowed = realpathSync(options.allowed ?? STYLES);
  return [...stylesheetsBeyond(graph, allowed), ...sourceProblems(graph.sources, { ...options, packages: false })];
}

/** What index.html carries beyond its one script, main.tsx: a <link>, a <style>, a style=, another script. */
export function htmlProblems(/** @type {string} */ html) {
  /** @type {string[]} */
  const problems = [];
  for (const m of html.matchAll(/<link\b[^>]*>/gi)) problems.push(`design/css: index.html carries ${m[0]}; the page's only CSS is styles.css, imported by main.tsx`);
  for (const m of html.matchAll(/<style\b[^>]*>/gi)) problems.push(`design/css: index.html carries ${m[0]}; the page's only CSS is styles.css, imported by main.tsx`);
  for (const m of html.matchAll(/<[a-z][^>]*\sstyle\s*=[^>]*>/gi)) problems.push(`design/css: index.html carries a style= attribute: ${m[0]}`);
  const scripts = [...html.matchAll(/<script\b[^>]*>/gi)].map((m) => m[0]);
  const one = scripts.length === 1 && /\stype="module"/.test(scripts[0] ?? '') && /\ssrc="\.\/main\.tsx"/.test(scripts[0] ?? '');
  if (!one) problems.push(`design/css: index.html must load one script, <script type="module" src="./main.tsx">; it has ${scripts.length === 0 ? 'none' : scripts.join(' ')}`);
  return problems;
}

// ---- 2. styles.css at source ----

const TAILWIND_PARTS = new Set(['base', 'components', 'utilities']);
const COLOR_SCHEMES = new Set(['light dark', 'dark light', 'light', 'dark']);
/** A token's value: an R G B triplet or an alpha, numbers only. */
const TOKEN_VALUE = /^\d+(?:\.\d+)?(?:\s+\d+(?:\.\d+)?)*$/;
/** The variables the config reads (rgb(var(--ink) ...), var(--hover-alpha)): the only tokens styles.css may set. */
const TOKEN_NAMES = new Set([...JSON.stringify(tailwindConfig.theme ?? {}).matchAll(/var\(--([a-z][a-z0-9-]*)\)/g)].map((m) => `--${m[1]}`));
/** The page's roots, the only elements styles.css may style, and only by @apply of tokens. */
const PAGE_ROOTS = new Set(['html', 'body', '#root']);

/**
 * Every construct in `css` (styles.css's text) outside its grammar, which is exact per context:
 * - top level: @tailwind base, components or utilities; one :root rule of tokens (variables the config
 *   reads, valued as numbers) and color-scheme; @media (prefers-color-scheme: dark); @layer base;
 * - in the dark media query: :root rules of tokens only (it flips the tokens, nothing else);
 * - in @layer base: rules on the page's roots (html, body, #root) holding only @apply of legal classes.
 * Nothing else: no raw property, no other selector, no import rule, no font, no other media or layer.
 * @param {string} css
 */
export function stylesheetProblems(css) {
  /** @type {string[]} */
  const problems = [];
  const say = (/** @type {string} */ what) => problems.push(`design/css: styles.css holds ${what}`);
  const params = (/** @type {any} */ node) => String(node.params).trim().replace(/\s+/g, ' ');
  const selector = (/** @type {any} */ rule) => String(rule.selector).trim().replace(/\s+/g, ' ');
  const where = (/** @type {any} */ node) => (node.type === 'rule' ? selector(node) : node.type === 'atrule' ? `@${node.name} ${params(node)}`.trim() : node.type === 'decl' ? `${node.prop}: ${node.value}` : node.type);

  /** A :root rule of tokens; `scheme` allows color-scheme too (the top-level one). */
  const tokenRule = (/** @type {any} */ rule, /** @type {boolean} */ scheme, /** @type {string} */ context) => {
    rule.each((/** @type {any} */ node) => {
      if (node.type === 'comment') return;
      if (node.type !== 'decl') return say(`${where(node)} in ${context}; only tokens go there`);
      const value = String(node.value).trim();
      if (node.important) return say(`${node.prop}: ${value} !important`);
      if (node.prop === 'color-scheme' && scheme) {
        if (!COLOR_SCHEMES.has(value)) say(`color-scheme: ${value}`);
        return;
      }
      if (!node.prop.startsWith('--')) return say(`${node.prop}: ${value} in ${context}; only tokens go there, and a value comes from a token by @apply on the page's roots`);
      if (!TOKEN_NAMES.has(node.prop)) return say(`${node.prop} in ${context}, which tailwind.config.js never reads`);
      if (!TOKEN_VALUE.test(value)) say(`${node.prop}: ${value}; a token is numbers (an R G B triplet or an alpha)`);
    });
  };

  /** A rule on the page's roots in @layer base, holding only @apply of legal classes. */
  const rootRule = (/** @type {any} */ rule) => {
    const parts = selector(rule).split(',').map((p) => p.trim());
    if (!parts.every((p) => PAGE_ROOTS.has(p))) say(`a rule on ${selector(rule)} in @layer base; only html, body and #root may be styled there`);
    body(rule);
  };

  /** A rule's contents: only @apply of legal classes, never a raw declaration or !important. */
  const body = (/** @type {any} */ rule) => {
    rule.each((/** @type {any} */ node) => {
      if (node.type === 'comment') return;
      if (node.type !== 'atrule' || node.name !== 'apply') return say(`${where(node)} under ${selector(rule)}; a value comes from a token, by @apply`);
      for (const cls of params(node).split(' ')) {
        if (cls.startsWith('!') || cls.endsWith('!') || cls === '!important') say(`@apply ${cls}: no !important`);
        else if (!classify(cls).legal) say(`@apply ${cls}, which is not in the closed vocabulary (app/tailwind.config.js)`);
      }
    });
  };

  let roots = 0;
  postcss.parse(css).each((/** @type {any} */ node) => {
    if (node.type === 'comment') return;
    if (node.type === 'atrule' && node.name === 'tailwind') {
      if (!TAILWIND_PARTS.has(params(node))) say(`@tailwind ${params(node)}`);
      return;
    }
    if (node.type === 'rule' && selector(node) === ':root') {
      roots += 1;
      if (roots > 1) say('a second top-level :root rule');
      return tokenRule(node, true, 'the :root rule');
    }
    if (node.type === 'atrule' && node.name === 'media') {
      if (params(node) !== '(prefers-color-scheme: dark)') return say(`@media ${params(node)}; only (prefers-color-scheme: dark) may flip the tokens`);
      return node.each((/** @type {any} */ inner) => {
        if (inner.type === 'comment') return;
        if (inner.type === 'rule' && selector(inner) === ':root') return tokenRule(inner, false, 'the dark media query');
        say(`${where(inner)} in the dark media query; it holds only :root rules of tokens`);
      });
    }
    if (node.type === 'atrule' && node.name === 'layer') {
      if (params(node) !== 'base') return say(`@layer ${params(node)}; only @layer base, for the page's roots`);
      return node.each((/** @type {any} */ inner) => {
        if (inner.type === 'comment') return;
        if (inner.type === 'rule') return rootRule(inner);
        say(`${where(inner)} in @layer base; it holds only rules on html, body and #root`);
      });
    }
    say(`${where(node)} at the top level`);
    if (node.type === 'rule') body(node);
  });
  return problems;
}

// ---- 4. Fail closed ----

/**
 * Content globs (relative to app/) that match no file, or that miss a window module the shipped page
 * loads (`sources`, paths), which Tailwind would then never scan.
 * @param {string[]} globs
 * @param {string[]} sources
 */
export function contentProblems(globs, sources) {
  const real = realpathSync(APP);
  const files = new Set(globSync(globs, { cwd: real }).map((f) => path.join(real, f)));
  const window = path.join(real, 'src', 'window');
  if (files.size === 0) return [`design/css: the content globs (${globs.join(', ')}) match no file, so nothing would be judged`];
  return sources
    .filter((f) => f.startsWith(window + path.sep) && !SHEET.test(f) && !files.has(f))
    .map((f) => `design/css: ${path.relative(real, f)} is loaded by the shipped page but not in Tailwind's content (window-sources.js)`);
}

// ---- The whole check ----

/**
 * Every problem the four parts find in the shipped CSS, as npm run lint prints them; empty when the
 * guarantee holds. `content` replaces Tailwind's content list (a test's probe).
 * @param {{ content?: unknown[] }} [options]
 */
export async function shippedCssProblems(options = {}) {
  const graph = await walkShipped();
  const sources = shippedSources();
  const scanned = new Set(sources);
  const classes = await emittedClasses(options);
  return [
    ...(await configProblems()),
    ...htmlProblems(readFileSync(INDEX, 'utf8')),
    ...stylesheetsBeyond(graph, realpathSync(STYLES)),
    ...graph.sources.filter((f) => !scanned.has(f)).map((f) => `design/css: the shipped page loads ${path.relative(APP, f)}, which the CSS check does not scan (shippedSources)`),
    ...sourceProblems(sources),
    ...packageProblems(),
    ...stylesheetProblems(readFileSync(STYLES, 'utf8')),
    ...contentProblems(SOURCE_GLOBS, graph.sources),
    ...(classes.length === 0 ? ['design/css: Tailwind emitted no class for the window, so nothing was judged'] : []),
    ...classes.filter((cls) => !classify(cls).legal).map((cls) => `design/css: \`${cls}\` reaches the shipped CSS but is not in the closed vocabulary (app/tailwind.config.js)`),
  ];
}
