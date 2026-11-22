// The window's frame: a plain title bar on the canvas over a 1px hairline (the black brand mark, Keel,
// and the project's name or 'No project'), the screens' tabs (W6: Waits on you, with how many wait on
// you, and Projects), the screen's title in New York, and the screen. The app opens on Waits on you when
// a project is listed, else on Projects; a click on an alert or a keel:// link (a focus with a newer
// count) brings Waits on you forward on that work unit. While the state is not yet read the screen says
// so; a first read that fails says why, as text. No coloured band and no gradient: light and dark follow the Mac.

import { useEffect, useRef, useState } from 'react';
import { useAction, useAppState, useWaiting } from './hooks.ts';
import { Nav, type Screen } from './screens/Nav.tsx';
import { Projects } from './screens/Projects.tsx';
import { Waits } from './screens/Waits.tsx';

const TITLE: Record<Screen, string> = { waits: 'Waits on you', projects: 'Projects' };

export function App({ start }: { start?: Screen } = {}) {
  const { state, error } = useAppState();
  const waiting = useWaiting();
  const action = useAction();
  const project = state?.projects[0];
  const [picked, setPicked] = useState<Screen | null>(start ?? null);
  // The focus a click set, once the window has acted on it: a newer count acts again.
  const [focus, setFocus] = useState<{ wu: string | null; seq: number } | null>(null);
  const acted = useRef(0);

  const pointed = waiting.view?.focus ?? null;
  useEffect(() => {
    if (pointed === null || pointed.seq <= acted.current) return;
    acted.current = pointed.seq;
    setFocus(pointed);
    setPicked('waits');
  }, [pointed]);

  // The first state read picks the opening screen: Waits on you with a project listed, else Projects; it then stays until the owner or a click moves it.
  const listed = state === null ? null : project !== undefined;
  useEffect(() => {
    if (listed !== null) setPicked((p) => p ?? (listed ? 'waits' : 'projects'));
  }, [listed]);
  const screen: Screen = picked ?? (listed === true ? 'waits' : 'projects');
  const pick = (s: Screen) => {
    setPicked(s);
    if (s === 'projects') setFocus(null);
  };

  return (
    <div className="flex min-h-full flex-col bg-canvas text-body text-ink">
      <header className="flex items-center gap-sm border-b border-edge bg-canvas px-xxl py-row">
        <span aria-hidden="true" className="h-sm w-sm shrink-0 rounded-mark bg-ink" />
        <span className="text-title font-ui text-ink">Keel</span>
        {state === null ? null : <span className="text-ui-sm text-ink-secondary">{project === undefined ? 'No project' : project.name}</span>}
      </header>
      {state === null ? null : <Nav screen={screen} yours={waiting.view === null || waiting.view.project === null ? null : waiting.view.counts.yours} onPick={pick} />}
      <main className="mx-auto flex w-full max-w-column flex-1 flex-col gap-xxl px-xxl py-xxl">
        <h1 className="m-0 font-serif text-heading font-regular text-ink">{TITLE[screen]}</h1>
        {state === null ? (
          error === '' ? (
            <p className="m-0 text-ink-secondary">Loading…</p>
          ) : (
            <p role="alert" className="m-0 font-medium text-problem">
              {`Keel could not read its state. ${error}`}
            </p>
          )
        ) : screen === 'waits' ? (
          <Waits view={waiting.view} error={waiting.error} action={action} focus={focus} />
        ) : (
          <Projects state={state} action={action} error={error} />
        )}
      </main>
      {state === null ? null : <footer className="px-xxl pb-xl text-legal text-ink-secondary">{`Keel ${state.version}`}</footer>}
    </div>
  );
}
