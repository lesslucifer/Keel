// A small label for a state. Its tone carries a meaning: accent for a choice you made, ok for done or
// passing, wait for waits on someone, fail for failed or missing; muted for none of those.
import type { ReactNode } from 'react';

const TONE = {
  accent: 'border-accent text-accent',
  ok: 'border-ok text-ok',
  wait: 'border-wait text-wait',
  fail: 'border-fail text-fail',
  muted: 'border-line text-ink-muted',
} as const;

export type Tone = keyof typeof TONE;

export function Badge({ tone, children }: { tone: Tone; children: ReactNode }) {
  return <span className={`inline-flex items-center rounded-full border px-2 text-caption font-medium ${TONE[tone]}`}>{children}</span>;
}
