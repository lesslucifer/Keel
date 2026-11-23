// Lint fixture: two classes built while the app runs, which no static check could read.
export function Fragments({ n }: { n: number }) {
  return (
    <p className={'p-' + n}>
      <span className={`gap-${n}`}>a</span>
    </p>
  );
}
