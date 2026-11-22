// The alerter's rules on fabricated input: what is shown for which alert lines, the record of what
// was shown, the keel:// link, the scheme's owner in words, the focus, and the seams W9's test uses.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { decide, alertOf, summaryOf, parseRecord, emptyRecord, Focus, RECORD_CAP, RUNS_KEPT, heldSink, HELD_CAP, type Clickable, type AlertRecord } from '../../src/main/alerter/alerts.ts';
import { parseLink, openLink, schemeText, LINK_CAP } from '../../src/main/alerter/link.ts';
import { seamsOf } from '../../src/main/alerter/seams.ts';
import { WU_ID, WU_SOURCE, type StoredAlert } from '../../src/main/reader/log.ts';
import { seeded, hostileString, pick } from '../support/seeded.ts';

const RUN = 'keel-1c.20261122-083000.4242';
const AT = '2026-11-22T08:30:00+08:00';
const alert = (n: number, o: Partial<StoredAlert> = {}): StoredAlert => ({ n, at: AT, headline: `h${n}`, lines: [`l${n}`], ...o });

const OLD = 'keel-1c.20261122-070000.1';
const rec = (...runs: Array<[string, number[]]>): AlertRecord => ({ version: 2, runs: runs.map(([run, shown]) => ({ run, shown })) });

test('a new alert line is shown once, as its headline over its lines, with its work unit; then it is in the record', () => {
  const r = decide(rec([RUN, []]), RUN, [alert(4, { wu: 'E-A.W1', lines: ['one', 'two'] })], null);
  assert.deepEqual(r.show, [{ title: 'h4', body: 'one\ntwo', wu: 'E-A.W1' }]);
  assert.deepEqual(r.record, rec([RUN, [4]]));
  assert.deepEqual(decide(r.record, RUN, [alert(4, { wu: 'E-A.W1' })], null).show, []);
});

test('each new line is its own alert, oldest first; lines already shown are not shown again', () => {
  const r = decide(rec([RUN, [1, 2]]), RUN, [alert(1), alert(2), alert(3), alert(5)], null);
  assert.deepEqual(r.show.map((s) => s.title), ['h3', 'h5']);
  assert.deepEqual(r.record, rec([RUN, [1, 2, 3, 5]]));
});

test('at the app\'s start, lines written while it was closed make one summary alert, each recorded as shown (owner answer 1a)', () => {
  const r = decide(rec([RUN, [1]]), RUN, [alert(1), alert(2), alert(3), alert(4)], 'start');
  assert.deepEqual(r.show, [summaryOf(3, 'start')]);
  assert.deepEqual(summaryOf(3, 'start'), { title: '3 alerts while Keel was closed', body: 'The list shows each one.', wu: null });
  assert.deepEqual(r.record, rec([RUN, [1, 2, 3, 4]]));
});

test('at the start, a single line written while closed is shown as itself: a summary of one says less', () => {
  assert.deepEqual(decide(emptyRecord(), RUN, [alert(7, { wu: 'E-A.W2' })], 'start').show, [{ title: 'h7', body: 'l7', wu: 'E-A.W2' }]);
});

test('a run first looked at while the app runs (a project switched in) gets one summary, titled neutrally: Keel was not closed', () => {
  const r = decide(rec([OLD, [1, 2, 3]]), RUN, [alert(1), alert(2), alert(3), alert(4), alert(5)], 'look');
  assert.deepEqual(r.show, [summaryOf(5, 'look')]);
  assert.deepEqual(summaryOf(5, 'look'), { title: '5 new alerts', body: 'The list shows each one.', wu: null });
  assert.deepEqual(r.record, rec([OLD, [1, 2, 3]], [RUN, [1, 2, 3, 4, 5]]));
});

test('switching back to a run already in the record shows nothing again; its new lines are each shown', () => {
  const r = decide(rec([RUN, [1, 2]], [OLD, [1]]), RUN, [alert(1), alert(2)], null);
  assert.deepEqual(r.show, []);
  assert.deepEqual(decide(r.record, RUN, [alert(1), alert(2), alert(3)], null).show.map((s) => s.title), ['h3']);
});

test('a run first seen with nothing stored yet (a router restart seen at once) enters the record; its lines then come one by one', () => {
  const r = decide(rec([OLD, [1, 2]]), RUN, [], 'look');
  assert.deepEqual([r.show, r.record], [[], rec([OLD, [1, 2]], [RUN, []])]);
  assert.deepEqual(decide(r.record, RUN, [alert(2)], null).show.map((s) => s.title), ['h2']);
});

test('no run: nothing is shown and the record is kept as it is', () => {
  const kept = rec([RUN, [1]]);
  assert.deepEqual(decide(kept, null, [], 'start'), { record: kept, show: [] });
});

test('the record keeps only the newest numbers per run, and only the newest runs', () => {
  const many = Array.from({ length: RECORD_CAP + 10 }, (_, i) => alert(i + 1));
  const r = decide(rec([RUN, []]), RUN, many, null);
  assert.equal(r.record.runs[0]?.shown.length, RECORD_CAP);
  assert.equal(r.record.runs[0]?.shown[0], 11);
  let record = emptyRecord();
  for (let i = 0; i < RUNS_KEPT + 3; i++) record = decide(record, `keel-1c.20261122-0800${String(i).padStart(2, '0')}.1`, [], 'look').record;
  assert.equal(record.runs.length, RUNS_KEPT);
  assert.equal(record.runs.at(-1)?.run, `keel-1c.20261122-0800${String(RUNS_KEPT + 2).padStart(2, '0')}.1`);
});

test('an alert\'s text is made plain: control and bidi characters blanked, so a worker\'s text cannot reorder or split it', () => {
  assert.deepEqual(alertOf(alert(1, { headline: 'Sign\u202e now\n', lines: ['a\u0007b', 'c\u2028d'] })), { title: 'Sign  now ', body: 'a b\nc d', wu: null });
});

test('the record parses only its own shape; anything else, version 1\'s included, is no record', () => {
  assert.deepEqual(parseRecord(rec([RUN, [1, 2]], [OLD, []])), rec([RUN, [1, 2]], [OLD, []]));
  assert.deepEqual(parseRecord({ version: 2, runs: [] }), emptyRecord());
  for (const bad of [null, [], { version: 1, run: RUN, shown: [] }, { version: 2 }, { version: 2, runs: {} }, { version: 2, runs: [{ run: 7, shown: [] }] }, { version: 2, runs: [{ run: RUN, shown: [0] }] }, { version: 2, runs: [{ run: RUN, shown: ['1'] }] }, { version: 2, runs: [{ run: RUN }] }]) {
    assert.equal(parseRecord(bad), null, JSON.stringify(bad));
  }
});

test('a keel link opens only keel://wu/<work unit>; the router\'s own grammar admits more, and all of that is refused', () => {
  assert.deepEqual(parseLink('keel://wu/E-APPLIST.W2'), { wu: 'E-APPLIST.W2' });
  assert.deepEqual(parseLink('keel://wu/E-A1.W123'), { wu: 'E-A1.W123' });
  const refused = [
    'keel://../..', 'keel://wu/../../etc', 'keel://wu/E-APPLIST.W2/', 'keel://wu/E-APPLIST.W2?x=1', 'keel://wu/E-APPLIST.W2#a',
    'keel://wu/%45-APPLIST.W2', 'keel://wu/e-applist.w2', 'keel://epic/E-APPLIST', 'keel://wu/E-APPLIST', 'keel://wu/E-APPLIST.W', 'keel://wu/E-.W1',
    'KEEL://wu/E-A.W1', 'keel:wu/E-A.W1', 'keel:///wu/E-A.W1', 'https://wu/E-A.W1', ' keel://wu/E-A.W1', 'keel://wu/E-A.W1\n', 'keel://wu/E-A.W1234',
    `keel://wu/E-${'A'.repeat(17)}.W1`, '', 7, null, undefined, { toString: () => 'keel://wu/E-A.W1' },
  ];
  for (const url of refused) assert.equal(parseLink(url), null, JSON.stringify(String(url)));
  assert.equal(LINK_CAP, 64);
});

test('property: of generated keel:// strings, every one taken is exactly keel://wu/E-<CODE>.W<m>', () => {
  const rng = seeded(0x11c);
  // Near-valid links, then mutated: a code, the .W, a number and a tail, each sometimes wrong.
  const code = [...'APL0189Z'.repeat(4), '-', '.', 'a', '%', '/', ' '];
  const digits = [...'0123456789'.repeat(3), 'a', '/', '?', '.'];
  let taken = 0;
  for (let i = 0; i < 5000; i++) {
    const url = pick(rng, ['keel://wu/E-', 'keel://wu/E-', 'keel://wu/', 'keel://wu/e-', 'KEEL://wu/E-', 'keel:/wu/E-', 'keel://epic/E-'])
      + hostileString(rng, 18, code) + pick(rng, ['.W', '.W', '.w', 'W', '.W.', '']) + hostileString(rng, 4, digits) + pick(rng, ['', '', '', '/', '?x', '#', '\n', '%2e']);
    const r = parseLink(url);
    if (r === null) continue;
    taken++;
    assert.match(url, /^keel:\/\/wu\/E-[A-Z0-9]{1,16}\.W[0-9]{1,3}$/, url);
    assert.equal(`keel://wu/${r.wu}`, url);
  }
  assert.ok(taken > 20, `only ${taken} taken: too thin`);
});

test('a link sets the focus on its work unit and says so; a refused link leaves the focus as it was', () => {
  const f = new Focus();
  assert.equal(f.get(), null);
  assert.equal(openLink('keel://wu/E-A.W1', f), true);
  assert.deepEqual(f.get(), { wu: 'E-A.W1', seq: 1 });
  assert.equal(openLink('keel://../..', f), false);
  assert.deepEqual(f.get(), { wu: 'E-A.W1', seq: 1 });
  f.point(null);
  assert.deepEqual(f.get(), { wu: null, seq: 2 });
});

test('the scheme\'s owner in words: the app\'s name, plain; none or empty is unknown', () => {
  assert.equal(schemeText('Keel'), 'Keel');
  assert.equal(schemeText('Electron‮'), 'Electron ');
  assert.equal(schemeText(''), null);
  assert.equal(schemeText(undefined), null);
});

test('the seams for W9\'s test are read only when the app is not packaged; packaged, they are ignored', () => {
  const env = { KEEL_APP_ALERT_SINK: '/tmp/x/alerts.jsonl', KEEL_APP_SILENCE_MS: '2000' };
  assert.deepEqual(seamsOf(env, false), { alertSink: '/tmp/x/alerts.jsonl', silenceMs: 2000 });
  assert.deepEqual(seamsOf(env, true), { alertSink: null, silenceMs: null });
  assert.deepEqual(seamsOf({}, false), { alertSink: null, silenceMs: null });
  for (const [k, v] of [['KEEL_APP_ALERT_SINK', 'relative.jsonl'], ['KEEL_APP_ALERT_SINK', '/a\nb'], ['KEEL_APP_SILENCE_MS', '0'], ['KEEL_APP_SILENCE_MS', '-5'], ['KEEL_APP_SILENCE_MS', '1e3'], ['KEEL_APP_SILENCE_MS', '900001']]) {
    const s = seamsOf({ [k as string]: v }, false);
    assert.deepEqual(s, { alertSink: null, silenceMs: null }, `${k}=${v}`);
  }
});

/** A notification as Electron's: its listeners, shown or not, and fire() for what macOS does. */
function fakeNote() {
  const on: Record<string, Array<() => void>> = {};
  const n: Clickable & { shown: boolean; fire(e: string): void } = {
    shown: false,
    on(event, listener) {
      (on[event] ??= []).push(listener);
      return n;
    },
    show() {
      n.shown = true;
    },
    fire(e) {
      for (const l of on[e] ?? []) l();
    },
  };
  return n;
}

test('the notification sink shows each alert and holds it until it is clicked, closed or fails, so its click is never lost', () => {
  const made: Array<ReturnType<typeof fakeNote>> = [];
  const sink = heldSink(() => {
    const n = fakeNote();
    made.push(n);
    return n;
  });
  const clicks: string[] = [];
  sink.show({ title: 'a', body: 'b', wu: 'E-A.W1' }, () => clicks.push('a'));
  sink.show({ title: 'c', body: 'd', wu: null }, () => clicks.push('c'));
  sink.show({ title: 'e', body: 'f', wu: null }, () => clicks.push('e'));
  assert.deepEqual(made.map((n) => n.shown), [true, true, true]);
  assert.equal(sink.held(), 3);
  made[0]?.fire('click');
  assert.deepEqual(clicks, ['a']);
  assert.equal(sink.held(), 2);
  made[1]?.fire('close');
  made[2]?.fire('failed');
  assert.equal(sink.held(), 0);
});

test('the sink holds at most its cap, dropping the oldest; a make that throws shows nothing and throws nothing', () => {
  const sink = heldSink(() => fakeNote());
  for (let i = 0; i < HELD_CAP + 5; i++) sink.show({ title: `t${i}`, body: 'b', wu: null }, () => undefined);
  assert.equal(sink.held(), HELD_CAP);
  const broken = heldSink(() => {
    throw new Error('no notification');
  });
  assert.doesNotThrow(() => broken.show({ title: 't', body: 'b', wu: null }, () => undefined));
  assert.equal(broken.held(), 0);
});

test('property: a link\'s work unit is exactly what the log takes as an alert\'s wu (one pattern, WU_SOURCE)', () => {
  assert.equal(WU_ID.source, `^${WU_SOURCE}$`);
  const rng = seeded(0x3b);
  const chars = [...'EAW.-019Z'.repeat(3), 'a', '_', '/'];
  for (let i = 0; i < 3000; i++) {
    const id = pick(rng, ['E-', 'E-', '']) + hostileString(rng, 20, chars);
    assert.equal(parseLink(`keel://wu/${id}`) !== null, WU_ID.test(id) && `keel://wu/${id}`.length <= LINK_CAP, id);
  }
});
