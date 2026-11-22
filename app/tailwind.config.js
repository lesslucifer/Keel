// @ts-check
// Keel's tokens: the window's whole visual vocabulary (DEC-72), valued from the owner's design file
// (docs/work/2026-11-22-e-applist-w8-design.md, Mercury's design system) and fitted to a desktop
// settings window. Its day world is the light appearance and its night world the dark one; the Mac
// picks (styles.css flips each colour). Every scale below REPLACES Tailwind's default instead of
// extending it, so a value that is not a token emits no CSS; the lint rule (eslint-rules/vocabulary.js)
// empties every scale left undeclared and refuses any class outside this list, an arbitrary value, a
// class built at run time and the dark: variant. A new size or colour is a change here (and, for a
// colour, in src/window/styles.css), never a class.
import path from 'node:path';
import { SOURCE_GLOBS } from './window-sources.js';

const here = import.meta.dirname;

/** A colour token: its R G B triplet lives in styles.css, once for light (day) and once for dark (night). */
const token = (/** @type {string} */ name) => `rgb(var(--${name}) / <alpha-value>)`;

/** @type {import('tailwindcss').Config} */
export default {
  // The one source list (window-sources.js): what Tailwind scans is what the CSS check reads.
  content: SOURCE_GLOBS.map((g) => path.join(here, g)),
  // Unused: dark lives in the tokens (styles.css flips each variable), and the lint refuses dark:.
  darkMode: 'media',
  theme: {
    // No breakpoint: one window, no responsive variants (the design file's breakpoints are a website's).
    screens: {},
    colors: {
      transparent: 'transparent',
      current: 'currentColor',
      // The window's ground: canvas by day, night by night.
      canvas: token('canvas'),
      // A card, one step of fill off the canvas (no border): lavender by day, night-raised by night.
      card: token('card'),
      // A 1px rule between rows inside a card: fog by day, slate by night.
      line: token('line'),
      // Text: ink and ink-secondary by day, on-dark and fog by night.
      ink: token('ink'),
      'ink-secondary': token('ink-secondary'),
      // The one saturated action colour, Mercury Indigo, and the label on it.
      primary: token('primary'),
      'on-primary': token('on-primary'),
      // The translucent pill's colour (indigo by day, periwinkle by night), always used at the
      // opacity step tint or tint-hover, whose strength flips with the world too.
      tint: token('tint'),
    },
    // The design file's 8px scale, a 4px half step for tight inline gaps, and two heights.
    spacing: {
      0: '0px',
      px: '1px',
      half: '4px',
      xs: '8px',
      sm: '12px',
      md: '16px',
      lg: '20px',
      xl: '24px',
      xxl: '32px',
      huge: '40px',
      // A state label's height.
      badge: '22px',
      // A button's height: the design file's small pill, sized for a window.
      control: '32px',
    },
    // The scales Tailwind derives from spacing, each declared (the lint empties an undeclared one):
    // padding, gap and height draw on the spacing tokens; the others hold only what the window uses.
    padding: ({ theme }) => theme('spacing'),
    gap: ({ theme }) => theme('spacing'),
    height: ({ theme }) => theme('spacing'),
    margin: {
      0: '0px',
      auto: 'auto',
    },
    width: {
      full: '100%',
    },
    minWidth: {
      0: '0px',
    },
    minHeight: {
      full: '100%',
    },
    // The widest a screen's column gets, so a line stays readable in a wide window.
    maxWidth: {
      none: 'none',
      full: '100%',
      column: '720px',
    },
    fontFamily: {
      // The Mac's own font (DEC-72): no font file ships. SF Pro is variable, so the design file's light
      // weights (360-480) render; its Inter substitute is not shipped.
      sans: ['system-ui', '-apple-system', 'BlinkMacSystemFont', 'sans-serif'],
      // Paths.
      mono: ['ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
    },
    // The design file's type scale, sized for a window: no display or stat sizes.
    fontSize: {
      // Small uppercase section labels (the only uppercase).
      eyebrow: ['11px', { lineHeight: '14px', letterSpacing: '0.06em' }],
      legal: ['12px', { lineHeight: '17px', letterSpacing: '0.02em' }],
      'ui-sm': ['13px', { lineHeight: '16px', letterSpacing: '0.005em' }],
      ui: ['14px', { lineHeight: '16px' }],
      body: ['14px', { lineHeight: '20px' }],
      'body-lg': ['15px', { lineHeight: '21px' }],
      title: ['17px', { lineHeight: '22px', letterSpacing: '0.01em' }],
      heading: ['22px', { lineHeight: '27px', letterSpacing: '0.42px' }],
    },
    // Light weights: body 360-400, UI 420, headings 480, eyebrows 500; nothing above 500.
    fontWeight: {
      light: '360',
      regular: '400',
      ui: '420',
      display: '480',
      medium: '500',
    },
    lineHeight: {
      none: '1',
      body: '20px',
    },
    letterSpacing: {
      normal: '0',
      eyebrow: '0.06em',
    },
    // The design file's rounded scale: capsules for actions, soft corners for cards.
    borderRadius: {
      none: '0px',
      xs: '4px',
      sm: '8px',
      md: '12px',
      lg: '16px',
      xl: '32px',
      pill: '40px',
      full: '9999px',
    },
    borderWidth: {
      DEFAULT: '1px',
      0: '0px',
    },
    // Depth is tonal: no shadow.
    boxShadow: {
      none: 'none',
    },
    opacity: {
      // A control that is off.
      disabled: '0.45',
      // The translucent pill and its hover: 10% and 16% by day, 20% and 30% by night (styles.css).
      tint: 'var(--tint-alpha)',
      'tint-hover': 'var(--tint-hover-alpha)',
    },
    // Scales with no magnitude, declared anyway: an undeclared scale is emptied by the lint rule, so
    // each holds exactly the members the window draws from.
    flex: {
      1: '1 1 0%',
    },
    flexShrink: {
      0: '0',
    },
    cursor: {
      pointer: 'pointer',
    },
  },
  plugins: [],
};
