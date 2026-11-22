// Screen 6, Projects (docs/local-app.html 'The screens'): the one project with Remove, or Add when
// there is none; keel, git and claude each with its path, its source, its problem, Choose and, when
// set in Keel, Use the PATH; where the PATH came from; the test alert; the version at the foot. W6: the
// project's list field (6.4, the counts, each a way to Waits on you; 6.5, when Keel last saw a change). Each
// control sends one request through useAction; while one runs every control is off. Every value is
// React text, never markup. The look is mx3a (the owner's choice, 2026-11-22): white cards on
// hairlines, one solid black pill at most per card, every other action an outline pill, states in
// words on a badge whose colour only repeats them, problems in words in the problem colour.

import type { AppState, Tool, ToolState, View } from '../../shared/contract.ts';
import { TOOL_NAMES } from '../../shared/contract.ts';
import { sinceText, type ActionView } from '../hooks.ts';
import { Badge, type Tone } from '../ui/Badge.tsx';
import { Button } from '../ui/Button.tsx';
import { Path } from '../ui/Path.tsx';
import { Row } from '../ui/Row.tsx';
import { Section } from '../ui/Section.tsx';

const SOURCE: Record<ToolState['source'], { words: string; tone: Tone }> = {
  set: { words: 'set in Keel', tone: 'set' },
  path: { words: 'found on the PATH', tone: 'found' },
  missing: { words: 'missing', tone: 'missing' },
};

function ToolRow({ tool, t, action }: { tool: Tool; t: ToolState; action: ActionView }) {
  const { busy, run } = action;
  return (
    <Row
      label={tool}
      actions={
        <>
          <Button disabled={busy} onClick={() => void run({ name: 'settings.choose', args: { tool } })}>
            Choose…
          </Button>
          {t.source === 'set' ? (
            <Button disabled={busy} onClick={() => void run({ name: 'settings.reset', args: { tool } })}>
              Use the PATH
            </Button>
          ) : null}
        </>
      }
    >
      <div className="flex flex-wrap items-center gap-xs">
        <Badge tone={SOURCE[t.source].tone}>{SOURCE[t.source].words}</Badge>
        {t.path === null ? <span className="text-ui-sm text-ink-secondary">not found</span> : <Path path={t.path} />}
      </div>
      {t.problem === null ? null : <span className="font-medium text-problem">{t.problem}</span>}
    </Row>
  );
}

/** Fields 6.4 and 6.5: how many wait on you, are a worker's and are sent, each a way to the list; and when Keel last saw a change. */
function ListField({ waiting, onShowWaits }: { waiting: View | null; onShowWaits: () => void }) {
  const counts = waiting === null || waiting.project === null ? null : waiting.counts;
  const last = waiting?.changedAt == null ? '' : sinceText(waiting.changedAt).replace(/^since /, '');
  return (
    <Row label="The list">
      {counts === null ? (
        <span className="text-ink-secondary">Not read yet.</span>
      ) : (
        <div className="flex flex-wrap gap-xs">
          <Button disabled={false} onClick={onShowWaits}>{`${counts.yours} wait on you`}</Button>
          <Button disabled={false} onClick={onShowWaits}>{`${counts.notYours} not yours`}</Button>
          <Button disabled={false} onClick={onShowWaits}>{`${counts.sent} sent`}</Button>
        </div>
      )}
      {last === '' ? null : <span className="text-ui-sm text-ink-secondary">{`Last change seen ${last}`}</span>}
    </Row>
  );
}

export function Projects({ state, action, error, waiting = null, onShowWaits = () => undefined }: { state: AppState; action: ActionView; error: string; waiting?: View | null; onShowWaits?: () => void }) {
  const { busy, run, note } = action;
  const project = state.projects[0];
  return (
    <div className="flex flex-col gap-xxl">
      <Section title="Project">
        {project === undefined ? (
          <Row
            actions={
              <Button kind="primary" disabled={busy} onClick={() => void run({ name: 'projects.add', args: {} })}>
                Add a project…
              </Button>
            }
          >
            <span className="text-body-lg font-ui text-ink">No project yet.</span>
            <span className="text-ink-secondary">Add the main checkout of a repository set up for Keel.</span>
          </Row>
        ) : (
          <Row
            actions={
              <Button disabled={busy} onClick={() => void run({ name: 'projects.remove', args: { project: project.id } })}>
                Remove from the list
              </Button>
            }
          >
            <span className="text-body-lg font-ui text-ink">{project.name}</span>
            <Path path={project.path} />
          </Row>
        )}
        {project === undefined ? null : <ListField waiting={waiting} onShowWaits={onShowWaits} />}
      </Section>

      <Section title="Tools">
        {TOOL_NAMES.map((tool) => (
          <ToolRow key={tool} tool={tool} t={state.tools[tool]} action={action} />
        ))}
      </Section>

      <Section title="PATH">
        <div className="flex flex-col gap-xs py-row">
          {state.path.source === 'login-shell' ? (
            <span className="text-ink">From your login shell.</span>
          ) : (
            <span className="font-medium text-problem">{`A fallback: ${(state.path.reason ?? 'no reason given').replace(/\.$/, '')}.`}</span>
          )}
          <details className="text-ink-secondary">
            <summary className="cursor-pointer font-ui text-ui-sm">The folders ({state.path.entries.length})</summary>
            <ol className="m-0 flex flex-col gap-half pl-xl pt-xs">
            {state.path.entries.map((entry, i) => (
                <li key={`${i}:${entry}`}>
                  <Path path={entry} />
                </li>
              ))}
            </ol>
          </details>
        </div>
      </Section>

      <Section title="Alerts">
        <Row
          actions={
            <Button disabled={busy} onClick={() => void run({ name: 'alert.test', args: {} })}>
              Send a test alert
            </Button>
          }
        >
          <span className="text-ink-secondary">Shows one macOS alert from Keel, to check that alerts reach you.</span>
        </Row>
      </Section>

      <div className="flex flex-col gap-half px-half">
        {note === '' ? null : (
          <p role="status" className="m-0 text-ink">
            {note}
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
