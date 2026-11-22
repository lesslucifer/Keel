// @ts-check
// The guarantee that only Keel's vocabulary reaches the shipped CSS, in four parts, all in memory (no
// build, no port):
// 1. One stylesheet. Vite's own module graph, walked from the shipped entry in middleware mode,
//    finds every stylesheet the page pulls in (a side-effect import, ?inline, ?url, one inside a
//    package), and no file it loads may name another for new URL; index.html may carry no <link>,
//    <style> or style= and loads main.tsx alone. Anything but src/window/styles.css is refused.
// 2. styles.css judged at source against an exact grammar: Tailwind's three directives; one :root
//    rule of tokens (variables the config reads, as numbers) and color-scheme; the dark media query
//    holding only :root rules of tokens; @layer base holding only @apply of legal classes on html,
//    body and #root. Nothing else (no raw property, no other selector, @import, font, media or
//    layer). It must be judged before
//    Tailwind runs: compiled, a raw div{margin:7px} and an @apply of a token look alike.
// 3. The emitted classes. Tailwind writes CSS only for whole class names it finds written out in its
//    content files, however a string is built later, so styles.css through Tailwind's PostCSS plugin
//    with the shipped config (as vite.renderer.config.ts runs it), over the one content list, must
//    emit only classes the closed vocabulary holds. What Tailwind writes for a legal class (and its
//    own preflight) is valued from the closed tokens, so the selector is what is judged.
// 4. Fail closed: content globs matching no file, or missing a module the page loads, and Tailwind
//    emitting no class at all, are refused rather than passed unjudged.
// Route chosen in review round 3 over a renderer build (the owner keeps builds out of the inner loop)
// and over a static import guard (Vite's resolver is the one that decides what ships).

import { globSync, readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import postcss from 'postcss';
import tailwindcss from 'tailwindcss';
import { createServer } from 'vite';
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

/**
 * The modules Vite loads for the shipped page, walked from `entry` through its own resolver in
 * middleware mode (no port, no build, no dependency scan): the stylesheets (ids, queries kept) and the
 * other files (paths). A stylesheet's own dependencies are not followed: Tailwind registers its content
 * files there, which the page never loads.
 * @param {{ root?: string, configFile?: string | false, entry?: string }} [options]
 */
async function walkShipped(options = {}) {
  const server = await createServer({
    configFile: options.configFile ?? path.join(APP, 'vite.renderer.config.ts'),
    ...(options.root === undefined ? {} : { root: options.root }),
    logLevel: 'silent',
    server: { middlewareMode: true, hmr: false, ws: false, preTransformRequests: false },
    optimizeDeps: { noDiscovery: true, entries: [] },
  });
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
      if (mod?.file) sources.push(mod.file);
      for (const next of mod?.importedModules ?? []) queue.push(next.url);
    }
    return { stylesheets, sources };
  } finally {
    await server.close();
  }
}

/**
 * Every stylesheet the shipped page would pull in beyond `allowed`, and every one a loaded file names
 * (for new URL, or any other way) but `namer`'s one import of `allowed`.
 * @param {{ stylesheets: string[], sources: string[] }} graph
 * @param {{ allowed?: string, namer?: string }} [options]
 */
function stylesheetsBeyond(graph, options = {}) {
  // Vite reports real paths (macOS: /tmp is /private/tmp), so both sides are compared real.
  const allowed = realpathSync(options.allowed ?? STYLES);
  const namer = realpathSync(options.namer ?? ENTRY);
  /** @type {string[]} */
  const problems = [];
  for (const id of graph.stylesheets) {
    if (id !== allowed) problems.push(`design/css: the shipped page loads ${path.relative(APP, id)}; only ${path.relative(APP, allowed)} may carry its CSS`);
  }
  for (const file of graph.sources) {
    if (file.includes(`${path.sep}node_modules${path.sep}`)) continue;
    const names = [...readFileSync(file, 'utf8').matchAll(NAMED_SHEET)].map((m) => /** @type {string} */ (m[1]));
    const own = file === namer ? names.filter((n) => path.resolve(path.dirname(file), n) === allowed) : [];
    const extra = file === namer && own.length === 1 ? names.filter((n) => n !== own[0]) : names;
    for (const name of extra) problems.push(`design/css: ${path.relative(APP, file)} names the stylesheet ${name}; only ${path.relative(APP, namer)} may, and only ${path.relative(APP, allowed)}`);
  }
  return problems;
}

/**
 * The graph part of the check: a stylesheet loaded or named beyond styles.css. Options aim it at a
 * probe project (a test).
 * @param {{ root?: string, configFile?: string | false, entry?: string, allowed?: string, namer?: string }} [options]
 */
export async function graphProblems(options = {}) {
  return stylesheetsBeyond(await walkShipped(options), options);
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
  const classes = await emittedClasses(options);
  return [
    ...htmlProblems(readFileSync(INDEX, 'utf8')),
    ...stylesheetsBeyond(graph),
    ...stylesheetProblems(readFileSync(STYLES, 'utf8')),
    ...contentProblems(SOURCE_GLOBS, graph.sources),
    ...(classes.length === 0 ? ['design/css: Tailwind emitted no class for the window, so nothing was judged'] : []),
    ...classes.filter((cls) => !classify(cls).legal).map((cls) => `design/css: \`${cls}\` reaches the shipped CSS but is not in the closed vocabulary (app/tailwind.config.js)`),
  ];
}
