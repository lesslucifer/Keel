// One item on screen 2 (E-APPLIST.W6): what it is, which work unit or epic, its tip and since when, why
// it waits, who acts, a line the owner may run himself as text, and its actions: Open (a page, an epic
// result, or a question's text shown in place) and Mark sent or Not sent. Every value is React text.
// An item a click on an alert pointed at carries a badge saying so. Buttons are outline pills: a card
// of items holds no solid one (mx3a: one at most per card, and here each row is equal).
import { useState } from 'react';
import type { Item } from '../../shared/contract.ts';
import { sinceText, type ActionView } from '../hooks.ts';
import { Badge } from './Badge.tsx';
import { Button } from './Button.tsx';
import { Path } from './Path.tsx';
import { Row } from './Row.tsx';

const OPEN_WORDS: Record<Item['kind'], string> = {
  result: 'Open the result',
  'epic-result': 'Open the epic result',
  plan: 'Open the plan',
  question: 'Show the question',
  worker: 'Open',
  task: 'Open',
  push: 'Open',
  motion: 'Open',
};

/** What it is about, in one line: the work unit or epic, the branch, the tip, since when. */
function meta(item: Item): string {
  const parts = [item.wu ?? item.epic, item.branch, item.tip === null ? null : `tip ${item.tip}`, item.since === null ? null : sinceText(item.since)];
  return parts.filter((p): p is string => p !== null && p !== '').join(' · ');
}

export function ItemRow({ item, action, focused }: { item: Item; action: ActionView; focused: boolean }) {
  const { busy, run } = action;
  const [shown, setShown] = useState(false);
  const actions = (
    <>
      {item.open === 'page' || item.open === 'archive' ? (
        <Button disabled={busy} onClick={() => void run({ name: 'item.open', args: { item: item.id } })}>
          {OPEN_WORDS[item.kind]}
        </Button>
      ) : null}
      {item.open === 'text' ? (
        <Button disabled={false} onClick={() => setShown((s) => !s)}>
          {shown ? 'Hide the question' : 'Show the question'}
        </Button>
      ) : null}
      {item.markable && item.group !== 'sent' ? (
        <Button disabled={busy} onClick={() => void run({ name: 'item.mark', args: { item: item.id, sent: 'yes' } })}>
          Mark sent
        </Button>
      ) : null}
      {item.markable && item.group === 'sent' ? (
        <Button disabled={busy} onClick={() => void run({ name: 'item.mark', args: { item: item.id, sent: 'no' } })}>
          Not sent
        </Button>
      ) : null}
    </>
  );
  const info = meta(item);
  return (
    <Row label={item.title} actions={actions}>
      {focused ? (
        <div className="flex">
          <Badge tone="set">From your alert</Badge>
        </div>
      ) : null}
      {info === '' ? null : <span className="text-ui-sm text-ink-secondary">{info}</span>}
      <span className="text-ink">{item.why}</span>
      {item.worker === null ? null : <span className="text-ui-sm text-ink-secondary">{`Its worker: ${item.worker}`}</span>}
      {item.line === null ? null : <Path path={item.line} />}
      {item.open === 'text' && shown ? (
        <ul aria-label="The question" className="m-0 flex flex-col gap-half pl-xl">
          {item.text.map((line, i) => (
            <li key={`${i}:${line}`} className="text-ink">
              {line}
            </li>
          ))}
        </ul>
      ) : null}
    </Row>
  );
}
