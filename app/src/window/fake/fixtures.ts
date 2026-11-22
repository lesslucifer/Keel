// The fake bridge's fixtures: named scenarios the browser mode picks with ?state=, and the tests draw
// from. Each state has the contract's shapes (test-window/unit/fixtures.test.ts) and paths shaped like
// a real Mac's. Never part of the shipped page: only browser.tsx and the window's tests import fake/.

import type { AppState, Tool, ToolState } from '../../shared/contract.ts';

export const SCENARIO_NAMES = Object.freeze(['one-project', 'no-project', 'tool-missing', 'path-fallback', 'refusing', 'failing', 'slow'] as const);
export type ScenarioName = (typeof SCENARIO_NAMES)[number];

export interface Scenario {
  /** What state.get answers first. */
  state: AppState;
  /** 'answer' runs each request; 'refuse' refuses every request (state.get too); 'fail' reads state and fails every other request. */
  answers: 'answer' | 'refuse' | 'fail';
  /** How long each answer waits, in ms. */
  delayMs: number;
}

/** Where each tool sits on the fixture PATH; Settings > Choose picks the CHOSEN one. */
export const ON_PATH: Record<Tool, string> = { keel: '/opt/homebrew/bin/keel', git: '/opt/homebrew/bin/git', claude: '/Users/you/.local/bin/claude' };
export const CHOSEN: Record<Tool, string> = { keel: '/Users/you/.cargo/bin/keel', git: '/usr/bin/git', claude: '/opt/homebrew/bin/claude' };

/** The project Add adds in a browser, where there is no folder dialog. */
export const NEXT_PROJECT = Object.freeze({ id: 'p7e3a91c4', path: '/Users/you/code/keel', name: 'keel' });

const LOGIN_PATH = ['/opt/homebrew/bin', '/opt/homebrew/sbin', '/Users/you/.local/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin'];
const FALLBACK_PATH = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin'];

const onPath = (tool: Tool): ToolState => ({ path: ON_PATH[tool], source: 'path', problem: null });

function base(): AppState {
  return {
    version: '0.1.0',
    path: { entries: [...LOGIN_PATH], source: 'login-shell', reason: null },
    tools: { keel: onPath('keel'), git: onPath('git'), claude: onPath('claude') },
    projects: [{ id: 'p0a1b2c3d', path: '/Users/you/code/keel', name: 'keel' }],
  };
}

const with_ = (change: (s: AppState) => void): AppState => {
  const s = base();
  change(s);
  return s;
};

export const SCENARIOS: Readonly<Record<ScenarioName, Scenario>> = Object.freeze({
  'one-project': { state: base(), answers: 'answer', delayMs: 0 },
  'no-project': { state: with_((s) => (s.projects = [])), answers: 'answer', delayMs: 0 },
  'tool-missing': {
    state: with_((s) => {
      s.tools.claude = { path: null, source: 'missing', problem: 'not found on the PATH' };
      s.tools.git = { path: '/usr/local/old/git', source: 'set', problem: 'git 2.14.1 is older than 2.15, the oldest Keel runs' };
    }),
    answers: 'answer',
    delayMs: 0,
  },
  'path-fallback': {
    state: with_((s) => (s.path = { entries: [...FALLBACK_PATH], source: 'fallback', reason: 'the login shell did not answer within 5 s' })),
    answers: 'answer',
    delayMs: 0,
  },
  refusing: { state: base(), answers: 'refuse', delayMs: 0 },
  failing: { state: base(), answers: 'fail', delayMs: 0 },
  slow: { state: base(), answers: 'answer', delayMs: 800 },
});

/** The scenario a ?state= value names; anything else is the default, one-project. */
export function scenarioOf(value: string | null): ScenarioName {
  return (SCENARIO_NAMES as readonly string[]).includes(value ?? '') ? (value as ScenarioName) : 'one-project';
}
