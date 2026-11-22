// Screen 6, Projects (docs/local-app.html 'The screens'): the one project with Remove, or Add when
// there is none; keel, git and claude each with its path, its source, its problem, Choose and, when
// set in Keel, Use the PATH; where the PATH came from; the test alert; the version at the foot. Each
// control sends one request through useAction; while one runs every control is off. Every value is
// React text, never markup.

import type { AppState, Tool, ToolState } from '../../shared/contract.ts';
import { TOOL_NAMES } from '../../shared/contract.ts';
import type { ActionView } from '../hooks.ts';
import { Badge, type Tone } from '../ui/Badge.tsx';
import { Button } from '../ui/Button.tsx';
import { Path } from '../ui/Path.tsx';
import { Row } from '../ui/Row.tsx';
import { Section } from '../ui/Section.tsx';

const SOURCE: Record<ToolState['source'], { words: string; tone: Tone }> = {
  set: { words: 'set in Keel', tone: 'accent' },
  path: { words: 'found on the PATH', tone: 'ok' },
  missing: { words: 'missing', tone: 'fail' },
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
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={SOURCE[t.source].tone}>{SOURCE[t.source].words}</Badge>
        {t.path === null ? <span className="text-ink-muted">not found</span> : <Path path={t.path} />}
      </div>
      {t.problem === null ? null : <span className="text-fail">{t.problem}</span>}
    </Row>
  );
}

export function Projects({ state, action, error }: { state: AppState; action: ActionView; error: string }) {
  const { busy, run, note } = action;
  const project = state.projects[0];
  return (
    <div className="flex flex-col gap-6">
      <Section title="Project">
        {project === undefined ? (
          <Row
            actions={
              <Button kind="primary" disabled={busy} onClick={() => void run({ name: 'projects.add', args: {} })}>
                Add a project…
              </Button>
            }
          >
            <span className="text-ink">No project yet.</span>
            <span className="text-ink-muted">Add the main checkout of a repository set up for Keel.</span>
          </Row>
        ) : (
          <Row
            actions={
              <Button disabled={busy} onClick={() => void run({ name: 'projects.remove', args: { project: project.id } })}>
                Remove from the list
              </Button>
            }
          >
            <span className="font-semibold text-ink">{project.name}</span>
            <Path path={project.path} />
          </Row>
        )}
      </Section>

      <Section title="Tools">
        {TOOL_NAMES.map((tool) => (
          <ToolRow key={tool} tool={tool} t={state.tools[tool]} action={action} />
        ))}
      </Section>

      <Section title="PATH">
        <span className="text-ink">{state.path.source === 'login-shell' ? 'From your login shell.' : `A fallback: ${(state.path.reason ?? 'no reason given').replace(/\.$/, '')}.`}</span>
        <details className="text-ink-muted">
          <summary className="cursor-pointer">The folders ({state.path.entries.length})</summary>
          <ol className="mt-2 flex flex-col gap-1">
            {state.path.entries.map((entry, i) => (
              <li key={`${i}:${entry}`}>
                <Path path={entry} />
              </li>
            ))}
          </ol>
        </details>
      </Section>

      <Section title="Alerts">
        <Row
          actions={
            <Button disabled={busy} onClick={() => void run({ name: 'alert.test', args: {} })}>
              Send a test alert
            </Button>
          }
        >
          <span className="text-ink-muted">Shows one macOS alert from Keel, to check that alerts reach you.</span>
        </Row>
      </Section>

      <div className="flex flex-col gap-1">
        {note === '' ? null : (
          <p role="status" className="m-0 text-ink">
            {note}
          </p>
        )}
        {error === '' ? null : (
          <p role="alert" className="m-0 text-fail">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
