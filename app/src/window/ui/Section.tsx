// One section of a screen: a small uppercase eyebrow, then a card that stands off the canvas by fill
// alone (lavender by day, night-raised by night; no border, no shadow). Rows inside are split by
// 1px rules.
import type { ReactNode } from 'react';

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-xs">
      <h2 className="m-0 px-half text-eyebrow font-medium uppercase text-ink-secondary">{title}</h2>
      <div className="flex flex-col divide-y divide-line rounded-md bg-card px-xl py-half">{children}</div>
    </section>
  );
}
