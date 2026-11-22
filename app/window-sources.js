// @ts-check
// The window's source files, one list: Tailwind's content (what it scans for classes, so what can
// reach the shipped CSS), and, minus the html pages, what ESLint's vocabulary rule reads. The CSS
// check (eslint-rules/check-css.js) runs Tailwind itself over this list, so whatever the lint cannot
// read, the check still sees.

/** Every file Tailwind scans for classes, relative to app/. */
export const SOURCE_GLOBS = ['src/window/**/*.{html,js,jsx,mjs,cjs,ts,tsx,mts,cts}'];

/** The same files ESLint can parse: the list without its html pages. */
export const SCRIPT_GLOBS = SOURCE_GLOBS.map((g) => g.replace('html,', ''));
