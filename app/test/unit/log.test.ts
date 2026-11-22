// The update log's rules (local-app.html 'Applying a line'), each on fabricated input: a line's
// shape, the apply step, the state's shape, whole lines only, and the heard rule.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyLine, emptyFold, foldLines, heardAt, parseLine, parseState, wholeLines, SILENCE_MS, ALERTS_KEPT, type Fold, type Line } from '../../src/main/reader/log.ts';
import { seeded, pick, type Rng } from '../support/seeded.ts';

const RUN = 'keel-1c.20261122-083000.4242';
const OTHER = 'keel-1c.20261122-090000.4343';
const AT = '2026-11-22T08:30:00+08:00';

const line = (o: Record<string, unknown>) => JSON.stringify({ run: RUN, at: AT, ...o });

test('a contract line parses: start, set, del and alert, keys in any order', () => {
  assert.deepEqual(parseLine(line({ n: 1, op: 'start' })), { run: RUN, n: 1, at: AT, op: 'start' });
  assert.deepEqual(parseLine(line({ n: 2, op: 'set', key: 'wu/E-APPLIST.W5/stage', value: { stage: 'building' } })), {
    run: RUN, n: 2, at: AT, op: 'set', key: 'wu/E-APPLIST.W5/stage', value: { stage: 'building' },
  });
  assert.deepEqual(parseLine(line({ n: 3, op: 'del', key: 'question/q-1' })), { run: RUN, n: 3, at: AT, op: 'del', key: 'question/q-1' });
  assert.deepEqual(parseLine(line({ n: 4, op: 'alert', value: { headline: 'h', lines: ['a', 'b'], wu: 'E-X.W1', link: 'keel://wu/E-X.W1' } })), {
    run: RUN, n: 4, at: AT, op: 'alert', value: { headline: 'h', lines: ['a', 'b'], wu: 'E-X.W1', link: 'keel://wu/E-X.W1' },
  });
  assert.deepEqual(parseLine(JSON.stringify({ op: 'del', key: 'router', n: 5, at: AT, run: RUN })), { run: RUN, n: 5, at: AT, op: 'del', key: 'router' });
});

test('every key the router writes is known: router, facts, a stage, a conflict, a paused epic, a question, a worker, its report', () => {
  for (const key of ['router', 'facts', 'wu/E-A1.W12/stage', 'wu/E-A.W1/conflict', 'epic/E-APPLIST/paused', 'question/merge-order', 'worker/e-applist-w5', 'worker/w1/report']) {
    assert.equal(parseLine(line({ n: 2, op: 'set', key, value: 1 }))?.op, 'set', key);
  }
});

test('a well-formed line with an unknown key is a skip that still holds its number', () => {
  for (const key of ['newthing/x', 'wu/E-A.W1/other', 'worker/Bad_Name', 'question/', 'Router']) {
    assert.deepEqual(parseLine(line({ n: 7, op: 'set', key, value: 1 })), { run: RUN, n: 7, at: AT, op: 'skip' }, key);
  }
  assert.deepEqual(parseLine(line({ n: 7, op: 'rename', key: 'router' })), { run: RUN, n: 7, at: AT, op: 'skip' });
});

test('the caps count code points, as the writers do (jq length, Python len): 80 emoji is a headline, 200 a line; one more is not', () => {
  const at = (headline: string, lines: string[]) => parseLine(line({ n: 2, op: 'alert', value: { headline, lines } }));
  assert.equal(at('😀'.repeat(80), ['😀'.repeat(200)])?.op, 'alert');
  assert.equal(at('漢'.repeat(80), ['é'.repeat(200)])?.op, 'alert');
  assert.equal(at('😀'.repeat(81), ['l']), null);
  assert.equal(at('h', ['😀'.repeat(201)]), null);
  const state = parseState({ run: RUN, n: 2, at: AT, values: {}, alerts: [{ n: 2, at: AT, headline: '😀'.repeat(80), lines: ['😀'.repeat(200)] }] });
  assert.equal(state?.alerts.length, 1);
});

test('anything else is no line: not JSON, half a line, not an object, a run, n or at of the wrong type, a null value, a bad alert', () => {
  const bad = [
    '', 'x', '{"run":', line({ n: 1, op: 'start' }).slice(0, 20), '[]', '7', 'null',
    JSON.stringify({ run: 7, n: 1, at: AT, op: 'start' }),
    JSON.stringify({ run: 'Not A Run', n: 1, at: AT, op: 'start' }),
    line({ n: 0, op: 'start' }), line({ n: 1.5, op: 'start' }), line({ n: '2', op: 'start' }), line({ n: -1, op: 'start' }),
    JSON.stringify({ run: RUN, n: 1, at: 7, op: 'start' }),
    line({ n: 2, op: 'set', key: 'router' }), line({ n: 2, op: 'set', key: 'router', value: null }),
    line({ n: 2, op: 'alert', value: { headline: '', lines: ['a'] } }),
    line({ n: 2, op: 'alert', value: { headline: 'h', lines: [] } }),
    line({ n: 2, op: 'alert', value: { headline: 'h', lines: ['a', 'b', 'c'] } }),
    line({ n: 2, op: 'alert', value: { headline: 'h', lines: ['a'], wu: 'nope' } }),
    line({ n: 2, op: 'alert', value: { headline: 'h', lines: ['a'], extra: 1 } }),
    line({ n: 2, op: 'alert', value: { headline: 'h'.repeat(81), lines: ['a'] } }),
  ];
  for (const b of bad) assert.equal(parseLine(b), null, b);
});

const fold = (o: Partial<Fold> = {}): Fold => ({ ...emptyFold(), run: RUN, n: 3, at: AT, ...o });
const set = (n: number, key: string, value: unknown, run = RUN): Line => ({ run, n, at: AT, op: 'set', key, value });

test('the next number is applied; a number seen is skipped; a missing one asks for the state', () => {
  const f = fold({ values: { router: 1 } });
  assert.deepEqual(applyLine(f, set(4, 'facts', 2)), { fold: { ...f, n: 4, values: { router: 1, facts: 2 } }, needState: false });
  assert.deepEqual(applyLine(f, set(3, 'facts', 2)), { fold: f, needState: false });
  assert.deepEqual(applyLine(f, set(1, 'facts', 2)), { fold: f, needState: false });
  assert.deepEqual(applyLine(f, set(5, 'facts', 2)), { fold: f, needState: true });
});

test('a new run drops every router value and asks for the state', () => {
  const f = fold({ values: { router: 1, 'question/q': { headline: 'h' } }, alerts: [{ n: 2, at: AT, headline: 'h', lines: ['l'] }] });
  const r = applyLine(f, { run: OTHER, n: 1, at: AT, op: 'start' });
  assert.equal(r.needState, true);
  assert.deepEqual(r.fold, emptyFold());
});

test('del removes a key; an alert is kept with its number and time, the newest 20 only; a skip only moves the number', () => {
  let f = fold({ values: { router: 1, facts: 2 } });
  f = applyLine(f, { run: RUN, n: 4, at: AT, op: 'del', key: 'facts' }).fold;
  assert.deepEqual(f.values, { router: 1 });
  for (let n = 5; n < 5 + ALERTS_KEPT + 3; n++) f = applyLine(f, { run: RUN, n, at: AT, op: 'alert', value: { headline: `h${n}`, lines: ['l'] } }).fold;
  assert.equal(f.alerts.length, ALERTS_KEPT);
  assert.equal(f.alerts[0]?.n, 8);
  assert.deepEqual(f.alerts.at(-1), { n: 27, at: AT, headline: 'h27', lines: ['l'] });
  const g = applyLine(f, { run: RUN, n: 28, at: '2026-11-22T09:00:00+08:00', op: 'skip' }).fold;
  assert.deepEqual(g, { ...f, n: 28, at: '2026-11-22T09:00:00+08:00' });
});

test('the state parses as the writer folds it; unknown keys and bad alerts are dropped; any other shape is no state', () => {
  const good = { run: RUN, n: 9, at: AT, values: { router: { name: 'keel-1c' }, 'odd/key': 1 }, alerts: [{ n: 4, at: AT, headline: 'h', lines: ['l'], wu: 'E-X.W1' }, { n: 5, at: AT, headline: '', lines: ['l'] }] };
  assert.deepEqual(parseState(good), { run: RUN, n: 9, at: AT, values: { router: { name: 'keel-1c' } }, alerts: [{ n: 4, at: AT, headline: 'h', lines: ['l'], wu: 'E-X.W1' }] });
  for (const bad of [null, [], 'x', { ...good, run: 1 }, { ...good, n: 0 }, { ...good, n: 2.5 }, { ...good, values: [] }, { ...good, alerts: {} }, { ...good, at: 3 }]) {
    assert.equal(parseState(bad), null, JSON.stringify(bad));
  }
});

test('only whole lines are read: a trailing half line is left for the next read, a character cut across reads too', () => {
  const enc = (s: string) => Buffer.from(s, 'utf8');
  assert.deepEqual(wholeLines(enc('a\nb\nc')), { lines: ['a', 'b'], used: 4 });
  assert.deepEqual(wholeLines(enc('a\n')), { lines: ['a'], used: 2 });
  assert.deepEqual(wholeLines(enc('half')), { lines: [], used: 0 });
  assert.deepEqual(wholeLines(enc('')), { lines: [], used: 0 });
  const cut = enc('é\n漢字');
  assert.deepEqual(wholeLines(cut.subarray(0, cut.length - 2)), { lines: ['é'], used: 3 });
  assert.deepEqual(wholeLines(enc('\n\nx\n')), { lines: ['', '', 'x'], used: 4 });
});

test('heard: a line under 15 minutes old on the app clock; an unparseable time counts as the file\'s; no line is not heard', () => {
  const t = Date.parse(AT);
  assert.equal(SILENCE_MS, 15 * 60 * 1000);
  assert.equal(heardAt(AT, t + SILENCE_MS - 1, null), true);
  assert.equal(heardAt(AT, t + SILENCE_MS, null), false);
  assert.equal(heardAt('not a time', t + 1000, t), true);
  assert.equal(heardAt('not a time', t + SILENCE_MS, t), false);
  assert.equal(heardAt('not a time', t, null), false);
  assert.equal(heardAt(null, t, t), false);
  assert.equal(heardAt(AT, t + 60_000, null, 60_000), false);
});

// ---- The property: whatever reaches the reader, it ends where the writer's state is. ----

const KEYS = ['router', 'facts', 'wu/E-A.W1/stage', 'wu/E-A.W2/conflict', 'epic/E-A/paused', 'question/q-1', 'worker/w1', 'worker/w1/report'];

/** The writer's own fold, as router-app.sh does it: every value as it stands, the last 20 alerts. */
function writerRun(rng: Rng, count: number): { lines: Line[]; states: Fold[] } {
  const lines: Line[] = [{ run: RUN, n: 1, at: AT, op: 'start' }];
  let st: Fold = { run: RUN, n: 1, at: AT, values: {}, alerts: [] };
  const states = [structuredClone(st)];
  for (let n = 2; n <= count; n++) {
    const r = rng();
    let l: Line;
    if (r < 0.5) l = { run: RUN, n, at: AT, op: 'set', key: pick(rng, KEYS), value: { v: Math.floor(rng() * 100) } };
    else if (r < 0.75) l = { run: RUN, n, at: AT, op: 'del', key: pick(rng, KEYS) };
    else l = { run: RUN, n, at: AT, op: 'alert', value: { headline: `h${n}`, lines: ['l'] } };
    const values = { ...st.values };
    let alerts = st.alerts;
    if (l.op === 'set') values[l.key] = l.value;
    if (l.op === 'del') delete values[l.key];
    if (l.op === 'alert') alerts = [...alerts, { n, at: AT, ...l.value }].slice(-ALERTS_KEPT);
    st = { run: RUN, n, at: AT, values, alerts };
    lines.push(l);
    states.push(structuredClone(st));
  }
  return { lines, states };
}

test('property: lines dropped, repeated or cut, the reader folds to the writer\'s state, reading the state only on a gap', () => {
  const rng = seeded(0x5eed05);
  for (let round = 0; round < 300; round++) {
    const { lines, states } = writerRun(rng, 2 + Math.floor(rng() * 60));
    const final = states.at(-1) as Fold;
    const delivered: Array<Line | null> = [];
    let gaps = false;
    for (const l of lines) {
      const r = rng();
      if (r < 0.08) { gaps = true; continue; }
      if (r < 0.14) { delivered.push(null); gaps = true; continue; }
      delivered.push(l);
      if (r < 0.22) delivered.push(l);
    }
    let reads = 0;
    const { fold } = foldLines(emptyFold(), delivered.filter((l): l is Line => l !== null), () => (reads++, structuredClone(final)));
    if (!gaps) {
      assert.deepEqual(fold, final, `round ${round}`);
      assert.equal(reads <= 1, true, `round ${round}: ${reads} reads with no gap`);
    } else {
      // A drop at the very end leaves nothing to show the gap until the next line: the state is never newer than the fold says.
      const caught = reads > 0 ? final : fold.run === null ? emptyFold() : states.find((s) => s.n === fold.n);
      assert.deepEqual(fold, caught, `round ${round}`);
    }
  }
});

test('foldLines reads the state once at most per call, even when a gap follows the state', () => {
  let reads = 0;
  const stale: Fold = { run: RUN, n: 2, at: AT, values: {}, alerts: [] };
  const r = foldLines(emptyFold(), [set(5, 'router', 1), set(9, 'router', 2)], () => (reads++, structuredClone(stale)));
  assert.equal(reads, 1);
  assert.equal(r.needState, true);
  assert.deepEqual(r.fold, stale);
});
