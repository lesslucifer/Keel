// @ts-check
// The guarantee that only Keel's vocabulary reaches the shipped CSS. Tailwind writes CSS only for
// whole class names it finds written out in its content files, however a string is built later, so
// this reads what Tailwind emits - src/window/styles.css through Tailwind's PostCSS plugin with the
// shipped config (as vite.renderer.config.ts runs it), over the one content list - and lists every
// emitted class the closed vocabulary does not hold. In memory: no build, no port.

import { readFileSync } from 'node:fs';
import path from 'node:path';
import postcss from 'postcss';
import tailwindcss from 'tailwindcss';
import tailwindConfig from '../tailwind.config.js';
import { classify } from './classes.js';

const STYLES = path.join(import.meta.dirname, '..', 'src', 'window', 'styles.css');

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
