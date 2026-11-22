// A small pill naming a tool's source, always in words, so no state is told by colour alone; the colour
// only repeats what the words say: 'set' (set in Keel, blue), 'found' (found on the PATH, green),
// 'missing' (red, white words). Each pair reaches AA in both worlds (test-window/unit/contrast.test.ts).
import type { ReactNode } from 'react';

const TONE = {
  set: 'bg-set text-on-set',
  found: 'bg-found text-on-found',
  missing: 'bg-missing text-on-missing',
} as const;

export type Tone = keyof typeof TONE;

export function Badge({ tone, children }: { tone: Tone; children: ReactNode }) {
  return <span className={`inline-flex h-badge shrink-0 items-center rounded-pill px-xs font-ui text-ui-sm ${TONE[tone]}`}>{children}</span>;
}
