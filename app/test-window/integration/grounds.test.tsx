// Review round 3's nit (contrast.test.ts:74): the contrast test measured text colours, never the
// ground each one sits on, so a known colour on a new ground (ink on the black pill) would pass. Here
// every fixture state is drawn in browser mode (screen 6 and the banner), and every element with a
// text colour is paired with its ground - its own background, else its nearest ancestor's, else the
// canvas; each pair drawn must be one the contrast test measures.
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { expect, test } from 'vitest';
import { BrowserApp } from '../../src/window/browser.tsx';
import { SCENARIO_NAMES } from '../../src/window/fake/fixtures.ts';
import { PAIRS } from '../support/pairs.ts';

const css = readFileSync(path.resolve(import.meta.dirname, '..', '..', 'src', 'window', 'styles.css'), 'utf8');
const COLOURS = new Set([...css.matchAll(/--([a-z-]+):\s*\d+ \d+ \d+;/g)].map((m) => m[1] as string));

/** The colour token an element's plain `prefix-<token>` class names (variants like hover: ignored). */
function colourOf(el: Element, prefix: 'text' | 'bg'): string | undefined {
  for (const cls of el.className && typeof el.className === 'string' ? el.className.split(/\s+/) : []) {
    const m = new RegExp(`^${prefix}-([a-z-]+)$`).exec(cls);
    if (m && COLOURS.has(m[1] as string)) return m[1];
  }
  return undefined;
}

/** The ground an element's text sits on. */
function groundOf(el: Element): string {
  for (let cur: Element | null = el; cur !== null; cur = cur.parentElement) {
    const bg = colourOf(cur, 'bg');
    if (bg !== undefined) return bg;
  }
  return 'canvas';
}

test('every text colour is drawn only on grounds the contrast test measures, in every fixture state', async () => {
  const measured = new Set(PAIRS.map(([fg, bg]) => `${fg} on ${bg}`));
  const drawn = new Set<string>();
  for (const name of SCENARIO_NAMES) {
    render(<BrowserApp search={`?state=${name}`} />);
    await waitFor(() => expect(screen.queryByText('Loading…')).not.toBeInTheDocument());
    for (const el of document.body.querySelectorAll('*')) {
      const fg = colourOf(el, 'text');
      if (fg !== undefined) drawn.add(`${fg} on ${groundOf(el)}`);
    }
    cleanup();
  }
  expect(drawn.size).toBeGreaterThan(5);
  expect([...drawn].filter((p) => !measured.has(p)).sort()).toEqual([]);
});

test('the pairing sees a known colour on an unmeasured ground', () => {
  render(
    <div className="bg-primary">
      <span className="text-ink">x</span>
    </div>,
  );
  const span = screen.getByText('x');
  expect(`${colourOf(span, 'text')} on ${groundOf(span)}`).toBe('ink on primary');
  expect(PAIRS.some(([fg, bg]) => fg === 'ink' && bg === 'primary')).toBe(false);
});
