// A button, drawn only from tokens, by mx3a's rules: 'primary' is the one solid pill a group may hold
// (its most important action: black by day, white by night); 'plain' is every other action, an outline
// pill with no fill whose hover is a faint wash of ink. Both carry a 1px edge, so they stand the same
// height. Off while a request runs.
import type { ReactNode } from 'react';

const LOOK = {
  primary: 'border-primary bg-primary text-on-primary',
  plain: 'border-outline text-ink hover:bg-ink/hover',
} as const;

export function Button({ kind = 'plain', disabled, onClick, children }: { kind?: keyof typeof LOOK; disabled: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`inline-flex h-control shrink-0 cursor-pointer items-center rounded-pill border px-md font-ui text-ui focus-visible:outline focus-visible:outline-primary disabled:opacity-disabled ${LOOK[kind]}`}
    >
      {children}
    </button>
  );
}
