// One section of a screen: its head in New York, in sentence case, then a white card on a 1px hairline
// with 12px corners (mx3a: no fill step, no shadow, no gradient). Rows inside are split by 1px rules.
import type { ReactNode } from 'react';

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-sm">
      <h2 className="m-0 font-serif text-section font-regular text-ink">{title}</h2>
      <div className="flex flex-col divide-y divide-line rounded-card border border-edge bg-card px-xl py-half">{children}</div>
    </section>
  );
}
