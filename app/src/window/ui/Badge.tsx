// A small capsule naming a state, always in words, so no state is told by colour alone. The design
// file keeps status greens and oranges out of the chrome, so a badge's tone is only how loud it is:
// 'choice' (a value you set: the indigo tint of the actions), 'quiet' (fine as it is: a faint ink
// wash), 'strong' (needs you: the polarity flipped, ink fill and canvas text, in both worlds).
import type { ReactNode } from 'react';

const TONE = {
  choice: 'bg-tint/tint text-ink',
  quiet: 'bg-ink/tint text-ink-secondary',
  strong: 'bg-ink text-canvas',
} as const;

export type Tone = keyof typeof TONE;

export function Badge({ tone, children }: { tone: Tone; children: ReactNode }) {
  return <span className={`inline-flex h-badge shrink-0 items-center rounded-pill px-xs font-ui text-ui-sm ${TONE[tone]}`}>{children}</span>;
}
