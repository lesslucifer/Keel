// Lint fixture: every class a token or a structural utility; the rule reports nothing.
const TONE = { quiet: 'text-ink-secondary', strong: 'font-medium text-ink' } as const;

export function Clean({ on, tone }: { on: boolean; tone: keyof typeof TONE }) {
  return (
    <div className="flex items-center gap-sm rounded-card border border-edge bg-card p-xl text-body text-ink">
      <span className={on ? 'font-serif text-heading' : 'text-ink-secondary'}>a</span>
      <span className={TONE[tone]}>b</span>
      <button type="button" className="h-control rounded-pill bg-primary px-lg font-ui text-ui text-on-primary hover:bg-ink/hover disabled:opacity-disabled">c</button>
      <span className="bg-missing text-section font-medium text-on-missing">e</span>
      <code className={`font-mono ${on ? 'break-all' : ''}`}>d</code>
    </div>
  );
}
