// Screen 2, Waits on you, at its boundary, the bridge: drawn in jsdom inside the real App against the
// stub bridge with the fixtures' views (drawn by the main part's own item rules). Each group and kind is
// drawn with its words; Open and Mark sent send exactly their request and slots; a push redraws; a click
// on an alert brings its work unit's items forward; a silent router hides its items and says so.
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { expect, test } from 'vitest';
import type { Answer, View } from '../../src/shared/contract.ts';
import { App } from '../../src/window/App.tsx';
import { BridgeProvider } from '../../src/window/bridge.ts';
import { VIEWS } from '../../src/window/fake/fixtures.ts';
import { makeStub, ok, type Handler, type Stub } from '../support/stub-bridge.ts';

function draw(view: View, handlers: Parameters<typeof makeStub>[0] = {}): Stub {
  const stub = makeStub({ 'waiting.get': () => ok(structuredClone(view)), ...handlers });
  render(
    <BridgeProvider bridge={stub.bridge}>
      <App />
    </BridgeProvider>,
  );
  return stub;
}

const section = (name: RegExp) => screen.getByRole('heading', { level: 2, name }).closest('section') as HTMLElement;
const row = (title: string, within_: HTMLElement = document.body) => within(within_).getAllByRole('group', { name: title });
const ready = () => screen.findByRole('heading', { level: 2, name: /^Yours/ });

test('it opens on Waits on you and draws every kind in its group, with each group\'s count in its head and the tab\'s', async () => {
  const view = VIEWS['every-kind'];
  draw(view);
  await ready();
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Waits on you');
  expect(screen.getByRole('button', { name: `Waits on you (${view.counts.yours})` })).toHaveAttribute('aria-current', 'page');
  const yours = section(/^Yours/);
  expect(yours).toHaveTextContent(`Yours (${view.counts.yours})`);
  for (const title of ['Result to merge', 'Epic result to sign', 'Question from the router', 'Plan to read', 'Worker waits on you', 'Task into its work unit', 'Main not pushed']) {
    expect(row(title, yours).length, title).toBeGreaterThan(0);
  }
  const notYours = section(/^Not yours/);
  expect(row('Review failed', notYours)).toHaveLength(1);
  expect(row('Building', notYours)).toHaveLength(1);
  expect(within(notYours).getByText('Its worker: e-applist-w10')).toBeInTheDocument();
  const sent = section(/^Sent/);
  expect(within(row('Plan to read', sent)[0] as HTMLElement).getByRole('button', { name: 'Not sent' })).toBeInTheDocument();
  // A line the owner runs himself is text, never a button.
  expect(screen.getByText('git push origin main')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /push|merge/i })).not.toBeInTheDocument();
});

test('an item says which work unit, branch, tip and since when, and why it waits', async () => {
  draw(VIEWS.live);
  await ready();
  const result = row('Result to sign')[0] as HTMLElement;
  expect(result).toHaveTextContent('E-APPLIST.W5 · wu/E-APPLIST/w5-reader-watcher · tip 0f1e2d3 · since');
  expect(result).toHaveTextContent('The review passed. Your sign-off is missing.');
});

test('nothing waits: each group says so in words', async () => {
  draw(VIEWS['nothing-waits']);
  await ready();
  expect(screen.getByText('Nothing waits on you.')).toBeInTheDocument();
  expect(screen.getByText('No work in motion.')).toBeInTheDocument();
  expect(screen.getByText('Nothing marked sent.')).toBeInTheDocument();
});

test('Open sends item.open with the item\'s id and says what happened; a refusal and a failure show as text', async () => {
  const opened: Array<Record<string, string>> = [];
  const open: Handler = (args) => (opened.push(args), ok({ done: true }));
  const stub = draw(VIEWS.live, { 'item.open': open });
  await ready();
  const result = VIEWS.live.items.find((i) => i.kind === 'result');
  await userEvent.click(within(row('Result to sign')[0] as HTMLElement).getByRole('button', { name: 'Open the result' }));
  expect(opened).toEqual([{ item: result?.id }]);
  expect(await screen.findByText('Opened in your browser.')).toBeInTheDocument();
  stub.answer('item.open', () => ok({ done: false, why: 'its tip moved since the list was drawn' }));
  await userEvent.click(within(row('Result to sign')[0] as HTMLElement).getByRole('button', { name: 'Open the result' }));
  expect(await screen.findByText('Not done: its tip moved since the list was drawn.')).toBeInTheDocument();
  stub.answer('item.open', (): Answer => ({ ok: false, refused: 'bad-slot:item' }));
  await userEvent.click(within(row('Result to sign')[0] as HTMLElement).getByRole('button', { name: 'Open the result' }));
  expect(await screen.findByText('Keel refused the request (bad-slot:item).')).toBeInTheDocument();
});

test('Show the question shows its lines in place and sends nothing; Mark sent and Not sent send item.mark yes and no', async () => {
  const marks: Array<Record<string, string>> = [];
  const stub = draw(VIEWS['every-kind'], { 'item.mark': (args) => (marks.push(args), ok({ done: true })) });
  await ready();
  const q = row('Question from the router')[0] as HTMLElement;
  await userEvent.click(within(q).getByRole('button', { name: 'Show the question' }));
  expect(within(q).getByRole('list', { name: 'The question' })).toHaveTextContent('W6 builds on W5.');
  await userEvent.click(within(q).getByRole('button', { name: 'Hide the question' }));
  expect(within(q).queryByRole('list')).not.toBeInTheDocument();
  expect(stub.names().filter((n) => n.startsWith('item.'))).toEqual([]);
  const plan = VIEWS['every-kind'].items.find((i) => i.kind === 'plan' && i.group === 'yours');
  const sentPlan = VIEWS['every-kind'].items.find((i) => i.group === 'sent');
  await userEvent.click(within(row('Plan to read', section(/^Yours/))[0] as HTMLElement).getByRole('button', { name: 'Mark sent' }));
  expect(await screen.findByText('Marked sent: it waits for a sign under Sent.')).toBeInTheDocument();
  await userEvent.click(within(row('Plan to read', section(/^Sent/))[0] as HTMLElement).getByRole('button', { name: 'Not sent' }));
  expect(await screen.findByText('Back on your list.')).toBeInTheDocument();
  expect(marks).toEqual([{ item: plan?.id, sent: 'yes' }, { item: sentPlan?.id, sent: 'no' }]);
});

test('a waiting.changed push causes exactly one new waiting.get, and the new list is drawn', async () => {
  let view = VIEWS.live;
  const stub = draw(view, { 'waiting.get': () => ok(structuredClone(view)) });
  await ready();
  const before = stub.names().filter((n) => n === 'waiting.get').length;
  view = VIEWS['nothing-waits'];
  act(() => void stub.push('waiting.changed'));
  expect(await screen.findByText('Nothing waits on you.')).toBeInTheDocument();
  expect(stub.names().filter((n) => n === 'waiting.get').length).toBe(before + 1);
});

test('a click on an alert brings Waits on you forward with that work unit\'s items marked; a work unit with none gets a line; a newer click acts again', async () => {
  let view: View = { ...structuredClone(VIEWS.live), focus: null };
  const stub = draw(view, { 'waiting.get': () => ok(structuredClone(view)) });
  await ready();
  await userEvent.click(screen.getByRole('button', { name: 'Projects' }));
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Projects');
  view = { ...view, focus: { wu: 'E-APPLIST.W5', seq: 1 } };
  act(() => void stub.push('waiting.changed'));
  await waitFor(() => expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Waits on you'));
  expect(within(row('Result to sign')[0] as HTMLElement).getByText('From your alert')).toBeInTheDocument();
  expect(within(row('Main not pushed')[0] as HTMLElement).queryByText('From your alert')).not.toBeInTheDocument();
  view = { ...view, focus: { wu: 'E-APPLIST.W9', seq: 2 } };
  act(() => void stub.push('waiting.changed'));
  expect(await screen.findByText('Nothing of E-APPLIST.W9 waits on you.')).toBeInTheDocument();
  expect(screen.queryByText('From your alert')).not.toBeInTheDocument();
});

test('a launch by a link (a focus already set) opens on that work unit', async () => {
  draw(VIEWS.focus);
  await ready();
  expect(within(row('Result to sign')[0] as HTMLElement).getByText('From your alert')).toBeInTheDocument();
});

test('a silent router: a line says its items are hidden and what git shows is listed; with no run it says the router has not started', async () => {
  draw(VIEWS['not-heard']);
  await ready();
  expect(screen.getByRole('status')).toHaveTextContent('The router is not heard: plans to read, questions, workers waiting and work in motion are hidden until it is.');
  expect(screen.queryByRole('group', { name: 'Question from the router' })).not.toBeInTheDocument();
  expect(row('Result to sign')).toHaveLength(1);
  cleanup();
  draw(VIEWS['no-run']);
  await ready();
  expect(screen.getByRole('status')).toHaveTextContent('The router has not started here');
});

test('a value holding markup is shown as its own characters, never as markup', async () => {
  const view = structuredClone(VIEWS.live);
  const item = view.items[0];
  if (item !== undefined) item.why = '<img src=x onerror=alert(1)><b>bold</b>';
  draw(view);
  await ready();
  expect(screen.getByText('<img src=x onerror=alert(1)><b>bold</b>')).toBeInTheDocument();
  expect(document.querySelector('img, b')).toBeNull();
});

test('a waiting.get that fails says so as text; with no project the screen points to Projects', async () => {
  draw(VIEWS.live, { 'waiting.get': (): Answer => ({ ok: false, failed: 'the reader stopped' }) });
  expect(await screen.findByText('Keel could not read the list. The request failed: the reader stopped.')).toBeInTheDocument();
  cleanup();
  const none = structuredClone(VIEWS.live);
  none.project = null;
  draw(none);
  expect(await screen.findByText('No project is listed. Add one on the Projects screen.')).toBeInTheDocument();
});
