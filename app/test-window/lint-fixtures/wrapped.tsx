// Lint fixture: off-list classes in tables wrapped in as const, satisfies, as T and !, each reported.
const A = { x: 'bg-red-500' } as const;
const B = { x: 'text-sm' } satisfies Record<string, string>;
const C = { x: 'p-[13px]' } as Record<string, string>;
const D: Record<string, string | undefined> = { x: 'dark:bg-card' };

export function Wrapped({ k }: { k: 'x' }) {
  return (
    <p className={A[k]}>
      <span className={B[k]}>b</span>
      <span className={C[k]}>c</span>
      <span className={D[k]!}>d</span>
    </p>
  );
}
