// The fake bridge's fixtures: named scenarios the browser mode picks with ?state=, and the tests draw
// from. Each state has the contract's shapes (test-window/unit/fixtures.test.ts) and paths shaped like
// a real Mac's. Never part of the shipped page: only browser.tsx and the window's tests import fake/.

import type { AppState, GitFacts, Tool, ToolState, View } from '../../shared/contract.ts';
import { countsOf, itemId, itemsOf } from '../../shared/items.ts';

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
    packaged: true,
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

export const VIEW_NAMES = Object.freeze(['live', 'every-kind', 'nothing-waits', 'no-run', 'not-heard', 'after-restart', 'focus'] as const);
export type ViewName = (typeof VIEW_NAMES)[number];

const RUN = 'keel-1c.20261122-083000.4242';
const EPIC_TIP = 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678';
const WU_TIP = '0f1e2d3c4b5a69788796a5b4c3d2e1f001234567';
const MAIN_TIP = '9a8b7c6d5e4f30211203f4e5d6c7b8a998765432';

const NO_COUNTS = Object.freeze({ yours: 0, notYours: 0, sent: 0 });

/** `v` with its list drawn by the main part's own rules (src/shared/items.ts), `sent` the ids marked sent. */
export function withItems(v: View, sent: readonly string[] = []): View {
  v.items = itemsOf({ git: v.git, router: v.router, sent: new Set(sent) });
  v.counts = countsOf(v.items);
  return v;
}

function liveView(): View {
  return withItems({
    project: { id: 'p0a1b2c3d', name: 'keel' },
    git: {
      branches: [
        { name: 'main', tip: MAIN_TIP, upstream: 'origin/main', remote: 'origin', ahead: 1, behind: 0, gone: false },
        { name: 'epic/E-APPLIST', tip: EPIC_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
        { name: 'wu/E-APPLIST/w5-reader-watcher', tip: WU_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
      ],
      notes: { mr: [], cr: [WU_TIP], dod: [] },
      merged: { main: [], 'epic/E-APPLIST': [] },
      worktrees: [
        { path: '/Users/you/code/keel', branch: 'main', tip: MAIN_TIP },
        { path: '/Users/you/code/keel-w5', branch: 'wu/E-APPLIST/w5-reader-watcher', tip: WU_TIP },
      ],
      open: { epics: ['epic/E-APPLIST'], units: ['wu/E-APPLIST/w5-reader-watcher'], tasks: [] },
      reviews: { [WU_TIP]: { verdict: 'PASS', blockers: 0, importants: 0 } },
      times: { [MAIN_TIP]: '2026-11-22T08:30:00+08:00', [EPIC_TIP]: '2026-11-22T08:10:00+08:00', [WU_TIP]: '2026-11-22T08:41:00+08:00' },
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
    changedAt: '2026-11-22T08:44:01+08:00',
    items: [],
    counts: { ...NO_COUNTS },
  });
}

// Every kind at once: one more epic, four more work units, a task, a worker waiting, a sent plan.
const EPIC2_TIP = 'b2c3d4e5f60718293a4b5c6d7e8f901234567891';
const W6_TIP = '1e2d3c4b5a69788796a5b4c3d2e1f00123456789';
const W8_TIP = '2d3c4b5a69788796a5b4c3d2e1f0012345678901';
const W9_TIP = '3c4b5a69788796a5b4c3d2e1f001234567890123';
const W10_TIP = '4b5a69788796a5b4c3d2e1f00123456789012345';
const TASK_TIP = '5a69788796a5b4c3d2e1f0012345678901234567';

function everyKindView(): View {
  const v = liveView();
  const git = v.git as GitFacts;
  git.branches.find((b) => b.name === 'main')!.ahead = 2;
  git.branches.push(
    { name: 'epic/E-KEELFIX', tip: EPIC2_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
    { name: 'wu/E-APPLIST/w6-list-screens', tip: W6_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
    { name: 'wu/E-APPLIST/w8-window-foundation', tip: W8_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
    { name: 'wu/E-APPLIST/w9-close', tip: W9_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
    { name: 'wu/E-APPLIST/w10-pnpm', tip: W10_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
    { name: 'wu/E-APPLIST/w10-pnpm_lockfile', tip: TASK_TIP, upstream: null, ahead: 0, behind: 0, gone: false },
  );
  git.notes = { mr: [WU_TIP], cr: [EPIC2_TIP, WU_TIP, W6_TIP], dod: [] };
  git.merged = { main: [], 'epic/E-APPLIST': [], 'epic/E-KEELFIX': [], 'wu/E-APPLIST/w10-pnpm': [] };
  // As the reader's openSets lists them from these branches and merged reads.
  git.open = {
    epics: ['epic/E-APPLIST', 'epic/E-KEELFIX'],
    units: ['wu/E-APPLIST/w5-reader-watcher', 'wu/E-APPLIST/w6-list-screens', 'wu/E-APPLIST/w8-window-foundation', 'wu/E-APPLIST/w9-close', 'wu/E-APPLIST/w10-pnpm'],
    tasks: ['wu/E-APPLIST/w10-pnpm_lockfile'],
  };
  git.reviews = {
    [EPIC2_TIP]: { verdict: 'PASS', blockers: 0, importants: 0 },
    [WU_TIP]: { verdict: 'PASS', blockers: 0, importants: 0 },
    [W6_TIP]: { verdict: 'FAIL', blockers: 2, importants: 1 },
  };
  Object.assign(git.times, {
    [EPIC2_TIP]: '2026-11-22T07:50:00+08:00',
    [W6_TIP]: '2026-11-22T08:35:00+08:00',
    [W8_TIP]: '2026-11-22T08:20:00+08:00',
    [W9_TIP]: '2026-11-22T08:05:00+08:00',
    [W10_TIP]: '2026-11-22T08:15:00+08:00',
    [TASK_TIP]: '2026-11-22T08:25:00+08:00',
  });
  Object.assign(v.router.values, {
    'wu/E-APPLIST.W6/stage': { stage: 'in review', tip: '1e2d3c4' },
    'wu/E-APPLIST.W8/stage': { stage: 'plan to read', tip: '2d3c4b5' },
    'wu/E-APPLIST.W9/stage': { stage: 'plan to read', tip: '3c4b5a6' },
    'wu/E-APPLIST.W10/stage': { stage: 'building' },
    'worker/e-applist-w10': { state: 'busy', branch: 'wu/E-APPLIST/w10-pnpm', tip: '4b5a697', dirty: 2, active: '2026-11-22T08:43:00Z' },
    'worker/e-applist-w6-fix': { state: 'waiting', waiting: 'a permission prompt', session: 'b7c1e2d4-5f60-4a1b-9c3d-2e1f00123456', branch: 'wu/E-APPLIST/w6-list-screens', active: '2026-11-22T08:42:00Z' },
  });
  // The W9 plan is marked sent.
  return withItems(v, [itemId('plan', 'E-APPLIST.W9', '3c4b5a6')]);
}

const viewWith = (change: (v: View) => void): View => {
  const v = liveView();
  change(v);
  return withItems(v);
};

export const VIEWS: Readonly<Record<ViewName, View>> = Object.freeze({
  live: liveView(),
  'every-kind': everyKindView(),
  'nothing-waits': viewWith((v) => {
    const git = v.git as GitFacts;
    git.branches.find((b) => b.name === 'main')!.ahead = 0;
    git.notes.cr = [];
    git.reviews = {};
    delete v.router.values['question/merge-order'];
    delete v.router.values['wu/E-APPLIST.W5/stage'];
  }),
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

/** The view a ?view= value names, or null for the scenario's own. */
export function viewOf(value: string | null): ViewName | null {
  return (VIEW_NAMES as readonly string[]).includes(value ?? '') ? (value as ViewName) : null;
}

/** The view with no project listed: nothing read. */
export function noProjectView(): View {
  return {
    project: null,
    git: null,
    router: { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null },
    alerts: [],
    focus: null,
    scheme: 'Keel',
    readAt: null,
    changedAt: null,
    items: [],
    counts: { ...NO_COUNTS },
  };
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
