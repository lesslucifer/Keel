// The fake bridge's fixtures: named scenarios the browser mode picks with ?state=, and the tests draw
// from. Each state has the contract's shapes (test-window/unit/fixtures.test.ts) and paths shaped like
// a real Mac's. Never part of the shipped page: only browser.tsx and the window's tests import fake/.

import type { AppState, Tool, ToolState, View } from '../../shared/contract.ts';

export const SCENARIO_NAMES = Object.freeze(['one-project', 'no-project', 'tool-missing', 'path-fallback', 'refusing', 'failing', 'slow'] as const);
export type ScenarioName = (typeof SCENARIO_NAMES)[number];

export interface Scenario {
  /** What state.get answers first. */
  state: AppState;
  /** 'answer' runs each request; 'refuse' refuses every request (state.get too); 'fail' reads state and fails every other request. */
  answers: 'answer' | 'refuse' | 'fail';
  /** How long each answer waits, in ms. */
  delayMs: number;
  /** What waiting.get answers. */
  view: View;
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

// ---- The waiting view (waiting.get): named views the tests draw from; each scenario carries one. ----

export const VIEW_NAMES = Object.freeze(['live', 'no-run', 'not-heard', 'after-restart', 'focus'] as const);
export type ViewName = (typeof VIEW_NAMES)[number];

const RUN = 'keel-1c.20261122-083000.4242';
const EPIC_TIP = 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678';
const WU_TIP = '0f1e2d3c4b5a69788796a5b4c3d2e1f001234567';
const MAIN_TIP = '9a8b7c6d5e4f30211203f4e5d6c7b8a998765432';

function liveView(): View {
  return {
    project: { id: 'p0a1b2c3d', name: 'keel' },
    git: {
      branches: [
        { name: 'main', tip: MAIN_TIP, upstream: 'origin/main', ahead: 1, behind: 0, gone: false },
        { name: 'epic/E-APPLIST', tip: EPIC_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
        { name: 'wu/E-APPLIST/w5-reader-watcher', tip: WU_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
      ],
      notes: { mr: [], cr: [WU_TIP], dod: [] },
      merged: { main: [], 'epic/E-APPLIST': [] },
      worktrees: [
        { path: '/Users/you/code/keel', branch: 'main', tip: MAIN_TIP },
        { path: '/Users/you/code/keel-w5', branch: 'wu/E-APPLIST/w5-reader-watcher', tip: WU_TIP },
      ],
      problem: null,
    },
    router: {
      run: RUN,
      n: 42,
      values: {
        router: { name: 'keel-1c', state: 'busy', workers: 1 },
        'wu/E-APPLIST.W5/stage': { stage: 'in review', tip: '0f1e2d3' },
        'worker/e-applist-w5': { state: 'busy', branch: 'wu/E-APPLIST/w5-reader-watcher', tip: '0f1e2d3', dirty: 0, active: '2026-11-22T08:41:00Z' },
        'question/merge-order': { headline: 'Merge W5 before W6?', lines: ['W6 builds on W5.'], wu: 'E-APPLIST.W5' },
      },
      heard: true,
      lastAt: '2026-11-22T08:45:00+08:00',
      problem: null,
    },
    alerts: [
      { n: 40, at: '2026-11-22T08:40:00+08:00', headline: 'Plan to read: E-APPLIST.W5', lines: ['The plan is committed.'], wu: 'E-APPLIST.W5', link: 'keel://wu/E-APPLIST.W5', shown: true },
      { n: 41, at: '2026-11-22T08:44:00+08:00', headline: 'Question: merge order', lines: ['Merge W5 before W6?'], wu: null, link: null, shown: false },
    ],
    focus: null,
    scheme: 'Keel',
    readAt: '2026-11-22T08:45:01+08:00',
  };
}

const viewWith = (change: (v: View) => void): View => {
  const v = liveView();
  change(v);
  return v;
};

export const VIEWS: Readonly<Record<ViewName, View>> = Object.freeze({
  live: liveView(),
  'no-run': viewWith((v) => {
    v.router = { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null };
    v.alerts = [];
  }),
  'not-heard': viewWith((v) => {
    v.router.heard = false;
    v.router.lastAt = '2026-11-22T08:20:00+08:00';
  }),
  'after-restart': viewWith((v) => {
    v.router = { run: 'keel-1c.20261122-090000.4343', n: 1, values: {}, heard: true, lastAt: '2026-11-22T09:00:00+08:00', problem: null };
    v.alerts = [];
  }),
  focus: viewWith((v) => (v.focus = { wu: 'E-APPLIST.W5', seq: 1 })),
});

/** The view with no project listed: nothing read. */
export function noProjectView(): View {
  return { project: null, git: null, router: { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null }, alerts: [], focus: null, scheme: 'Keel', readAt: null };
}

export const SCENARIOS: Readonly<Record<ScenarioName, Scenario>> = Object.freeze({
  'one-project': { state: base(), answers: 'answer', delayMs: 0, view: liveView() },
  'no-project': { state: with_((s) => (s.projects = [])), answers: 'answer', delayMs: 0, view: noProjectView() },
  'tool-missing': {
    state: with_((s) => {
      s.tools.claude = { path: null, source: 'missing', problem: 'not found on the PATH' };
      s.tools.git = { path: '/usr/local/old/git', source: 'set', problem: 'git 2.14.1 is older than 2.15, the oldest Keel runs' };
    }),
    answers: 'answer',
    delayMs: 0,
    view: liveView(),
  },
  'path-fallback': {
    state: with_((s) => (s.path = { entries: [...FALLBACK_PATH], source: 'fallback', reason: 'the login shell did not answer within 5 s' })),
    answers: 'answer',
    delayMs: 0,
    view: liveView(),
  },
  refusing: { state: base(), answers: 'refuse', delayMs: 0, view: liveView() },
  failing: { state: base(), answers: 'fail', delayMs: 0, view: liveView() },
  slow: { state: base(), answers: 'answer', delayMs: 800, view: liveView() },
});

/** The scenario a ?state= value names; anything else is the default, one-project. */
export function scenarioOf(value: string | null): ScenarioName {
  return (SCENARIO_NAMES as readonly string[]).includes(value ?? '') ? (value as ScenarioName) : 'one-project';
}
