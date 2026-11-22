// One section of a screen: a heading and a panel.
import type { ReactNode } from 'react';

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="text-title font-semibold text-ink">{title}</h2>
      <div className="flex flex-col gap-3 rounded-panel border border-line bg-surface p-4">{children}</div>
    </section>
  );
}
