// The browser mode's entry (npm run dev:browser, DEC-74): the window's screens in a plain browser, no
// Electron and no main part, against the fake bridge built from fixtures. ?state= picks the scenario;
// ?scheme=light|dark forces one world (data-scheme on the page's root, fake/scheme.css) so the owner
// sees both without changing the Mac; without it the Mac picks, as in the shipped page.
// Never part of the shipped page: the renderer's build input is index.html alone, and a node guard
// proves nothing main.tsx imports reaches this file or fake/.
import './styles.css';
import './fake/scheme.css';
import { StrictMode, useLayoutEffect, useMemo } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';
import { BridgeProvider } from './bridge.ts';
import { makeFakeBridge } from './fake/fake.ts';
import { SCENARIO_NAMES, scenarioOf } from './fake/fixtures.ts';

const SCHEMES = ['light', 'dark'] as const;
type Scheme = (typeof SCHEMES)[number];

/** The world a ?scheme= value forces, or null to follow the Mac. */
function schemeOf(value: string | null): Scheme | null {
  return value === 'light' || value === 'dark' ? value : null;
}

/** A browser-mode link keeping both choices: the scenario, and the forced world when there is one. */
function hrefOf(state: string, scheme: Scheme | null): string {
  return `?state=${state}${scheme === null ? '' : `&scheme=${scheme}`}`;
}

const CURRENT = 'font-medium text-ink';
const LINK = 'text-ink underline';

/** The window's App on the fake bridge of the scenario `search` names, under a fixed banner. */
export function BrowserApp({ search }: { search: string }) {
  const params = new URLSearchParams(search);
  const scenario = scenarioOf(params.get('state'));
  const scheme = schemeOf(params.get('scheme'));
  const bridge = useMemo(() => makeFakeBridge(scenario), [scenario]);
  useLayoutEffect(() => {
    const root = document.documentElement;
    if (scheme === null) delete root.dataset.scheme;
    else root.dataset.scheme = scheme;
  }, [scheme]);
  return (
    <>
      <div className="flex flex-wrap items-center gap-sm bg-card px-xxl py-xs text-ui-sm text-ink-secondary">
        <span className="font-medium text-ink">Browser mode: fixtures, not your Mac</span>
        <nav aria-label="Fixture states" className="flex flex-wrap gap-sm">
          {SCENARIO_NAMES.map((name) => (
            <a key={name} href={hrefOf(name, scheme)} aria-current={name === scenario ? 'page' : undefined} className={name === scenario ? CURRENT : LINK}>
              {name}
            </a>
          ))}
        </nav>
        <nav aria-label="Appearance" className="flex flex-wrap gap-sm">
          {[null, ...SCHEMES].map((id) => (
            <a key={id ?? 'mac'} href={hrefOf(scenario, id)} aria-current={id === scheme ? 'page' : undefined} className={id === scheme ? CURRENT : LINK}>
              {id === null ? 'Follow the Mac' : id === 'light' ? 'Light' : 'Dark'}
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
