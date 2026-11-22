// The top bar's screens (E-APPLIST.W6): Waits on you, with how many wait on you, and Projects. Screens 3
// to 5 are A5's and have no tab yet. The current one is marked for a screen reader and drawn with an ink
// edge; tabs are outline pills, so a screen's one solid pill stays its main action (mx3a).
export type Screen = 'waits' | 'projects';

const TAB = 'inline-flex h-control shrink-0 cursor-pointer items-center gap-xs rounded-pill border px-md font-ui text-ui focus-visible:outline focus-visible:outline-primary';
const CURRENT = `${TAB} border-primary text-ink`;
const OTHER = `${TAB} border-outline text-ink-secondary hover:bg-ink/hover`;

export function Nav({ screen, yours, onPick }: { screen: Screen; yours: number | null; onPick: (s: Screen) => void }) {
  return (
    <nav aria-label="Screens" className="flex flex-wrap items-center gap-xs border-b border-edge bg-canvas px-xxl py-xs">
      <button type="button" aria-current={screen === 'waits' ? 'page' : undefined} className={screen === 'waits' ? CURRENT : OTHER} onClick={() => onPick('waits')}>
        {yours === null ? 'Waits on you' : `Waits on you (${yours})`}
      </button>
      <button type="button" aria-current={screen === 'projects' ? 'page' : undefined} className={screen === 'projects' ? CURRENT : OTHER} onClick={() => onPick('projects')}>
        Projects
      </button>
    </nav>
  );
}
