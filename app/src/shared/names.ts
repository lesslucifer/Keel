// Keel's branch names, one grammar for the whole app (E-APPLIST.W6, review round 2, items.ts:103): the
// fixed commands' ref check (main/commands.ts refOk), the reader's open sets (main/reader/git.ts) and the
// item rules (shared/items.ts) all read it. Keel names a branch main, epic/<slug>, wu/<epic>/<slug> or a
// task wu/<epic>/<unit>_<task> (git check-ref-format, narrowed); an epic or a work unit's id is read from
// its name only when the name carries it (epic/E-<CODE>, wu/E-<CODE>/w<n>...). Pure: no import.

/** One part of a branch name: no leading dash or dot, no '..', not ending in '.' or '.lock' (checked below). */
const SEG = '[A-Za-z0-9][A-Za-z0-9._-]{0,99}';
const BRANCH = new RegExp(`^(?:main|epic/${SEG}|wu/${SEG}/${SEG})$`);
const EPIC = /^epic\/[^/]+$/;
const UNIT = /^wu\/[^/]+\/[^/_]+$/;
const TASK = /^wu\/[^/]+\/[^/_]+_[^/_]+$/;
const EPIC_ID = /^epic\/(E-[A-Z0-9]{1,16})$/;
const WU_ID = /^wu\/(E-[A-Z0-9]{1,16})\/w([0-9]{1,3})(?:[-_]|$)/;

/** Whether `name` is a branch keel names: main, an epic, a work unit or a task, by the grammar above. */
export function keelBranch(name: string): boolean {
  return BRANCH.test(name) && !name.includes('..') && !/\.(?:lock)?(?:\/|$)/.test(name);
}

export const isEpicBranch = (name: string): boolean => keelBranch(name) && EPIC.test(name);
export const isUnitBranch = (name: string): boolean => keelBranch(name) && UNIT.test(name);
export const isTaskBranch = (name: string): boolean => keelBranch(name) && TASK.test(name);

/** The work unit a task branch belongs to (wu/E-A/w1-x_probe: wu/E-A/w1-x). */
export const unitOfTask = (name: string): string => name.slice(0, name.lastIndexOf('_'));

/** The work unit id a branch's name carries (wu/E-APPLIST/w6-list-screens: E-APPLIST.W6), or null. */
export function wuOfBranch(branch: string): string | null {
  const m = WU_ID.exec(branch);
  return m === null ? null : `${m[1] as string}.W${Number(m[2])}`;
}

/** The epic id a branch's name carries (epic/E-APPLIST: E-APPLIST), or null. */
export function epicOfBranch(branch: string): string | null {
  return EPIC_ID.exec(branch)?.[1] ?? null;
}
