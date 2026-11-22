// The window's frame: a quiet top line (Keel, and the project's name or 'No project') on the canvas,
// the screen's title, and one screen, screen 6 (Projects). E-APPLIST.W6 adds the other screens and
// moving between them. While the state is not yet read the screen says so; a first read that fails
// says why, as text. No always-dark bar: light and dark follow the Mac.

import { useAction, useAppState } from './hooks.ts';
import { Projects } from './screens/Projects.tsx';

export function App() {
  const { state, error } = useAppState();
  const action = useAction();
  const project = state?.projects[0];

  return (
    <div className="flex min-h-full flex-col bg-canvas text-body text-ink">
      <header className="flex items-baseline gap-sm px-xxl pt-xl">
        <span className="text-title font-display text-ink">Keel</span>
        {state === null ? null : <span className="text-ui-sm text-ink-secondary">{project === undefined ? 'No project' : project.name}</span>}
      </header>
      <main className="mx-auto flex w-full max-w-column flex-1 flex-col gap-xl px-xxl py-xl">
        <h1 className="m-0 text-heading font-display text-ink">Projects</h1>
        {state === null ? (
          error === '' ? (
            <p className="m-0 text-ink-secondary">Loading…</p>
          ) : (
            <p role="alert" className="m-0 font-medium text-ink">
              {`Keel could not read its state. ${error}`}
            </p>
          )
        ) : (
          <Projects state={state} action={action} error={error} />
        )}
      </main>
      {state === null ? null : <footer className="px-xxl pb-xl text-legal text-ink-secondary">{`Keel ${state.version}`}</footer>}
    </div>
  );
}
