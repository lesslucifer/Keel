// @ts-check
// Keel's tokens: the window's whole visual vocabulary (DEC-72). Every scale below REPLACES
// Tailwind's default instead of extending it, so a value that is not a token emits no CSS; the
// lint rule (eslint-rules/vocabulary.js) empties every scale left undeclared and refuses any class
// outside this list, an arbitrary value, a class built at run time and the dark: variant.
// A new size or colour is a change here (and, for a colour, in src/window/styles.css), never a class.
import path from 'node:path';

const here = import.meta.dirname;

/** A colour token: its R G B triplet lives in styles.css, once for light and once for dark. */
const token = (/** @type {string} */ name) => `rgb(var(--${name}) / <alpha-value>)`;

/** @type {import('tailwindcss').Config} */
export default {
  content: [path.join(here, 'src', 'window', '**', '*.{html,ts,tsx}')],
  // Unused: dark lives in the tokens (styles.css flips each variable), and the lint refuses dark:.
  darkMode: 'media',
  theme: {
    // No breakpoint: one window, no responsive variants.
    screens: {},
    colors: {
      transparent: 'transparent',
      current: 'currentColor',
      // Grounds and lines.
      canvas: token('canvas'),
      surface: token('surface'),
      line: token('line'),
      // Text.
      ink: token('ink'),
      'ink-muted': token('ink-muted'),
      // The one accent, a muted blue (the owner's answer, 2026-11-22), and the text drawn on it.
      accent: token('accent'),
      'on-accent': token('on-accent'),
      // Meanings, never decoration: done or passing, waits on someone, failed or stuck.
      ok: token('ok'),
      wait: token('wait'),
      fail: token('fail'),
    },
    spacing: {
      0: '0px',
      px: '1px',
      1: '4px',
      2: '8px',
      3: '12px',
      4: '16px',
      6: '24px',
      8: '32px',
      // A control's height: a push button's on the Mac.
      control: '24px',
    },
    // The widest a screen's column gets, so a line stays readable in a wide window.
    maxWidth: {
      none: 'none',
      full: '100%',
      column: '640px',
    },
    fontFamily: {
      // The Mac's own font: no font file ships (DEC-72).
      sans: ['system-ui', '-apple-system', 'BlinkMacSystemFont', 'sans-serif'],
      // Paths.
      mono: ['ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
    },
    fontSize: {
      caption: ['11px', { lineHeight: '14px' }],
      // The Mac's body size.
      body: ['13px', { lineHeight: '18px' }],
      title: ['15px', { lineHeight: '20px' }],
      heading: ['20px', { lineHeight: '26px' }],
    },
    fontWeight: {
      regular: '400',
      medium: '500',
      semibold: '600',
    },
    lineHeight: {
      none: '1',
      body: '18px',
    },
    borderRadius: {
      none: '0px',
      control: '6px',
      panel: '8px',
      full: '9999px',
    },
    borderWidth: {
      DEFAULT: '1px',
      0: '0px',
    },
    opacity: {
      // A control that is off.
      disabled: '0.45',
    },
  },
  plugins: [],
};
