// The git facts the list needs, read with the fixed commands only (commands.ts): every local branch
// with its tip and where it stands against its upstream (no fetch: the app never fetches), which
// open branches' tips carry a sign-off (mr), review (cr) or gate (dod) note (one tip at a time), what
// main, each open epic (not in main, or in main with no commit of its own yet) and each open work unit
// with a task holds (main read first, so closed history costs no read), and the worktrees. Reads only; never a note's body, a diff or a fetch.
// A git older than 2.45 reads a project only when it is not a partial clone, as checkProject decides:
// the reader asks the project's config once per git and remembers the answer.

import { NOTE_KINDS, type GitBranch, type GitFacts, type GitWorktree, type NoteKind } from '../../shared/contract.ts';
import { run, refusedFlag, refOk, MIN_GIT, OLDEST_GIT, OUTPUT_CAP, type Captured, type CommandName, type RunOptions } from '../commands.ts';

const ID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const HEADS = 'refs/heads/';
const NOTES = 'refs/notes/';

/** The refs read: each local branch, and the note kinds whose notes ref exists. */
export function parseRefs(out: string): { branches: GitBranch[]; noteKinds: Set<NoteKind> } {
  const branches: GitBranch[] = [];
  const noteKinds = new Set<NoteKind>();
  for (const row of out.split('\n')) {
    const f = row.split('\u0000');
    if (f.length !== 4) continue;
    const [ref, tip, upstream, track] = f as [string, string, string, string];
    if (!ID.test(tip)) continue;
    if (ref.startsWith(NOTES)) {
      const kind = ref.slice(NOTES.length);
      if ((NOTE_KINDS as readonly string[]).includes(kind)) noteKinds.add(kind as NoteKind);
      continue;
    }
    if (!ref.startsWith(HEADS) || ref.length === HEADS.length) continue;
    branches.push({
      name: ref.slice(HEADS.length),
      tip,
      upstream: upstream === '' ? null : upstream,
      ahead: Number(/\bahead (\d+)/.exec(track)?.[1] ?? 0),
      behind: Number(/\bbehind (\d+)/.exec(track)?.[1] ?? 0),
      gone: track === 'gone',
    });
  }
  return { branches, noteKinds };
}

/** A merged read: the branch names it lists. */
export function parseMerged(out: string): string[] {
  return out.split('\n').filter((r) => r.startsWith(HEADS) && r.length > HEADS.length).map((r) => r.slice(HEADS.length));
}

/** git worktree list --porcelain: each worktree with an absolute path, its branch (none when detached or bare) and tip. */
export function parseWorktrees(out: string): GitWorktree[] {
  const list: GitWorktree[] = [];
  for (const block of out.split('\n\n')) {
    let wt: GitWorktree | null = null;
    for (const line of block.split('\n')) {
      if (line.startsWith('worktree ')) wt = { path: line.slice('worktree '.length), branch: null, tip: null };
      else if (wt !== null && line.startsWith('HEAD ') && ID.test(line.slice(5))) wt.tip = line.slice(5);
      else if (wt !== null && line.startsWith(`branch ${HEADS}`)) wt.branch = line.slice(`branch ${HEADS}`.length);
    }
    if (wt !== null && wt.path.startsWith('/')) list.push(wt);
  }
  return list;
}

const EPIC = /^epic\/[^/]+$/;
const WU = /^wu\/[^/]+\/[^/_]+$/;

/** keel's branch names among `names` (main, epics, work units, tasks). */
const named = (names: readonly string[]) => names.filter((n) => refOk(`${HEADS}${n}`));

/**
 * The open epics: those main does not hold (`inMain`; every epic when there is no main read), and
 * those it holds only because the epic has no commit of its own yet (`fresh`: its tip on main's own
 * line, as when it is cut and its plan's work unit is open). A merged epic is closed.
 */
export function openEpics(names: readonly string[], inMain: readonly string[] | undefined, fresh: ReadonlySet<string> = new Set()): string[] {
  const held = new Set(inMain ?? []);
  return named(names).filter((n) => EPIC.test(n) && (!held.has(n) || fresh.has(n)));
}

/**
 * Whether an epic main holds may still be fresh (no commit of its own), so worth asking name-rev:
 * only while one of its work units is outside main (else nothing open hangs on it), and none of its
 * units main holds has a tip of its own (a unit merged into the epic gave it commits: closed, no spawn).
 */
export function mayBeFresh(epic: string, branches: readonly GitBranch[], mainUnits: ReadonlySet<string>): boolean {
  const tip = branches.find((b) => b.name === epic)?.tip;
  const prefix = `wu/${epic.slice('epic/'.length)}/`;
  const units = branches.filter((b) => b.name.startsWith(prefix) && WU.test(b.name) && refOk(`${HEADS}${b.name}`));
  if (!units.some((u) => !mainUnits.has(u.name))) return false;
  return !units.some((u) => mainUnits.has(u.name) && u.tip !== tip);
}

/** name-rev's name for a tip: on main's own line when it is main or main~k (no ^: not reached through a merge). */
export function onMainLine(out: string): boolean {
  return /^main(?:~\d+)?$/.test(out.trim());
}

/** The work units of an open epic not yet in it: a unit whose epic is in main, or gone, is closed however long it is kept. */
export function openUnits(names: readonly string[], epics: readonly string[], merged: Readonly<Record<string, readonly string[]>>): string[] {
  const open = new Set(epics);
  return named(names).filter((n) => {
    if (!WU.test(n)) return false;
    const epic = `epic/${n.split('/')[1] as string}`;
    return open.has(epic) && !(merged[epic] ?? []).includes(n);
  });
}

/** Of `names`, the ones `target` is read for: main the epics, an epic its work units, a work unit its tasks. */
export function heldBy(target: string, names: readonly string[]): string[] {
  const all = named(names);
  if (target === 'main') return all.filter((n) => EPIC.test(n) || WU.test(n));
  if (EPIC.test(target)) return all.filter((n) => n.startsWith(`wu/${target.slice('epic/'.length)}/`));
  return all.filter((n) => n.startsWith(`${target}_`));
}

/** The most open branches whose tips' notes one read asks about. */
export const NOTE_TIPS = 40;

/**
 * The tips whose notes the list needs, each once: each epic not yet in main, each work unit of such
 * an epic not yet in it (a merged one's item has left the list; a unit of a closed or gone epic is
 * closed). At most NOTE_TIPS; `over` says more were open. So the notes read is bounded by the open
 * branches, never by history.
 */
export function openTips(branches: readonly GitBranch[], merged: Readonly<Record<string, readonly string[]>>, fresh: ReadonlySet<string> = new Set()): { tips: string[]; over: boolean } {
  const names = branches.map((b) => b.name);
  const epics = openEpics(names, merged.main, fresh);
  const open = new Set([...epics, ...openUnits(names, epics, merged)]);
  const tips: string[] = [];
  for (const b of branches) if (open.has(b.name) && !tips.includes(b.tip)) tips.push(b.tip);
  return { tips: tips.slice(0, NOTE_TIPS), over: tips.length > NOTE_TIPS };
}

/** Why a run gave no answer to read, or null. */
function failed(r: Captured, what: string): string | null {
  if (r.startError !== null) return `git could not start (${r.startError})`;
  if (r.timedOut) return `git did not answer (${what})`;
  if (r.code !== 0) return `git could not read the project (${what}, exit ${r.code ?? 'killed'})`;
  if (r.cut) return `git's answer was cut at ${OUTPUT_CAP / 1024 / 1024} MiB (${what}): not read`;
  return null;
}

/** No git facts, and why. */
export const noFacts = (problem: string): GitFacts => ({ branches: [], notes: { mr: [], cr: [], dod: [] }, merged: {}, worktrees: [], problem });

/** One project's git reader; read(git) reads it all again with that git. */
export class GitReader {
  /** Per git path: whether it is older than 2.45 (true), or why it may not read this project. */
  private old = new Map<string, true | string>();
  /** Per epic tip main holds: whether it sits on main's own line (no commit of its own). Never changes for a tip; kept only for the tips last asked about. */
  private mainLine = new Map<string, boolean>();

  private readonly project: string;
  private readonly opts: Omit<RunOptions, 'before245' | 'timeoutMs'>;

  constructor(project: string, opts: Omit<RunOptions, 'before245' | 'timeoutMs'>) {
    this.project = project;
    this.opts = opts;
  }

  /** Run a read; an old git is found out once (its config read), then remembered. */
  private async run(git: string, name: CommandName, slots: Record<string, string>): Promise<Captured | string> {
    const known = this.old.get(git);
    if (typeof known === 'string') return known;
    const all = { project: this.project, ...slots };
    const r = await run(git, name, all, { ...this.opts, before245: known === true });
    const refused = refusedFlag(r);
    if (refused === null || known === true) return r;
    if (refused === '--no-optional-locks') {
      this.old.set(git, `this git is older than ${OLDEST_GIT}: choose a newer git in the settings`);
    } else {
      const promisor = await run(git, 'promisor-config', { project: this.project }, { ...this.opts, before245: true });
      if (promisor.code === 1) this.old.set(git, true);
      else if (promisor.code === 0) this.old.set(git, `a partial clone needs git ${MIN_GIT} or later: choose a newer git in the settings`);
      else return failed(promisor, 'its git settings') ?? "git could not read the project's git settings";
    }
    return this.run(git, name, slots);
  }

  async read(git: string): Promise<GitFacts> {
    const refs = await this.run(git, 'refs', {});
    if (typeof refs === 'string') return noFacts(refs);
    const refsWhy = failed(refs, 'its branches');
    if (refsWhy !== null) return noFacts(refsWhy);
    const { branches, noteKinds } = parseRefs(refs.stdout);
    const facts: GitFacts = { branches, notes: { mr: [], cr: [], dod: [] }, merged: {}, worktrees: [], problem: null };
    const problems: string[] = [];
    const take = (r: Captured | string, what: string): string | null => {
      const why = typeof r === 'string' ? r : failed(r, what);
      if (why !== null) problems.push(why);
      return why === null && typeof r !== 'string' ? r.stdout : null;
    };
    const names = branches.map((b) => b.name);
    const holds = async (target: string): Promise<string[] | null> => {
      const out = take(await this.run(git, 'merged', { ref: `${HEADS}${target}` }), `what ${target} holds`);
      if (out === null) return null;
      const listed = new Set(parseMerged(out));
      const held = heldBy(target, names).filter((n) => listed.has(n));
      // main is read for its epics and units; the view publishes only the epics (the units stay here).
      facts.merged[target] = target === 'main' ? held.filter((n) => EPIC.test(n)) : held;
      return held;
    };
    // Main first, then only what is still open: a closed epic, and its kept units and tasks, cost no read.
    const mainHeld = names.includes('main') ? await holds('main') : null;
    const mainUnits = new Set((mainHeld ?? []).filter((n) => WU.test(n)));
    // An epic main holds is open while it has no commit of its own (its tip on main's own line) and one
    // of its work units is outside main; name-rev is asked only when mayBeFresh, once per tip.
    const fresh = new Set<string>();
    const inMain = new Set(facts.merged.main ?? []);
    const asked = new Map<string, boolean>();
    for (const b of branches) {
      if (!EPIC.test(b.name) || !inMain.has(b.name) || !refOk(`${HEADS}${b.name}`)) continue;
      if (!mayBeFresh(b.name, branches, mainUnits)) continue;
      let line = this.mainLine.get(b.tip) ?? asked.get(b.tip);
      if (line === undefined) {
        const r = await this.run(git, 'main-name', { object: b.tip });
        const out = take(r, `where main holds ${b.name}`);
        if (out === null) {
          fresh.add(b.name);
          continue;
        }
        line = onMainLine(out);
      }
      asked.set(b.tip, line);
      if (line) fresh.add(b.name);
    }
    // A failed main read asked nothing: keep what was known for the next good read.
    if (mainHeld !== null) this.mainLine = asked;
    const epics = openEpics(names, facts.merged.main, fresh);
    for (const epic of epics) await holds(epic);
    for (const unit of openUnits(names, epics, facts.merged)) if (names.some((n) => n.startsWith(`${unit}_`))) await holds(unit);
    const open = openTips(branches, facts.merged, fresh);
    if (open.over) problems.push(`more than ${NOTE_TIPS} open branches: notes read for the first ${NOTE_TIPS} only`);
    for (const kind of NOTE_KINDS) {
      if (!noteKinds.has(kind)) continue;
      for (const tip of open.tips) {
        const r = await this.run(git, 'note-of', { kind, object: tip });
        // Exit 1 is "no note found for object": the tip carries none.
        if (typeof r !== 'string' && r.code === 1 && !r.timedOut && !r.cut) continue;
        if (take(r, `the ${kind} note on ${tip.slice(0, 7)}`) !== null) facts.notes[kind].push(tip);
      }
    }
    const wt = take(await this.run(git, 'worktrees', {}), 'its worktrees');
    if (wt !== null) facts.worktrees = parseWorktrees(wt);
    facts.problem = problems.length === 0 ? null : problems.join('; ');
    return facts;
  }
}
