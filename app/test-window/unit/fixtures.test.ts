// The fake's fixtures: every scenario the browser mode names exists, and each one's AppState has
// the shapes the contract gives state.get, so the screens are drawn from data the main part could send.
import { expect, test } from 'vitest';
import { PROJECT_ID_SOURCE, TOOL_NAMES, type AppState } from '../../src/shared/contract.ts';
import { SCENARIOS, SCENARIO_NAMES, scenarioOf } from '../../src/window/fake/fixtures.ts';

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
