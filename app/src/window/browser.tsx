// The browser mode's entry (npm run dev:browser, DEC-74): the window's screens in a plain browser, no
// Electron and no main part, against the fake bridge built from fixtures. ?state= picks the scenario.
// Never part of the shipped page: the renderer's build input is index.html alone, and a node guard
// proves nothing main.tsx imports reaches this file or fake/.
import './styles.css';
import { StrictMode, useMemo } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';
import { BridgeProvider } from './bridge.ts';
import { makeFakeBridge } from './fake/fake.ts';
import { SCENARIO_NAMES, scenarioOf } from './fake/fixtures.ts';

/** The window's App on the fake bridge of the scenario `search` names, under a fixed banner. */
export function BrowserApp({ search }: { search: string }) {
  const scenario = scenarioOf(new URLSearchParams(search).get('state'));
  const bridge = useMemo(() => makeFakeBridge(scenario), [scenario]);
  return (
    <>
      <div className="flex flex-wrap items-center gap-sm bg-card px-xxl py-xs text-ui-sm text-ink-secondary">
        <span className="font-medium text-ink">Browser mode: fixtures, not your Mac</span>
        <nav aria-label="Fixture states" className="flex flex-wrap gap-sm">
          {SCENARIO_NAMES.map((name) => (
            <a key={name} href={`?state=${name}`} aria-current={name === scenario ? 'page' : undefined} className={name === scenario ? 'font-medium text-ink' : 'text-ink underline'}>
              {name}
            </a>
          ))}
        </nav>
      </div>
      <BridgeProvider bridge={bridge}>
        <App />
      </BridgeProvider>
    </>
  );
}

const root = typeof document === 'undefined' ? null : document.getElementById('root');
if (root !== null) {
  createRoot(root).render(
    <StrictMode>
      <BrowserApp search={window.location.search} />
    </StrictMode>,
  );
}
