// A button, drawn only from tokens, by the design file's rules: 'primary' is the one solid indigo
// capsule a group may hold (its most important action); 'plain' is every other action, a translucent
// tinted pill (indigo at 10% by day, periwinkle at 20% by night). Off while a request runs.
import type { ReactNode } from 'react';

const LOOK = {
  primary: 'rounded-xl bg-primary text-on-primary',
  plain: 'rounded-pill bg-tint/tint text-ink hover:bg-tint/tint-hover',
} as const;

export function Button({ kind = 'plain', disabled, onClick, children }: { kind?: keyof typeof LOOK; disabled: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`inline-flex h-control shrink-0 cursor-pointer items-center px-md font-ui text-ui focus-visible:outline focus-visible:outline-primary disabled:opacity-disabled ${LOOK[kind]}`}
    >
      {children}
    </button>
  );
}
