// Lint fixture: every class a token or a structural utility; the rule reports nothing.
const TONE = { ok: 'text-ok', fail: 'text-fail' } as const;

export function Clean({ on, tone }: { on: boolean; tone: keyof typeof TONE }) {
  return (
    <div className="flex items-center gap-2 rounded-panel border border-line bg-surface p-4 text-body text-ink">
      <span className={on ? 'font-semibold' : 'text-ink-muted'}>a</span>
      <span className={TONE[tone]}>b</span>
      <button type="button" className="h-control rounded-control bg-accent px-3 text-on-accent hover:bg-accent disabled:opacity-disabled">c</button>
      <code className={`font-mono ${on ? 'break-all' : ''}`}>d</code>
    </div>
  );
}
