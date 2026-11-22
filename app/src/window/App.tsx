// The window's frame: a top bar (Keel, and the project's name or 'No project') and one screen,
// screen 6 (Projects). E-APPLIST.W6 adds the other screens and moving between them. While the state
// is not yet read the screen says so; a first read that fails says why, as text.

import { useAction, useAppState } from './hooks.ts';
import { Projects } from './screens/Projects.tsx';

export function App() {
  const { state, error } = useAppState();
  const action = useAction();
  const project = state?.projects[0];

  return (
    <div className="flex min-h-full flex-col bg-canvas text-body text-ink">
      <header className="flex items-center gap-3 border-b border-line bg-surface px-4 py-2">
        <span className="text-title font-semibold">Keel</span>
        {state === null ? null : <span className="text-ink-muted">{project === undefined ? 'No project' : project.name}</span>}
      </header>
      <main className="mx-auto flex w-full max-w-column flex-1 flex-col gap-6 p-6">
        {state === null ? (
          error === '' ? (
            <p className="m-0 text-ink-muted">Loading…</p>
          ) : (
            <p role="alert" className="m-0 text-fail">
              {`Keel could not read its state. ${error}`}
            </p>
          )
        ) : (
          <Projects state={state} action={action} error={error} />
        )}
      </main>
      {state === null ? null : (
        <footer className="px-6 pb-4 text-caption text-ink-muted">{`Keel ${state.version}`}</footer>
      )}
    </div>
  );
}
