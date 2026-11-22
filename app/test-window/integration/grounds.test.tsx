// Every rendered text sits on a ground the contrast test measures. Each fixture state is drawn in
// browser mode (screen 6 and the banner), and every text node is paired by inheritance, as the
// browser draws it (review round 4, grounds.test.tsx:42): its colour is the nearest text-<token> on
// it or an ancestor, else body's (styles.css: @apply ... text-ink); its ground the nearest
// bg-<token>, else body's (bg-canvas). Each pair drawn must be one the contrast test measures.
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import postcss from 'postcss';
import { expect, test } from 'vitest';
import { BrowserApp } from '../../src/window/browser.tsx';
import { SCENARIO_NAMES, VIEW_NAMES } from '../../src/window/fake/fixtures.ts';
import { PAIRS } from '../support/pairs.ts';

const css = readFileSync(path.resolve(import.meta.dirname, '..', '..', 'src', 'window', 'styles.css'), 'utf8');
const COLOURS = new Set([...css.matchAll(/--([a-z-]+):\s*\d+ \d+ \d+;/g)].map((m) => m[1] as string));
// Every class styles.css applies to body, through any rule whose selector list names it, in order.
const BODY: string[] = [];
postcss.parse(css).walkRules((rule) => {
  if (!rule.selector.split(',').map((x) => x.trim()).includes('body')) return;
  rule.walkAtRules('apply', (at) => {
    BODY.push(...at.params.trim().split(/\s+/));
  });
});
const BODY_TEXT = BODY.map((c) => /^text-([a-z-]+)$/.exec(c)?.[1]).findLast((c) => c !== undefined && COLOURS.has(c));
const BODY_GROUND = BODY.map((c) => /^bg-([a-z-]+)$/.exec(c)?.[1]).findLast((c) => c !== undefined && COLOURS.has(c));

/** The colour token an element's plain `prefix-<token>` class names (variants like hover: ignored). */
function colourOf(el: Element, prefix: 'text' | 'bg'): string | undefined {
  for (const cls of typeof el.className === 'string' ? el.className.split(/\s+/) : []) {
    const m = new RegExp(`^${prefix}-([a-z-]+)$`).exec(cls);
    if (m && COLOURS.has(m[1] as string)) return m[1];
  }
  return undefined;
}

/** The nearest `prefix` colour on `el` or an ancestor, else body's. */
function inherited(el: Element | null, prefix: 'text' | 'bg'): string | undefined {
  for (let cur = el; cur !== null; cur = cur.parentElement) {
    const c = colourOf(cur, prefix);
    if (c !== undefined) return c;
  }
  return prefix === 'text' ? BODY_TEXT : BODY_GROUND;
}

/** Every pair drawn under `root`: each non-blank text node's inherited colour on its inherited ground. */
function pairsIn(root: Element): Set<string> {
  const out = new Set<string>();
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if ((node.textContent ?? '').trim() === '') continue;
    out.add(`${inherited(node.parentElement, 'text')} on ${inherited(node.parentElement, 'bg')}`);
  }
  return out;
}

test('body\'s own colours are read from styles.css: ink on canvas', () => {
  expect([BODY_TEXT, BODY_GROUND]).toEqual(['ink', 'canvas']);
});

test('every text, its colour inherited or its own, is drawn only on grounds the contrast test measures, in every fixture state', async () => {
  const measured = new Set(PAIRS.map(([fg, bg]) => `${fg} on ${bg}`));
  const drawn = new Set<string>();
  for (const name of SCENARIO_NAMES) {
    render(<BrowserApp search={`?state=${name}`} />);
    await waitFor(() => expect(screen.queryByText('Loading…')).not.toBeInTheDocument());
    for (const pair of pairsIn(document.body)) drawn.add(pair);
    cleanup();
  }
  // W6: screen 2 in each waiting view, the question's text shown and an alert's badge among them.
  for (const view of VIEW_NAMES) {
    render(<BrowserApp search={`?state=one-project&view=${view}`} />);
    await screen.findByRole('heading', { level: 2, name: /^Yours/ });
    for (const b of screen.queryAllByRole('button', { name: 'Show the question' })) fireEvent.click(b);
    for (const pair of pairsIn(document.body)) drawn.add(pair);
    cleanup();
  }
  expect(drawn.size).toBeGreaterThan(5);
  expect([...drawn].filter((p) => !measured.has(p)).sort()).toEqual([]);
});

test('the reviewer\'s probe is caught: a span on bg-primary inheriting text-ink draws ink on primary, which is not measured', () => {
  const { container } = render(
    <div className="text-ink">
      <span className="bg-primary">probe</span>
    </div>,
  );
  expect([...pairsIn(container)]).toEqual(['ink on primary']);
  expect(PAIRS.some(([fg, bg]) => fg === 'ink' && bg === 'primary')).toBe(false);
});

test('a text with no text class of its own on no ground of its own inherits body\'s: ink on canvas', () => {
  const { container } = render(<p>plain</p>);
  expect([...pairsIn(container)]).toEqual(['ink on canvas']);
});
