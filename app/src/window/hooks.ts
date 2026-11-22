// The screens' hooks over the bridge, and the sentences an answer becomes. useAppState holds the
// main part's state, asked on mount and again on each state.changed; useAction runs one request at
// a time and says its outcome. Every text here is shown as React text, never as markup.

import { useCallback, useEffect, useRef, useState } from 'react';
import type { AlertOutcome, Answer, AppState, Change, RequestName } from '../shared/contract.ts';
import { ask, onPush, useBridge, type Request } from './bridge.ts';

/** A sentence ends with one full stop. */
const sentence = (text: string) => (/[.!?]$/.test(text) ? text : `${text}.`);

/** Nothing for a value; the refusal (nothing ran) or the failure, in words. */
export function answerText(a: Answer): string {
  if (a.ok) return '';
  return 'refused' in a ? `Keel refused the request (${a.refused}).` : sentence(`The request failed: ${a.failed}`);
}

/** A change's outcome: `done` when it was done, else why not, or the refusal or failure. */
export function changeText(a: Answer<Change>, done = 'Done.'): string {
  if (!a.ok) return answerText(a);
  return a.value.done ? done : sentence(`Not done: ${a.value.why}`);
}

const ALERT_TEXT: Record<AlertOutcome, string> = {
  shown: 'The test alert was shown.',
  'not-shown': 'The test alert was not shown within 5 s. Check System Settings > Notifications > Keel.',
  'not-supported': 'This Mac does not support alerts from Keel.',
};

/** The test alert's outcome in words. */
export function alertText(a: Answer<AlertOutcome>): string {
  return a.ok ? ALERT_TEXT[a.value] : answerText(a);
}

/** What each change says when it was done. */
const DONE_TEXT: Record<Exclude<RequestName, 'state.get' | 'alert.test' | 'waiting.get' | 'item.mark'>, string> = {
  'item.open': 'Opened in your browser.',
  'projects.add': 'Project added.',
  'projects.remove': 'Project removed from the list. Add brings it back.',
  'settings.choose': 'Saved: Keel uses the chosen file.',
  'settings.reset': 'Saved: Keel uses the one on the PATH.',
};

/** The sentence a request's answer becomes. */
function outcomeText(request: Request, a: Answer): string {
  switch (request.name) {
    case 'state.get':
    case 'waiting.get':
      return answerText(a);
    case 'alert.test':
      return alertText(a as Answer<AlertOutcome>);
    case 'item.mark':
      return changeText(a as Answer<Change>, request.args.sent === 'yes' ? 'Marked sent: it waits for a sign under Sent.' : 'Back on your list.');
    default:
      return changeText(a as Answer<Change>, DONE_TEXT[request.name]);
  }
}

export interface AppStateView {
  /** The last state the main part gave; null until the first. */
  state: AppState | null;
  /** The last ask's refusal or failure in words; '' once an ask succeeds. */
  error: string;
  /** True until the first answer. */
  loading: boolean;
}

/** The main part's state: asked on mount and on each state.changed; the newest ask wins. */
export function useAppState(): AppStateView {
  const bridge = useBridge();
  const [view, setView] = useState<AppStateView>({ state: null, error: '', loading: true });
  const latest = useRef(0);

  useEffect(() => {
    let live = true;
    const refresh = async () => {
      const mine = (latest.current += 1);
      let a: Answer<AppState>;
      try {
        a = await ask(bridge, 'state.get', {});
      } catch (e) {
        a = { ok: false, failed: e instanceof Error ? e.message : String(e) };
      }
      if (!live || mine !== latest.current) return;
      setView((v) => (a.ok ? { state: a.value, error: '', loading: false } : { state: v.state, error: answerText(a), loading: false }));
    };
    void refresh();
    const off = onPush(bridge, 'state.changed', () => void refresh());
    return () => {
      live = false;
      off();
    };
  }, [bridge]);

  return view;
}

export interface ActionView {
  /** True while a request runs; every control is off then. */
  busy: boolean;
  /** The last request's outcome, a sentence; '' before the first. */
  note: string;
  /** Send `request` unless one is running; resolves when its outcome is shown. */
  run(request: Request): Promise<void>;
}

/** One request at a time, and its outcome as a sentence. */
export function useAction(): ActionView {
  const bridge = useBridge();
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState('');
  const running = useRef(false);

  const run = useCallback(
    async (request: Request) => {
      if (running.current) return;
      running.current = true;
      setBusy(true);
      try {
        const a = (await bridge.ask(request.name, request.args as Record<string, string>)) as Answer;
        setNote(outcomeText(request, a));
      } catch (e) {
        setNote(answerText({ ok: false, failed: e instanceof Error ? e.message : String(e) }));
      } finally {
        running.current = false;
        setBusy(false);
      }
    },
    [bridge],
  );

  return { busy, note, run };
}
