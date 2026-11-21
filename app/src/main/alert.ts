// The test alert: one macOS notification, and what became of it. The shell makes Electron's
// Notification; this module shows it and waits: shown, not shown within the wait (macOS said no,
// the owner has not allowed it yet, or show() failed), or not supported on this Mac.

import type { AlertOutcome } from '../shared/contract.ts';

export const ALERT_TITLE = 'Keel';
export const ALERT_BODY = 'Test alert: alerts work on this Mac.';
export const ALERT_WAIT_MS = 5_000;

/** The part of Electron's Notification this module uses. */
export interface NotificationLike {
  on(event: 'show' | 'failed', listener: (...args: unknown[]) => void): unknown;
  show(): void;
}

/** Show one notification made by `make` and answer how it went; never throws, never waits past `waitMs`. */
export function testAlert(supported: boolean, make: () => NotificationLike, waitMs: number = ALERT_WAIT_MS): Promise<AlertOutcome> {
  if (!supported) return Promise.resolve('not-supported');
  return new Promise((resolve) => {
    let settled = false;
    // Held until the answer, so the notification is not collected before macOS shows it.
    let held: NotificationLike | null = null;
    const done = (outcome: AlertOutcome) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      held = null;
      resolve(outcome);
    };
    const timer = setTimeout(() => done('not-shown'), waitMs);
    try {
      held = make();
      held.on('show', () => done('shown'));
      held.on('failed', () => done('not-shown'));
      held.show();
    } catch {
      done('not-shown');
    }
  });
}
