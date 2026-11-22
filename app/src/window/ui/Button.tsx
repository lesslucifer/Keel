// A push button, drawn only from tokens. 'primary' is the screen's main action (the accent);
// 'plain' every other. Off while a request runs (the caller passes busy).
import type { ReactNode } from 'react';

const LOOK = {
  primary: 'border border-accent bg-accent text-on-accent',
  plain: 'border border-line bg-surface text-ink hover:bg-canvas',
} as const;

export function Button({ kind = 'plain', disabled, onClick, children }: { kind?: keyof typeof LOOK; disabled: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`inline-flex h-control shrink-0 items-center rounded-control px-3 text-body font-medium focus-visible:outline focus-visible:outline-accent disabled:opacity-disabled ${LOOK[kind]}`}
    >
      {children}
    </button>
  );
}
