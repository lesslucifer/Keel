// The git facts the list needs, read with the fixed commands only (commands.ts): every local branch
// with its tip and where it stands against its upstream (no fetch: the app never fetches), which
// open branches' tips carry a sign-off (mr), review (cr) or gate (dod) note (one tip at a time), what
// main, each open epic (not in main, or in main with no commit of its own yet) and each open work unit
// with a task holds (main read first, so closed history costs no read), and the worktrees. W6: each open
// tip's review note's verdict (its body, read by the note's own id, so a rewritten note is read again) and
// the commit times the list shows. Reads only; never a diff or a fetch.
// A git older than 2.45 reads a project only when it is not a partial clone, as checkProject decides:
// the project's runner (commands.ts ProjectRunner) asks the project's config once per git and remembers it.

import { NOTE_KINDS, type GitBranch, type GitFacts, type GitWorktree, type NoteKind, type Review } from '../../shared/contract.ts';
import { epicOfUnit, isEpicBranch, isTaskBranch, isUnitBranch, keelBranch, unitOfTask, unitPrefix } from '../../shared/names.ts';
import { ProjectRunner, refOk, OUTPUT_CAP, type Captured, type CommandName, type RunOptions } from '../commands.ts';

const ID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const HEADS = 'refs/heads/';
const NOTES = 'refs/notes/';

/** The refs read: each local branch, the note kinds whose notes ref exists, and each such ref's tip (epic review round 1, git.ts:323). */
export function parseRefs(out: string): { branches: GitBranch[]; noteKinds: Set<NoteKind>; noteTips: Map<NoteKind, string> } {
  const branches: GitBranch[] = [];
  const noteKinds = new Set<NoteKind>();
  const noteTips = new Map<NoteKind, string>();
  for (const row of out.split('\n')) {
    const f = row.split('\u0000');
    if (f.length !== 4) continue;
    const [ref, tip, upstream, track] = f as [string, string, string, string];
    if (!ID.test(tip)) continue;
    if (ref.startsWith(NOTES)) {
      const kind = ref.slice(NOTES.length);
      if ((NOTE_KINDS as readonly string[]).includes(kind)) {
        noteKinds.add(kind as NoteKind);
        noteTips.set(kind as NoteKind, tip);
      }
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
  return { branches, noteKinds, noteTips };
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

// Keel's names, one grammar (shared/names.ts): an epic, a work unit; named() keeps keel's branches only.
const EPIC = { test: isEpicBranch };
const WU = { test: isUnitBranch };

/** keel's branch names among `names` (main, epics, work units, tasks). */
const named = (names: readonly string[]) => names.filter(keelBranch);

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
  const prefix = unitPrefix(epic);
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
    const epic = epicOfUnit(n);
    return open.has(epic) && !(merged[epic] ?? []).includes(n);
  });
}

/** Of `names`, the ones `target` is read for: main the epics, an epic its work units, a work unit its tasks. */
export function heldBy(target: string, names: readonly string[]): string[] {
  const all = named(names);
  if (target === 'main') return all.filter((n) => EPIC.test(n) || WU.test(n));
  if (EPIC.test(target)) return all.filter((n) => n.startsWith(unitPrefix(target)));
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

const VERDICTS = ['PASS', 'FAIL', 'WAIVED'] as const;
const count = (v: unknown): number => (typeof v === 'number' && Number.isInteger(v) && v >= 0 ? v : 0);

/** A review note's body (cr/v1 JSON): its verdict and open counts; anything else reads as no verdict. */
export function parseReview(out: string): Review {
  const none: Review = { verdict: null, blockers: 0, importants: 0 };
  let v: unknown;
  try {
    v = JSON.parse(out);
  } catch {
    return none;
  }
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return none;
  const r = v as Record<string, unknown>;
  if (r.schema !== 'cr/v1' || !(VERDICTS as readonly unknown[]).includes(r.verdict)) return none;
  return { verdict: r.verdict as Review['verdict'], blockers: count(r.blockers), importants: count(r.importants) };
}

/** A commit-time answer: its one ISO 8601 line, or null. */
export function parseTime(out: string): string | null {
  const line = out.replace(/\n$/, '');
  return /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:Z|[+-]\d\d:\d\d)$/.test(line) ? line : null;
}

/**
 * The open branches the list draws its items from (W6), by the one rule: keel's names only (shared/names.ts); the
 * open epics as openEpics has them, none while main is listed but its read failed; a work unit of an open
 * epic only when that epic's read succeeded and does not hold it; a task of an open unit only when that
 * unit's read succeeded and does not hold it.
 */
export function openSets(names: readonly string[], merged: Readonly<Record<string, readonly string[]>>, fresh: ReadonlySet<string> = new Set()): { epics: string[]; units: string[]; tasks: string[] } {
  if (names.includes('main') && merged.main === undefined) return { epics: [], units: [], tasks: [] };
  const epics = openEpics(names, merged.main, fresh);
  const units = named(names).filter((n) => {
    if (!WU.test(n)) return false;
    const epic = epicOfUnit(n);
    return epics.includes(epic) && merged[epic] !== undefined && !(merged[epic] as readonly string[]).includes(n);
  });
  const tasks = named(names).filter((n) => {
    if (!isTaskBranch(n)) return false;
    const unit = unitOfTask(n);
    return units.includes(unit) && merged[unit] !== undefined && !(merged[unit] as readonly string[]).includes(n);
  });
  return { epics, units, tasks };
}

/**
 * main's upstream remote, as the config read answers it (W6): none for one starting with '-' (it would read
 * as an option to git push: review round 3, git.ts:350), a control character, empty or over 200 characters.
 */
export function parseRemote(out: string): string | null {
  const remote = out.replace(/\n$/, '');
  return remote !== '' && !remote.startsWith('-') && remote.length <= 200 && !/[\u0000-\u001f\u007f-\u009f]/.test(remote) ? remote : null;
}

/** The most tips whose commit times one read asks. */
export const TIME_TIPS = 60;

/** The tips whose times the list shows, each once: main, each open epic, each open work unit and each task of one. At most TIME_TIPS. */
export function timeTips(branches: readonly GitBranch[], merged: Readonly<Record<string, readonly string[]>>, fresh: ReadonlySet<string> = new Set()): { tips: string[]; over: boolean } {
  const names = branches.map((b) => b.name);
  const epics = openEpics(names, merged.main, fresh);
  const units = openUnits(names, epics, merged);
  const open = new Set(['main', ...epics, ...units, ...named(names).filter((n) => units.some((u) => n.startsWith(`${u}_`)))]);
  const tips: string[] = [];
  for (const b of branches) if (open.has(b.name) && !tips.includes(b.tip)) tips.push(b.tip);
  return { tips: tips.slice(0, TIME_TIPS), over: tips.length > TIME_TIPS };
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
export const noFacts = (problem: string): GitFacts => ({ branches: [], notes: { mr: [], cr: [], dod: [] }, merged: {}, worktrees: [], open: { epics: [], units: [], tasks: [] }, reviews: {}, times: {}, problem });

/** One project's git reader; read(git) reads it all again with that git. */
export class GitReader {
  /** Every read goes through the project's runner, which handles an old git (commands.ts ProjectRunner); Open reads through it too (W12 review round 1 nit, open.ts:132). */
  readonly runner: ProjectRunner;
  /** Per epic tip main holds: whether it sits on main's own line (no commit of its own). Never changes for a tip; kept only for the tips last asked about. */
  private mainLine = new Map<string, boolean>();
  /** W6: per review note id (a rewritten note is a new id), its verdict; kept only for the notes last read. */
  private reviewOf = new Map<string, Review>();
  /** W6: per tip, its commit time (never changes); kept only for the tips last asked about. */
  private timeOf = new Map<string, string>();
  /**
   * Per note kind: its notes ref's tip at the last read, and per open tip the note's id there (null: none).
   * A tip's note changes only with its notes ref, so an unmoved ref asks nothing (epic review round 1, git.ts:323).
   */
  private noteOf = new Map<NoteKind, { ref: string; byTip: Map<string, string | null> }>();
  /**
   * Per target read for what it holds: the key it was read under (its tip and the tips of the branches it is
   * read for) and what it held. Whether a tip holds another changes only when one of the two moves, so an
   * unchanged key asks nothing (epic review round 1, git.ts:315). Kept only for the targets last read.
   */
  private heldOf = new Map<string, { key: string; held: string[] }>();

  constructor(project: string, opts: Omit<RunOptions, 'before245' | 'timeoutMs'>) {
    this.runner = new ProjectRunner(project, opts);
  }

  private run(git: string, name: CommandName, slots: Record<string, string>): Promise<Captured | string> {
    return this.runner.run(git, name, slots);
  }

  async read(git: string): Promise<GitFacts> {
    const refs = await this.run(git, 'refs', {});
    if (typeof refs === 'string') return noFacts(refs);
    const refsWhy = failed(refs, 'its branches');
    if (refsWhy !== null) return noFacts(refsWhy);
    const { branches, noteTips } = parseRefs(refs.stdout);
    const facts: GitFacts = { branches, notes: { mr: [], cr: [], dod: [] }, merged: {}, worktrees: [], open: { epics: [], units: [], tasks: [] }, reviews: {}, times: {}, problem: null };
    const problems: string[] = [];
    const take = (r: Captured | string, what: string): string | null => {
      const why = typeof r === 'string' ? r : failed(r, what);
      if (why !== null) problems.push(why);
      return why === null && typeof r !== 'string' ? r.stdout : null;
    };
    const names = branches.map((b) => b.name);
    // W6 (review round 2, items.ts:153): main's upstream remote, for the push line; exit 1: none set.
    const main = branches.find((b) => b.name === 'main');
    if (main !== undefined) {
      main.remote = null;
      if (main.upstream !== null) {
        const r = await this.run(git, 'main-remote', {});
        if (typeof r !== 'string' && r.code === 0 && !r.cut) main.remote = parseRemote(r.stdout);
      }
    }
    const tipOf = new Map(branches.map((b) => [b.name, b.tip]));
    const heldOf = new Map<string, { key: string; held: string[] }>();
    const holds = async (target: string): Promise<string[] | null> => {
      const candidates = heldBy(target, names);
      const key = JSON.stringify([tipOf.get(target) ?? null, candidates.map((n) => [n, tipOf.get(n) ?? null])]);
      const known = this.heldOf.get(target);
      let held: string[];
      if (known !== undefined && known.key === key) held = known.held;
      else {
        const out = take(await this.run(git, 'merged', { ref: `${HEADS}${target}` }), `what ${target} holds`);
        if (out === null) return null;
        const listed = new Set(parseMerged(out));
        held = candidates.filter((n) => listed.has(n));
      }
      heldOf.set(target, { key, held });
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
    this.heldOf = heldOf;
    facts.open = openSets(names, facts.merged, fresh);
    const open = openTips(branches, facts.merged, fresh);
    if (open.over) problems.push(`more than ${NOTE_TIPS} open branches: notes read for the first ${NOTE_TIPS} only`);
    const crNotes = new Map<string, string>();
    const noteOf = new Map<NoteKind, { ref: string; byTip: Map<string, string | null> }>();
    for (const kind of NOTE_KINDS) {
      const ref = noteTips.get(kind);
      if (ref === undefined) continue;
      const known = this.noteOf.get(kind);
      const kept = known !== undefined && known.ref === ref ? known.byTip : undefined;
      const byTip = new Map<string, string | null>();
      for (const tip of open.tips) {
        let note = kept?.get(tip);
        if (note === undefined) {
          const r = await this.run(git, 'note-of', { kind, object: tip });
          // Exit 1 is "no note found for object": the tip carries none.
          if (typeof r !== 'string' && r.code === 1 && !r.timedOut && !r.cut) note = null;
          else {
            const out = take(r, `the ${kind} note on ${tip.slice(0, 7)}`);
            if (out === null) continue;
            note = out.trim();
          }
        }
        byTip.set(tip, note);
        if (note === null) continue;
        facts.notes[kind].push(tip);
        // note-of answers the note's own id: the review's body is read by it.
        if (kind === 'cr' && ID.test(note)) crNotes.set(tip, note);
      }
      noteOf.set(kind, { ref, byTip });
    }
    this.noteOf = noteOf;
    // W6: each review's verdict, read once per note (a rewritten note has a new id).
    const reviewOf = new Map<string, Review>();
    for (const [tip, note] of crNotes) {
      let review = this.reviewOf.get(note) ?? reviewOf.get(note);
      if (review === undefined) {
        const out = take(await this.run(git, 'blob', { object: note }), `the review note on ${tip.slice(0, 7)}`);
        if (out === null) continue;
        review = parseReview(out);
      }
      reviewOf.set(note, review);
      facts.reviews[tip] = review;
    }
    this.reviewOf = reviewOf;
    // W6: since when each item waits, once per tip.
    const timed = timeTips(branches, facts.merged, fresh);
    const timeOf = new Map<string, string>();
    for (const tip of timed.tips) {
      let at = this.timeOf.get(tip);
      if (at === undefined) {
        const out = take(await this.run(git, 'commit-time', { object: tip }), `the time of ${tip.slice(0, 7)}`);
        const parsed = out === null ? null : parseTime(out);
        if (parsed === null) continue;
        at = parsed;
      }
      timeOf.set(tip, at);
      facts.times[tip] = at;
    }
    this.timeOf = timeOf;
    const wt = take(await this.run(git, 'worktrees', {}), 'its worktrees');
    if (wt !== null) facts.worktrees = parseWorktrees(wt);
    facts.problem = problems.length === 0 ? null : problems.join('; ');
    return facts;
  }
}
