// A row inside a card: what it is on the left, its actions on the right; it wraps when narrow.
// With a label it is a named group, so a screen reader (and a test) can find it by that name.
import { useId, type ReactNode } from 'react';

export function Row({ label, children, actions }: { label?: string; children: ReactNode; actions?: ReactNode }) {
  const id = useId();
  return (
    <div role={label === undefined ? undefined : 'group'} aria-labelledby={label === undefined ? undefined : id} className="flex flex-wrap items-center justify-between gap-md py-md">
      <div className="flex min-w-0 flex-1 flex-col gap-half">
        {label === undefined ? null : (
          <span id={id} className="text-body-lg font-display text-ink">
            {label}
          </span>
        )}
        {children}
      </div>
      {actions === undefined ? null : <div className="flex flex-wrap gap-xs">{actions}</div>}
    </div>
  );
}
