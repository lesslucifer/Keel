// The fake bridge's fixtures: named scenarios the browser mode picks with ?state=, and the tests draw
// from. Each state has the contract's shapes (test-window/unit/fixtures.test.ts) and paths shaped like
// a real Mac's. Never part of the shipped page: only browser.tsx and the window's tests import fake/.

import type { AppState, GitFacts, Item, ItemCounts, Tool, ToolState, View } from '../../shared/contract.ts';

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

export const VIEW_NAMES = Object.freeze(['live', 'every-kind', 'nothing-waits', 'no-run', 'not-heard', 'after-restart', 'focus'] as const);
export type ViewName = (typeof VIEW_NAMES)[number];

const RUN = 'keel-1c.20261122-083000.4242';
const EPIC_TIP = 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678';
const WU_TIP = '0f1e2d3c4b5a69788796a5b4c3d2e1f001234567';
const MAIN_TIP = '9a8b7c6d5e4f30211203f4e5d6c7b8a998765432';

const NO_COUNTS = Object.freeze({ yours: 0, notYours: 0, sent: 0 });

/** An item as the main part lists it; the fields a fixture leaves out take these values. */
export function fixtureItem(over: Partial<Item> & Pick<Item, 'id' | 'kind' | 'title' | 'why'>): Item {
  return { group: 'yours', wu: null, epic: null, branch: null, tip: null, since: null, open: 'none', markable: false, worker: null, line: null, text: [], ...over };
}

/** The counts a list holds. */
export function countsOf(items: readonly Item[]): ItemCounts {
  return { yours: items.filter((i) => i.group === 'yours').length, notYours: items.filter((i) => i.group === 'not-yours').length, sent: items.filter((i) => i.group === 'sent').length };
}

function liveView(): View {
  const items: Item[] = [
    fixtureItem({ id: 'result/E-APPLIST.W5@0f1e2d3c4b5a', kind: 'result', title: 'Result to sign', wu: 'E-APPLIST.W5', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w5-reader-watcher', tip: '0f1e2d3', since: '2026-11-22T08:41:00+08:00', why: 'The review passed. Your sign-off is missing.', open: 'page' }),
    fixtureItem({ id: 'question/merge-order@5d41402a', kind: 'question', title: 'Question from the router', wu: 'E-APPLIST.W5', since: '2026-11-22T08:44:00+08:00', why: 'Merge W5 before W6?', open: 'text', markable: true, text: ['W6 builds on W5.'] }),
    fixtureItem({ id: 'push/main@9a8b7c6d5e4f', kind: 'push', title: 'Main not pushed', branch: 'main', tip: '9a8b7c6', since: '2026-11-22T08:30:00+08:00', why: 'main is 1 commit ahead of origin/main.', line: 'git push origin main' }),
  ];
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
    items,
    counts: countsOf(items),
  };
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
  const items: Item[] = [
    fixtureItem({ id: 'result/E-APPLIST.W5@0f1e2d3c4b5a', kind: 'result', title: 'Result to merge', wu: 'E-APPLIST.W5', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w5-reader-watcher', tip: '0f1e2d3', since: '2026-11-22T08:41:00+08:00', why: 'Signed. The merge into epic/E-APPLIST is yours, in your terminal.', open: 'page', line: 'git merge --no-ff --no-edit wu/E-APPLIST/w5-reader-watcher' }),
    fixtureItem({ id: 'epic-result/E-KEELFIX@b2c3d4e5f607', kind: 'epic-result', title: 'Epic result to sign', epic: 'E-KEELFIX', branch: 'epic/E-KEELFIX', tip: 'b2c3d4e', since: '2026-11-22T07:50:00+08:00', why: 'The review passed. Your sign-off is missing.', open: 'archive' }),
    ...v.items.filter((i) => i.kind === 'question'),
    fixtureItem({ id: 'plan/E-APPLIST.W8@2d3c4b5', kind: 'plan', title: 'Plan to read', wu: 'E-APPLIST.W8', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w8-window-foundation', tip: '2d3c4b5', since: '2026-11-22T08:20:00+08:00', why: 'The router waits for your word on the plan.', open: 'page', markable: true }),
    fixtureItem({ id: 'worker/e-applist-w6-fix@7c1d9e2a', kind: 'worker', title: 'Worker waits on you', wu: 'E-APPLIST.W6', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w6-list-screens', since: '2026-11-22T08:42:00Z', why: 'Waits on a permission prompt: answer it in its session.', worker: 'e-applist-w6-fix', line: 'claude attach b7c1e2d4-5f60-4a1b-9c3d-2e1f00123456' }),
    fixtureItem({ id: 'task/9f3a1c2e@5a6978879', kind: 'task', title: 'Task into its work unit', wu: 'E-APPLIST.W10', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w10-pnpm_lockfile', tip: '5a69788', since: '2026-11-22T08:25:00+08:00', why: 'No worker is busy on it. The merge into wu/E-APPLIST/w10-pnpm is yours, in its folder.', line: 'git merge --no-ff wu/E-APPLIST/w10-pnpm_lockfile' }),
    fixtureItem({ ...(v.items.find((i) => i.kind === 'push') as Item), why: 'main is 2 commits ahead of origin/main.' }),
    fixtureItem({ id: 'result/E-APPLIST.W6@1e2d3c4b5a69', kind: 'result', group: 'not-yours', title: 'Review failed', wu: 'E-APPLIST.W6', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w6-list-screens', tip: '1e2d3c4', since: '2026-11-22T08:35:00+08:00', why: '2 blockers and 1 important open: its worker fixes them.', worker: 'e-applist-w6-fix' }),
    fixtureItem({ id: 'motion/E-APPLIST.W10@1b2c3d4e', kind: 'motion', group: 'not-yours', title: 'Building', wu: 'E-APPLIST.W10', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w10-pnpm', tip: '4b5a697', since: '2026-11-22T08:15:00+08:00', why: 'Its worker builds it.', worker: 'e-applist-w10' }),
    fixtureItem({ id: 'plan/E-APPLIST.W9@3c4b5a6', kind: 'plan', group: 'sent', title: 'Plan to read', wu: 'E-APPLIST.W9', epic: 'E-APPLIST', branch: 'wu/E-APPLIST/w9-close', tip: '3c4b5a6', since: '2026-11-22T08:05:00+08:00', why: 'You marked it sent: it leaves when the router moves the stage on.', open: 'page', markable: true }),
  ];
  v.items = items;
  v.counts = countsOf(items);
  return v;
}

const viewWith = (change: (v: View) => void): View => {
  const v = liveView();
  change(v);
  return v;
};

/** The router-told items (plan, question, worker, motion) leave the list when the router is not heard. */
const persistentOnly = (v: View) => {
  v.items = v.items.filter((i) => i.kind !== 'question' && i.kind !== 'plan' && i.kind !== 'worker' && i.kind !== 'motion');
  v.counts = countsOf(v.items);
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
    v.items = [];
    v.counts = { ...NO_COUNTS };
  }),
  'no-run': viewWith((v) => {
    v.router = { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null };
    v.alerts = [];
    persistentOnly(v);
  }),
  'not-heard': viewWith((v) => {
    v.router.heard = false;
    v.router.lastAt = '2026-11-22T08:20:00+08:00';
    persistentOnly(v);
  }),
  'after-restart': viewWith((v) => {
    v.router = { run: 'keel-1c.20261122-090000.4343', n: 1, values: {}, heard: true, lastAt: '2026-11-22T09:00:00+08:00', problem: null };
    v.alerts = [];
    persistentOnly(v);
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
