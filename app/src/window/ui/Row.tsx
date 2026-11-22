// A row inside a panel: what it is on the left, its actions on the right; it wraps when narrow.
// With a label it is a named group, so a screen reader (and a test) can find it by that name.
import { useId, type ReactNode } from 'react';

export function Row({ label, children, actions }: { label?: string; children: ReactNode; actions?: ReactNode }) {
  const id = useId();
  return (
    <div role={label === undefined ? undefined : 'group'} aria-labelledby={label === undefined ? undefined : id} className="flex flex-wrap items-start justify-between gap-3">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        {label === undefined ? null : (
          <span id={id} className="font-semibold text-ink">
            {label}
          </span>
        )}
        {children}
      </div>
      {actions === undefined ? null : <div className="flex flex-wrap gap-2">{actions}</div>}
    </div>
  );
}
