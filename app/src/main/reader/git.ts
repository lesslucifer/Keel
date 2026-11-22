// The git facts the list needs, read with the fixed commands only (commands.ts): every local branch
// with its tip and where it stands against its upstream (no fetch: the app never fetches), which
// tips carry a sign-off (mr), review (cr) or gate (dod) note, what main, each epic and each work unit
// with a task holds, and the worktrees. Reads only; never a note's body, a diff or a fetch.
// A git older than 2.45 reads a project only when it is not a partial clone, as checkProject decides:
// the reader asks the project's config once per git and remembers the answer.

import { NOTE_KINDS, type GitBranch, type GitFacts, type GitWorktree, type NoteKind } from '../../shared/contract.ts';
import { run, refusedFlag, refOk, MIN_GIT, OLDEST_GIT, type Captured, type CommandName, type RunOptions } from '../commands.ts';

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

/** A notes list ("<note> <object>" per line): the annotated objects that are in `tips`, in the list's order. */
export function parseNotes(out: string, tips: ReadonlySet<string>): string[] {
  const found: string[] = [];
  for (const line of out.split('\n')) {
    const [note, object, extra] = line.split(' ');
    if (extra !== undefined || note === undefined || object === undefined || !ID.test(note) || !ID.test(object)) continue;
    if (tips.has(object) && !found.includes(object)) found.push(object);
  }
  return found;
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

/** The branches whose merges the list reads: main, each epic, each work unit that has a task branch (wu/<e>/<w>_<task>). */
export function mergeTargets(names: readonly string[]): string[] {
  return names.filter((n) => {
    if (!refOk(`${HEADS}${n}`)) return false;
    if (n === 'main' || EPIC.test(n)) return true;
    return WU.test(n) && names.some((m) => m.startsWith(`${n}_`));
  });
}

/** Of `names`, the ones `target` is read for: main the epics, an epic its work units, a work unit its tasks. */
export function heldBy(target: string, names: readonly string[]): string[] {
  const named = names.filter((n) => refOk(`${HEADS}${n}`));
  if (target === 'main') return named.filter((n) => EPIC.test(n));
  if (EPIC.test(target)) return named.filter((n) => n.startsWith(`wu/${target.slice('epic/'.length)}/`));
  return named.filter((n) => n.startsWith(`${target}_`));
}

/** Why a run gave no answer to read, or null. */
function failed(r: Captured, what: string): string | null {
  if (r.startError !== null) return `git could not start (${r.startError})`;
  if (r.timedOut) return `git did not answer (${what})`;
  if (r.code !== 0) return `git could not read the project (${what}, exit ${r.code ?? 'killed'})`;
  return null;
}

const noFacts = (problem: string): GitFacts => ({ branches: [], notes: { mr: [], cr: [], dod: [] }, merged: {}, worktrees: [], problem });

/** One project's git reader; read(git) reads it all again with that git. */
export class GitReader {
  /** Per git path: whether it is older than 2.45 (true), or why it may not read this project. */
  private old = new Map<string, true | string>();

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
    const tips = new Set(branches.map((b) => b.tip));
    for (const kind of NOTE_KINDS) {
      if (!noteKinds.has(kind)) continue;
      const out = take(await this.run(git, 'notes-list', { kind }), `its ${kind} notes`);
      if (out !== null) facts.notes[kind] = parseNotes(out, tips);
    }
    const names = branches.map((b) => b.name);
    for (const target of mergeTargets(names)) {
      const out = take(await this.run(git, 'merged', { ref: `${HEADS}${target}` }), `what ${target} holds`);
      if (out === null) continue;
      const listed = new Set(parseMerged(out));
      facts.merged[target] = heldBy(target, names).filter((n) => listed.has(n));
    }
    const wt = take(await this.run(git, 'worktrees', {}), 'its worktrees');
    if (wt !== null) facts.worktrees = parseWorktrees(wt);
    facts.problem = problems.length === 0 ? null : problems.join('; ');
    return facts;
  }
}
