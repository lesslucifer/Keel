// The fake bridge at its boundary, the KeelBridge interface the window asks through: it answers every
// request in the contract and lists every push event (iterated from contract.ts, so a name the
// contract gains fails here until the fake has it), each change pushes state.changed once and the
// next state.get shows it, and it refuses what the main part refuses.
import { expect, test } from 'vitest';
import { PUSH_EVENTS, REQUESTS, SLOT_CAP, type Answer, type AppState, type RequestName, type Slot } from '../../src/shared/contract.ts';
import { makeFakeBridge } from '../../src/window/fake/fake.ts';
import { SCENARIO_NAMES } from '../../src/window/fake/fixtures.ts';

const now = () => Promise.resolve();

/** Arguments every slot of `name` accepts, built from the contract's table. */
function validArgs(name: RequestName, projectId: string): Record<string, string> {
  return Object.fromEntries(Object.entries(REQUESTS[name]).map(([k, slot]: [string, Slot]) => [k, slot.kind === 'enum' ? (slot.values[0] as string) : projectId]));
}

async function stateOf(bridge: ReturnType<typeof makeFakeBridge>): Promise<AppState> {
  const a = (await bridge.ask('state.get', {})) as Answer<AppState>;
  if (!a.ok) throw new Error('state.get did not answer');
  return a.value;
}

test('the fake answers every request name in the contract with a value, never unknown-name', async () => {
  const names = Object.keys(REQUESTS) as RequestName[];
  expect(names.length).toBeGreaterThan(0);
  for (const name of names) {
    const bridge = makeFakeBridge('one-project', { wait: now });
    const id = (await stateOf(bridge)).projects[0]?.id ?? 'p00000000';
    const a = (await bridge.ask(name, validArgs(name, id))) as Answer;
    expect(a, name).toMatchObject({ ok: true });
  }
});

test('the fake lists every push event in the contract and refuses any other', () => {
  const bridge = makeFakeBridge('one-project', { wait: now });
  for (const event of Object.keys(PUSH_EVENTS)) expect(() => bridge.on(event, () => undefined)(), event).not.toThrow();
  expect(() => bridge.on('state.changed.extra', () => undefined)).toThrow(/not a push event/);
});

test('each change pushes state.changed exactly once and the next state.get shows it', async () => {
  const bridge = makeFakeBridge('no-project', { wait: now });
  let pushes = 0;
  bridge.on('state.changed', () => {
    pushes += 1;
  });

  expect(await bridge.ask('projects.add', {})).toEqual({ ok: true, value: { done: true } });
  expect(pushes).toBe(1);
  const added = (await stateOf(bridge)).projects;
  expect(added).toHaveLength(1);

  expect(await bridge.ask('settings.choose', { tool: 'claude' })).toEqual({ ok: true, value: { done: true } });
  expect(pushes).toBe(2);
  expect((await stateOf(bridge)).tools.claude).toMatchObject({ source: 'set', path: expect.stringMatching(/^\/.*claude$/), problem: null });

  expect(await bridge.ask('settings.reset', { tool: 'claude' })).toEqual({ ok: true, value: { done: true } });
  expect(pushes).toBe(3);
  expect((await stateOf(bridge)).tools.claude.source).not.toBe('set');

  expect(await bridge.ask('projects.remove', { project: added[0]?.id ?? '' })).toEqual({ ok: true, value: { done: true } });
  expect(pushes).toBe(4);
  expect((await stateOf(bridge)).projects).toEqual([]);

  await bridge.ask('state.get', {});
  await bridge.ask('alert.test', {});
  expect(pushes).toBe(4);
});

test('a change that is not done says why and pushes nothing: a second project, an unknown project', async () => {
  const bridge = makeFakeBridge('one-project', { wait: now });
  let pushes = 0;
  bridge.on('state.changed', () => {
    pushes += 1;
  });
  expect(await bridge.ask('projects.add', {})).toEqual({ ok: true, value: { done: false, why: expect.stringMatching(/one project/) } });
  expect(await bridge.ask('projects.remove', { project: 'pffffffff' })).toEqual({ ok: true, value: { done: false, why: expect.stringMatching(/\S/) } });
  expect(pushes).toBe(0);
});

test('state.get hands out a copy: changing it does not change the fake', async () => {
  const bridge = makeFakeBridge('one-project', { wait: now });
  const first = await stateOf(bridge);
  first.projects.length = 0;
  expect((await stateOf(bridge)).projects).toHaveLength(1);
});

test('the refusing scenario refuses each name; the failing one reads state and fails each other request', async () => {
  for (const name of Object.keys(REQUESTS) as RequestName[]) {
    expect(await makeFakeBridge('refusing', { wait: now }).ask(name, validArgs(name, 'p0a1b2c3d')), name).toMatchObject({ ok: false, refused: expect.any(String) });
    const failing = makeFakeBridge('failing', { wait: now });
    const a = await failing.ask(name, validArgs(name, 'p0a1b2c3d'));
    if (name === 'state.get') expect(a, name).toMatchObject({ ok: true });
    else expect(a, name).toMatchObject({ ok: false, failed: expect.any(String) });
  }
});

test('a malformed call is refused like the main part refuses it, and nothing changes', async () => {
  const bridge = makeFakeBridge('one-project', { wait: now });
  let pushes = 0;
  bridge.on('state.changed', () => {
    pushes += 1;
  });
  const cases: Array<[string, unknown, string]> = [
    ['projects.rename', {}, 'unknown-name'],
    ['toString', {}, 'unknown-name'],
    ['state.get', { extra: 'x' }, 'bad-args'],
    ['projects.remove', {}, 'bad-args'],
    ['settings.choose', { tool: 'git', extra: 'x' }, 'bad-args'],
    ['settings.choose', null, 'bad-args'],
    ['settings.choose', ['git'], 'bad-args'],
    ['settings.choose', { tool: 'npm' }, 'bad-slot:tool'],
    ['settings.choose', { tool: 7 }, 'bad-slot:tool'],
    ['projects.remove', { project: '/Users/owner/code/keel' }, 'bad-slot:project'],
    ['projects.remove', { project: 'p'.repeat(SLOT_CAP + 1) }, 'bad-slot:project'],
  ];
  for (const [name, args, refused] of cases) {
    expect(await bridge.ask(name, args as Record<string, string>), `${name} ${JSON.stringify(args)}`).toEqual({ ok: false, refused });
  }
  expect(pushes).toBe(0);
  expect((await stateOf(bridge)).projects).toHaveLength(1);
});

test('every scenario answers state.get through its wait, slow after 800 ms', async () => {
  const waited: number[] = [];
  for (const name of SCENARIO_NAMES) {
    const bridge = makeFakeBridge(name, { wait: (ms) => (waited.push(ms), Promise.resolve()) });
    await bridge.ask('state.get', {});
  }
  expect(waited).toEqual(SCENARIO_NAMES.map((n) => (n === 'slow' ? 800 : 0)));
});
