// Lint fixture: honest code the rule must not refuse - a helper read by its returns (a throw, a text
// constant, an exhaustive switch), group and peer markers, a same-named variable in another scope.
type Tone = 'a' | 'b';

function toneOf(k: Tone): string {
  if ((k as string) === 'x') throw new Error('bad tone');
  const note = 'Remove from the list';
  void note;
  return k === 'a' ? 'text-ink' : 'text-ink-secondary';
}

const sizeOf = (k: Tone): string => {
  switch (k) {
    case 'a':
      return 'p-xs';
    case 'b':
      return 'p-sm';
    default: {
      const never: never = k;
      throw new Error(String(never));
    }
  }
};

const look = 'flex';

export function Honest({ k }: { k: Tone }) {
  return (
    <div className={`group ${look}`}>
      <span className={toneOf(k)}>a</span>
      <span className={`peer ${sizeOf(k)}`}>b</span>
      <span className="group-hover:text-ink peer-checked:bg-primary">c</span>
    </div>
  );
}

export function Other() {
  const look = 'Looks fine to me';
  return <p>{look}</p>;
}
