// The fake's fixtures: every scenario the browser mode names exists, and each one's AppState has
// the shapes the contract gives state.get, so the screens are drawn from data the main part could send.
import { expect, test } from 'vitest';
import { NOTE_KINDS, PROJECT_ID_SOURCE, TOOL_NAMES, type AppState, type View } from '../../src/shared/contract.ts';
import { SCENARIOS, SCENARIO_NAMES, VIEWS, VIEW_NAMES, scenarioOf } from '../../src/window/fake/fixtures.ts';

/** Every way `s` departs from the contract's AppState, in words. */
function problems(s: AppState): string[] {
  const out: string[] = [];
  const str = (v: unknown) => typeof v === 'string';
  if (!str(s.version) || s.version === '') out.push('version');
  if (!Array.isArray(s.path.entries) || !s.path.entries.every((e) => str(e) && e.startsWith('/'))) out.push('path.entries');
  if (!['login-shell', 'fallback'].includes(s.path.source)) out.push('path.source');
  if ((s.path.source === 'login-shell') !== (s.path.reason === null)) out.push('path.reason');
  if (JSON.stringify(Object.keys(s.tools).sort()) !== JSON.stringify([...TOOL_NAMES].sort())) out.push('tools');
  for (const tool of TOOL_NAMES) {
    const t = s.tools[tool];
    if (!['set', 'path', 'missing'].includes(t.source)) out.push(`${tool}.source`);
    if ((t.source === 'missing') !== (t.path === null)) out.push(`${tool}.path`);
    if (t.path !== null && !t.path.startsWith('/')) out.push(`${tool}.path absolute`);
    if (t.problem !== null && !str(t.problem)) out.push(`${tool}.problem`);
    if (t.source === 'missing' && t.problem === null) out.push(`${tool}.problem says why`);
  }
  if (s.projects.length > 1) out.push('one project at most');
  for (const p of s.projects) {
    if (!new RegExp(PROJECT_ID_SOURCE).test(p.id)) out.push(`project id ${p.id}`);
    if (!p.path.startsWith('/') || p.name === '') out.push(`project ${p.id}`);
  }
  return out;
}

test('the scenarios are exactly the seven the browser mode offers', () => {
  expect([...SCENARIO_NAMES]).toEqual(['one-project', 'no-project', 'tool-missing', 'path-fallback', 'refusing', 'failing', 'slow']);
  expect(Object.keys(SCENARIOS).sort()).toEqual([...SCENARIO_NAMES].sort());
});

test('each scenario\'s state passes the contract\'s shapes', () => {
  for (const name of SCENARIO_NAMES) expect(problems(SCENARIOS[name].state), name).toEqual([]);
});

test('each scenario says what it shows: no-project has none, tool-missing a missing tool, path-fallback the fallback and why, slow a delay', () => {
  expect(SCENARIOS['one-project'].state.projects).toHaveLength(1);
  expect(SCENARIOS['no-project'].state.projects).toHaveLength(0);
  expect(Object.values(SCENARIOS['tool-missing'].state.tools).some((t) => t.source === 'missing')).toBe(true);
  expect(SCENARIOS['path-fallback'].state.path).toMatchObject({ source: 'fallback', reason: expect.stringMatching(/\S/) });
  expect(SCENARIOS.refusing.answers).toBe('refuse');
  expect(SCENARIOS.failing.answers).toBe('fail');
  expect(SCENARIOS.slow.delayMs).toBe(800);
  expect(SCENARIOS['one-project'].delayMs).toBe(0);
});

test('the tools\' paths are shaped like a real Mac\'s', () => {
  expect(SCENARIOS['one-project'].state.tools.git.path).toBe('/opt/homebrew/bin/git');
});

test('scenarioOf reads a name, and anything else (none, unknown, markup) is the default one-project', () => {
  expect(scenarioOf('no-project')).toBe('no-project');
  expect(scenarioOf(null)).toBe('one-project');
  expect(scenarioOf('nope')).toBe('one-project');
  expect(scenarioOf('<b>x</b>')).toBe('one-project');
  expect(scenarioOf('constructor')).toBe('one-project');
});

const WU = /^E-[A-Z0-9]{1,16}\.W[0-9]{1,3}$/;
const SHA = /^[0-9a-f]{40}$/;

/** Every way `v` departs from the contract's View, in words. */
function viewProblems(v: View): string[] {
  const out: string[] = [];
  const iso = (s: unknown) => typeof s === 'string' && !Number.isNaN(Date.parse(s));
  if (v.project !== null && (!new RegExp(PROJECT_ID_SOURCE).test(v.project.id) || v.project.name === '')) out.push('project');
  if (v.project === null && v.git !== null) out.push('git without a project');
  if (v.git !== null) {
    for (const b of v.git.branches) {
      if (b.name === '' || b.name.startsWith('refs/') || !SHA.test(b.tip)) out.push(`branch ${b.name}`);
      if (![b.ahead, b.behind].every((x) => Number.isInteger(x) && x >= 0)) out.push(`branch ${b.name} counts`);
    }
    if (JSON.stringify(Object.keys(v.git.notes).sort()) !== JSON.stringify([...NOTE_KINDS].sort())) out.push('notes kinds');
    for (const k of NOTE_KINDS) if (!v.git.notes[k].every((t) => SHA.test(t))) out.push(`notes ${k}`);
    const names = new Set(v.git.branches.map((b) => b.name));
    for (const [target, held] of Object.entries(v.git.merged)) if (!names.has(target) || !held.every((h) => names.has(h))) out.push(`merged ${target}`);
    for (const w of v.git.worktrees) if (!w.path.startsWith('/') || (w.tip !== null && !SHA.test(w.tip))) out.push(`worktree ${w.path}`);
  }
  const r = v.router;
  if ((r.run === null) !== (Object.keys(r.values).length === 0 && r.n === 0)) out.push('router run');
  if (r.lastAt !== null && !iso(r.lastAt)) out.push('router lastAt');
  if (r.heard && r.lastAt === null) out.push('heard with no line');
  for (const a of v.alerts) {
    if (!Number.isInteger(a.n) || a.n < 1 || !iso(a.at) || a.headline === '' || a.lines.length < 1 || a.lines.length > 2) out.push(`alert ${a.n}`);
    if (a.wu !== null && !WU.test(a.wu)) out.push(`alert ${a.n} wu`);
  }
  if (v.focus !== null && ((v.focus.wu !== null && !WU.test(v.focus.wu)) || !Number.isInteger(v.focus.seq))) out.push('focus');
  return out;
}

test('the views are exactly the five the fixtures name, and each passes the contract\'s shapes', () => {
  expect([...VIEW_NAMES]).toEqual(['live', 'no-run', 'not-heard', 'after-restart', 'focus']);
  expect(Object.keys(VIEWS).sort()).toEqual([...VIEW_NAMES].sort());
  for (const name of VIEW_NAMES) expect(viewProblems(VIEWS[name]), name).toEqual([]);
});

test('each view says what it shows: live is heard with stages and an alert, no-run has no run, not-heard is silent, after-restart a run with no values, focus a work unit', () => {
  expect(VIEWS.live.router).toMatchObject({ heard: true, run: expect.stringMatching(/\S/) });
  expect(Object.keys(VIEWS.live.router.values).some((k) => k.endsWith('/stage'))).toBe(true);
  expect(VIEWS.live.alerts.length).toBeGreaterThan(0);
  expect(VIEWS['no-run'].router.run).toBeNull();
  expect(VIEWS['not-heard'].router.heard).toBe(false);
  expect(VIEWS['not-heard'].router.lastAt).not.toBeNull();
  expect(VIEWS['after-restart'].router.values).toEqual({});
  expect(VIEWS['after-restart'].router.run).not.toBeNull();
  expect(VIEWS.focus.focus).toMatchObject({ wu: expect.stringMatching(WU) });
});

test('each scenario carries a view: no-project the view with no project, every other the live view', () => {
  for (const name of SCENARIO_NAMES) {
    if (name === 'no-project') expect(SCENARIOS[name].view.project, name).toBeNull();
    else expect(SCENARIOS[name].view, name).toEqual(VIEWS.live);
  }
});
