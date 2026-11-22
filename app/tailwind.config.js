// @ts-check
// Keel's tokens: the window's whole visual vocabulary (DEC-72), valued from the look the owner chose on
// 2026-11-22, mx3a Spectrum Plain: Mistral's type (docs/work/2026-11-22-e-applist-w8-design-mistral.md)
// on MiniMax's colour (docs/work/2026-11-22-e-applist-w8-design-minimax.md) - a white canvas, deep
// black ink, a black pill for the main action and outline pills for the rest, white 12px cards on
// hairlines, a plain white title bar over a hairline, no gradient anywhere. System fonts only: New
// York stands in for PP Editorial Old, the system sans for Inter and DM Sans. Its day world is the
// light appearance and its night world the dark one; the Mac picks (styles.css flips each colour). Every scale below REPLACES Tailwind's default instead of
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
    // No breakpoint: one window, no responsive variants (the design files' breakpoints are a website's).
    screens: {},
    colors: {
      transparent: 'transparent',
      current: 'currentColor',
      // The window's ground and the title bar: white by day, near-black by night.
      canvas: token('canvas'),
      // A card: white on white by day (it stands off by its edge), one step up by night.
      card: token('card'),
      // A 1px rule between rows inside a card.
      line: token('line'),
      // The hairline round a card and under the title bar.
      edge: token('edge'),
      // Text: deep black and its quieter grey by day, off-white and grey by night.
      ink: token('ink'),
      'ink-secondary': token('ink-secondary'),
      // The main action: a black pill by day, a white one by night, and the words on it.
      primary: token('primary'),
      'on-primary': token('on-primary'),
      // Every other action's 1px edge: an outline pill with no fill.
      outline: token('outline'),
      // The three state badges, each a fill and the words on it (words always, never colour alone):
      // set in Keel (blue), found on the PATH (green), missing (red).
      set: token('set'),
      'on-set': token('on-set'),
      found: token('found'),
      'on-found': token('on-found'),
      missing: token('missing'),
      'on-missing': token('on-missing'),
      // A problem, a fallback or an error, always in words.
      problem: token('problem'),
    },
    // An 8px scale, a 4px half step for tight inline gaps, mx3a's 14px row, and two heights.
    spacing: {
      0: '0px',
      px: '1px',
      half: '4px',
      xs: '8px',
      sm: '12px',
      // A row's vertical padding inside a card, and the title bar's.
      row: '14px',
      md: '16px',
      lg: '20px',
      xl: '24px',
      xxl: '32px',
      huge: '40px',
      // A state label's height.
      badge: '22px',
      // A button's height: a small pill, sized for a window.
      control: '32px',
    },
    // The scales Tailwind derives from spacing, each declared (the lint empties an undeclared one):
    // padding and gap draw on the spacing tokens, height on them and full; the others hold only what the window uses.
    padding: ({ theme }) => theme('spacing'),
    gap: ({ theme }) => theme('spacing'),
    // Height also holds full, for the page's root (html, body: styles.css).
    height: ({ theme }) => ({ ...theme('spacing'), full: '100%' }),
    margin: {
      0: '0px',
      auto: 'auto',
    },
    width: {
      full: '100%',
      // The brand mark's side.
      sm: '12px',
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
      // The Mac's own font (DEC-72): no font file ships. The system sans stands in for Inter and DM Sans.
      sans: ['system-ui', '-apple-system', 'BlinkMacSystemFont', 'sans-serif'],
      // The screen title and section heads: New York (ui-serif), standing in for PP Editorial Old.
      serif: ['ui-serif', 'New York', 'Georgia', 'serif'],
      // Paths.
      mono: ['ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
    },
    // Mistral's type scale, sized for a window: no display or stat sizes.
    fontSize: {
      legal: ['12px', { lineHeight: '17px', letterSpacing: '0.02em' }],
      'ui-sm': ['13px', { lineHeight: '16px', letterSpacing: '0.005em' }],
      ui: ['14px', { lineHeight: '16px' }],
      body: ['14px', { lineHeight: '20px' }],
      'body-lg': ['15px', { lineHeight: '21px' }],
      // The wordmark in the title bar.
      title: ['17px', { lineHeight: '22px', letterSpacing: '0.01em' }],
      // A section head, in sentence case, in New York.
      section: ['20px', { lineHeight: '26px', letterSpacing: '-0.2px' }],
      // The screen title, in New York.
      heading: ['34px', { lineHeight: '38px', letterSpacing: '-0.5px' }],
    },
    // Three weights: 400 for text and the serif heads, 500 for controls and names, 600 for a problem.
    fontWeight: {
      regular: '400',
      ui: '500',
      medium: '600',
    },
    lineHeight: {
      none: '1',
      body: '20px',
    },
    letterSpacing: {
      normal: '0',
    },
    // Pills for every button and badge, 12px corners for a card, 3px for the brand mark.
    borderRadius: {
      none: '0px',
      mark: '3px',
      card: '12px',
      pill: '9999px',
    },
    borderWidth: {
      DEFAULT: '1px',
      0: '0px',
    },
    // No gradient anywhere: both scales are declared empty, so no bg-gradient-* and no from-, via- or
    // to- stop emits CSS.
    backgroundImage: {},
    gradientColorStops: {},
    // Depth is a hairline: no shadow.
    boxShadow: {
      none: 'none',
    },
    opacity: {
      // A control that is off.
      disabled: '0.45',
      // An outline pill's hover, a wash of ink: 6% by day, 10% by night (styles.css).
      hover: 'var(--hover-alpha)',
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
