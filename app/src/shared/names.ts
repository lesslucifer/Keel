// Keel's names, one grammar for the whole app (E-APPLIST.W6, review round 2, items.ts:103; epic review
// round 1, git.ts:106, items.ts:30, items.ts:172): the fixed commands' ref check (main/commands.ts refOk),
// the reader's open sets (main/reader/git.ts), the update log's keys (main/reader/log.ts), the keel://
// link (main/alerter/link.ts) and the item rules (shared/items.ts) all read it. Keel names a branch main,
// epic/<slug>, wu/<epic>/<slug> or a task wu/<epic>/<unit>_<task> (git check-ref-format, narrowed); an epic
// or a work unit's id is read from its name only when the name carries it (epic/E-<CODE>,
// wu/E-<CODE>/w<n>...). The ids E-<CODE> and E-<CODE>.W<m>, and router-app.sh's keys, are written here
// once. Pure: no import.

/** An epic id, E-<CODE>: the one pattern every id below is built from. */
export const EPIC_SOURCE = 'E-[A-Z0-9]{1,16}';
/** A work unit's number within its epic. */
const UNIT_NUMBER = '[0-9]{1,3}';
/** A work unit id, E-<CODE>.W<m>: router-app.sh's wu, and the one pattern the keel:// link, the log and the items take. */
export const WU_SOURCE = `${EPIC_SOURCE}\\.W${UNIT_NUMBER}`;
/** A whole work unit id. */
export const WU_ID = new RegExp(`^${WU_SOURCE}$`);
/** A question's or a worker's name in a router key. */
const KEY_NAME = '[a-z0-9][a-z0-9-]{0,40}';
/** The keys router-app.sh accepts, its key_ok. */
export const KEY = new RegExp(`^(?:router|facts|wu/${WU_SOURCE}/(?:stage|conflict)|epic/${EPIC_SOURCE}/paused|question/${KEY_NAME}|worker/${KEY_NAME}(?:/report)?)$`);

/** A router key, read: what it is about and its name or id. */
export type RouterKey =
  | { kind: 'router' | 'facts' }
  | { kind: 'stage' | 'conflict'; wu: string }
  | { kind: 'paused'; epic: string }
  | { kind: 'question' | 'worker' | 'report'; name: string };

/** A key the log takes (KEY), read; null for any other. */
export function routerKey(key: string): RouterKey | null {
  if (!KEY.test(key)) return null;
  if (key === 'router' || key === 'facts') return { kind: key };
  const [head, name, tail] = key.split('/') as [string, string, string | undefined];
  if (head === 'wu') return { kind: tail === 'stage' ? 'stage' : 'conflict', wu: name };
  if (head === 'epic') return { kind: 'paused', epic: name };
  if (head === 'question') return { kind: 'question', name };
  return { kind: tail === 'report' ? 'report' : 'worker', name };
}

/** The epic a work unit id belongs to (E-APPLIST.W6: E-APPLIST). */
export const epicOfWu = (wu: string): string => wu.slice(0, wu.indexOf('.W'));

/** One part of a branch name: no leading dash or dot, no '..', not ending in '.' or '.lock' (checked below). */
const SEG = '[A-Za-z0-9][A-Za-z0-9._-]{0,99}';
const BRANCH = new RegExp(`^(?:main|epic/${SEG}|wu/${SEG}/${SEG})$`);
const EPIC = /^epic\/[^/]+$/;
const UNIT = /^wu\/[^/]+\/[^/_]+$/;
const TASK = /^wu\/[^/]+\/[^/_]+_[^/_]+$/;
const EPIC_ID = new RegExp(`^epic/(${EPIC_SOURCE})$`);
const WU_BRANCH = new RegExp(`^wu/(${EPIC_SOURCE})/w(${UNIT_NUMBER})(?:[-_]|$)`);

/** Whether `name` is a branch keel names: main, an epic, a work unit or a task, by the grammar above. */
export function keelBranch(name: string): boolean {
  return BRANCH.test(name) && !name.includes('..') && !/\.(?:lock)?(?:\/|$)/.test(name);
}

export const isEpicBranch = (name: string): boolean => keelBranch(name) && EPIC.test(name);
export const isUnitBranch = (name: string): boolean => keelBranch(name) && UNIT.test(name);
export const isTaskBranch = (name: string): boolean => keelBranch(name) && TASK.test(name);

/** The epic branch a work unit or task branch belongs to (wu/E-A/w1-x: epic/E-A). */
export const epicOfUnit = (name: string): string => `epic/${name.split('/')[1] ?? ''}`;

/** What every work unit (and task) branch of an epic branch starts with (epic/E-A: wu/E-A/). */
export const unitPrefix = (epic: string): string => `wu/${epic.slice('epic/'.length)}/`;

/** The work unit a task branch belongs to (wu/E-A/w1-x_probe: wu/E-A/w1-x). */
export const unitOfTask = (name: string): string => name.slice(0, name.lastIndexOf('_'));

/** The work unit id a branch's name carries (wu/E-APPLIST/w6-list-screens: E-APPLIST.W6), or null. */
export function wuOfBranch(branch: string): string | null {
  const m = WU_BRANCH.exec(branch);
  return m === null ? null : `${m[1] as string}.W${Number(m[2])}`;
}

/** The epic id a branch's name carries (epic/E-APPLIST: E-APPLIST), or null. */
export function epicOfBranch(branch: string): string | null {
  return EPIC_ID.exec(branch)?.[1] ?? null;
}
