// The bare page, unstyled, now on the hooks: the three tools and their state, the PATH's source, the
// project, the version and the test alert, asked again on state.changed. Strings are rendered as
// React text, never as markup. Screen 6 replaces it (E-APPLIST.W8, step 7).

import { TOOL_NAMES } from '../shared/contract.ts';
import { useAction, useAppState } from './hooks.ts';

export function App() {
  const { state, error } = useAppState();
  const { busy, note, run } = useAction();

  if (state === null) return <main>{error === '' ? <p>Loading…</p> : <p>Keel could not read its state ({error}).</p>}</main>;

  return (
    <main>
      <h1>Keel</h1>
      <p>Version {state.version}</p>

      <h2>Tools</h2>
      <ul>
        {TOOL_NAMES.map((tool) => {
          const t = state.tools[tool];
          return (
            <li key={tool}>
              {tool}: {t.path ?? 'not found'} ({t.source === 'set' ? 'set in Keel' : t.source === 'path' ? 'found on the PATH' : 'missing'})
              {t.problem === null ? '' : ` - ${t.problem}`}{' '}
              <button type="button" disabled={busy} onClick={() => void run({ name: 'settings.choose', args: { tool } })}>
                Choose…
              </button>{' '}
              {t.source === 'set' ? (
                <button type="button" disabled={busy} onClick={() => void run({ name: 'settings.reset', args: { tool } })}>
                  Use the PATH
                </button>
              ) : null}
            </li>
          );
        })}
      </ul>

      <h2>PATH</h2>
      <p>{state.path.source === 'login-shell' ? 'PATH from your login shell' : `PATH fallback: ${state.path.reason ?? 'no reason given'}`}</p>

      <h2>Project</h2>
      {state.projects.length === 0 ? (
        <p>
          No project.{' '}
          <button type="button" disabled={busy} onClick={() => void run({ name: 'projects.add', args: {} })}>
            Add a project…
          </button>
        </p>
      ) : (
        <ul>
          {state.projects.map((p) => (
            <li key={p.id}>
              {p.name} ({p.path}){' '}
              <button type="button" disabled={busy} onClick={() => void run({ name: 'projects.remove', args: { project: p.id } })}>
                Remove from the list
              </button>
            </li>
          ))}
        </ul>
      )}

      <h2>Alerts</h2>
      <p>
        <button type="button" disabled={busy} onClick={() => void run({ name: 'alert.test', args: {} })}>
          Send a test alert
        </button>
      </p>
      {note === '' ? null : <p role="status">{note}</p>}
      {error === '' ? null : <p role="alert">{error}</p>}
    </main>
  );
}
