// The seams W9's epic test drives the unpackaged app through (E-APPLIST e2e-plan): a file sink in place
// of macOS's alert, a shorter silence than 15 minutes, (W6) a file sink in place of the browser Open
// hands a page to, and (W9) an app folder of its own, so the test never reads or writes the owner's
// and never meets a running Keel's single-instance lock. Read only when the app is not packaged; the
// packaged app ignores every variable, so none can change what the owner's app does.

import { appendFileSync } from 'node:fs';
import path from 'node:path';
import type { AlertSink } from './alerts.ts';
import { SILENCE_MS } from '../reader/log.ts';

export interface Seams {
  /** KEEL_APP_ALERT_SINK: an absolute file each alert is appended to as one JSON line. */
  alertSink: string | null;
  /** KEEL_APP_SILENCE_MS: the silence before 'router not heard', 1 ms to 15 minutes. */
  silenceMs: number | null;
  /** KEEL_APP_OPEN_SINK: an absolute file each URL Open would hand the browser is appended to, as one JSON line. */
  openSink: string | null;
  /** KEEL_APP_DIR: an absolute folder used as Electron's userData (the app folder and its single-instance lock); never the root. */
  appDir: string | null;
}

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

export function seamsOf(env: Record<string, string | undefined>, packaged: boolean): Seams {
  if (packaged) return { alertSink: null, silenceMs: null, openSink: null, appDir: null };
  const sink = env.KEEL_APP_ALERT_SINK;
  const open = env.KEEL_APP_OPEN_SINK;
  const dir = env.KEEL_APP_DIR;
  const appDir = typeof dir === 'string' && path.isAbsolute(dir) && !CONTROL.test(dir) ? path.resolve(dir) : null;
  const silence = env.KEEL_APP_SILENCE_MS;
  const ms = typeof silence === 'string' && /^[1-9][0-9]{0,8}$/.test(silence) ? Number(silence) : null;
  return {
    alertSink: typeof sink === 'string' && path.isAbsolute(sink) && !CONTROL.test(sink) ? sink : null,
    silenceMs: ms !== null && ms <= SILENCE_MS ? ms : null,
    openSink: typeof open === 'string' && path.isAbsolute(open) && !CONTROL.test(open) ? open : null,
    appDir: appDir !== null && appDir !== path.parse(appDir).root ? appDir : null,
  };
}

/** An opener that appends each URL to `file` as one JSON line, with the time; no browser opens. */
export function openFileSink(file: string): (url: string) => void {
  return (url) => appendFileSync(file, `${JSON.stringify({ url, at: new Date().toISOString() })}\n`, { mode: 0o600 });
}

/** A sink that appends each alert to `file` as one JSON line, with the time it was shown; a click never comes. */
export function fileSink(file: string): AlertSink {
  return {
    show: (alert) => appendFileSync(file, `${JSON.stringify({ ...alert, at: new Date().toISOString() })}\n`, { mode: 0o600 }),
  };
}
