// Screen 2, Waits on you (docs/local-app.html 'The screens'; E-APPLIST.W6): the one listed project's
// items in three groups - yours, not yours (a worker acts), sent (waiting for a sign) - as the main part
// draws them (src/shared/items.ts). When the router is not heard, a line says so and that its items are
// hidden. A click on an alert points the list at a work unit: its items carry a badge and the first is
// brought into view; with none, a line says nothing of it waits on you. Every value is React text.
import { useEffect, useRef } from 'react';
import type { Item, ItemGroup, View } from '../../shared/contract.ts';
import type { ActionView } from '../hooks.ts';
import { ItemRow } from '../ui/ItemRow.tsx';
import { Row } from '../ui/Row.tsx';
import { Section } from '../ui/Section.tsx';

const GROUPS: Array<{ group: ItemGroup; title: string; empty: string }> = [
  { group: 'yours', title: 'Yours', empty: 'Nothing waits on you.' },
  { group: 'not-yours', title: 'Not yours: a worker acts', empty: 'No work in motion.' },
  { group: 'sent', title: 'Sent, waiting for a sign', empty: 'Nothing marked sent.' },
];

/** The router's standing in words, when the list cannot show its items; null while it is heard. */
function routerLine(view: View): string | null {
  if (view.project === null || view.router.heard) return null;
  if (view.router.run === null) return 'The router has not started here: plans to read, questions, workers waiting and work in motion are hidden. What git shows is listed.';
  return 'The router is not heard: plans to read, questions, workers waiting and work in motion are hidden until it is. What git shows is listed.';
}

export function Waits({ view, error, action, focus }: { view: View | null; error: string; action: ActionView; focus: { wu: string | null; seq: number } | null }) {
  const first = useRef<HTMLDivElement | null>(null);
  const focusedWu = focus?.wu ?? null;
  const firstFocused = view?.items.find((i) => focusedWu !== null && i.wu === focusedWu)?.id ?? null;

  useEffect(() => {
    if (focus !== null) first.current?.scrollIntoView?.({ block: 'nearest' });
  }, [focus, firstFocused]);

  if (view === null) {
    return error === '' ? (
      <p className="m-0 text-ink-secondary">Loading…</p>
    ) : (
      <p role="alert" className="m-0 font-medium text-problem">
        {`Keel could not read the list. ${error}`}
      </p>
    );
  }
  if (view.project === null) return <p className="m-0 text-ink-secondary">No project is listed. Add one on the Projects screen.</p>;

  const line = routerLine(view);
  const row = (item: Item) => {
    const focused = focusedWu !== null && item.wu === focusedWu;
    return (
      <div key={item.id} ref={item.id === firstFocused ? first : undefined}>
        <ItemRow item={item} action={action} focused={focused} />
      </div>
    );
  };
  return (
    <div className="flex flex-col gap-xxl">
      {line === null ? null : (
        <p role="status" className="m-0 font-medium text-problem">
          {line}
        </p>
      )}
      {view.git?.problem == null ? null : (
        <p role="alert" className="m-0 font-medium text-problem">
          {`Git: ${view.git.problem}`}
        </p>
      )}
      {focusedWu !== null && firstFocused === null ? <p className="m-0 text-ink">{`Nothing of ${focusedWu} waits on you.`}</p> : null}
      {GROUPS.map(({ group, title, empty }) => {
        const items = view.items.filter((i) => i.group === group);
        return (
          <Section key={group} title={`${title} (${items.length})`}>
            {items.length === 0 ? (
              <Row>
                <span className="text-ink-secondary">{empty}</span>
              </Row>
            ) : (
              items.map(row)
            )}
          </Section>
        );
      })}
      <div className="flex flex-col gap-half px-half">
        {action.note === '' ? null : (
          <p role="status" className="m-0 text-ink">
            {action.note}
          </p>
        )}
        {error === '' ? null : (
          <p role="alert" className="m-0 font-medium text-problem">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
