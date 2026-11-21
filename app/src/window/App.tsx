// The bare page, unstyled: the three tools and their state, the PATH's source, the project, the
// version and the test alert, each from the main part through ./bridge.ts, asked again on
// state.changed. Strings are rendered as React text, never as markup. W8 replaces it with screen 6.

import { useCallback, useEffect, useState } from 'react';
import { TOOL_NAMES, type AlertOutcome, type Answer, type AppState, type Change, type Tool } from '../shared/contract.ts';
import { ask, onPush } from './bridge.ts';

const ALERT_TEXT: Record<AlertOutcome, string> = {
  shown: 'The test alert was shown.',
  'not-shown': 'The test alert was not shown within 5 s. Check System Settings > Notifications > Keel.',
  'not-supported': 'This Mac does not support alerts from Keel.',
};

function problemOf(a: Answer): string {
  return a.ok ? '' : 'refused' in a ? `refused: ${a.refused}` : `failed: ${a.failed}`;
}

function changeText(a: Answer<Change>): string {
  if (!a.ok) return problemOf(a);
  return a.value.done ? 'Done.' : `Not done: ${a.value.why}.`;
}

export function App() {
  const [state, setState] = useState<AppState | null>(null);
  const [error, setError] = useState('');
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    const a = await ask('state.get', {});
    if (a.ok) {
      setState(a.value);
      setError('');
    } else {
      setError(problemOf(a));
    }
  }, []);

  useEffect(() => {
    void refresh();
    return onPush('state.changed', () => void refresh());
  }, [refresh]);

  const act = async (run: () => Promise<string>) => {
    setBusy(true);
    try {
      setNote(await run());
    } finally {
      setBusy(false);
    }
  };

  const testAlert = () =>
    act(async () => {
      const a = await ask('alert.test', {});
      return a.ok ? ALERT_TEXT[a.value] : problemOf(a);
    });
  const choose = (tool: Tool) => act(async () => changeText(await ask('settings.choose', { tool })));
  const reset = (tool: Tool) => act(async () => changeText(await ask('settings.reset', { tool })));
  const addProject = () => act(async () => changeText(await ask('projects.add', {})));
  const removeProject = (project: string) => act(async () => changeText(await ask('projects.remove', { project })));

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
              <button type="button" disabled={busy} onClick={() => void choose(tool)}>
                Choose…
              </button>{' '}
              {t.source === 'set' ? (
                <button type="button" disabled={busy} onClick={() => void reset(tool)}>
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
          <button type="button" disabled={busy} onClick={() => void addProject()}>
            Add a project…
          </button>
        </p>
      ) : (
        <ul>
          {state.projects.map((p) => (
            <li key={p.id}>
              {p.name} ({p.path}){' '}
              <button type="button" disabled={busy} onClick={() => void removeProject(p.id)}>
                Remove from the list
              </button>
            </li>
          ))}
        </ul>
      )}

      <h2>Alerts</h2>
      <p>
        <button type="button" disabled={busy} onClick={() => void testAlert()}>
          Send a test alert
        </button>
      </p>
      {note === '' ? null : <p role="status">{note}</p>}
      {error === '' ? null : <p role="alert">{error}</p>}
    </main>
  );
}
