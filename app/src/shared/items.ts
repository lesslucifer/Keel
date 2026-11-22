// The item rules (docs/local-app.html 'The hand-off'; E-APPLIST.W6): which items the view's facts make,
// their group, their words, and when each leaves. Pure: no import but the contract's types, so the main
// part draws the list with it on every read and the window's fixtures draw theirs the same way.
//
// A result enters on a PASS or WAIVED review note on an open work unit's (or epic's) tip with no sign-off
// note, turns to merge on the sign-off note, and leaves when its target holds the tip; a failed or
// unreadable review is its worker's. A plan to read, a question, a worker waiting and work in motion come
// only from the router, and only while it is heard. Main not pushed while main is ahead of a live
// upstream. A task while its tip is outside its open work unit and no busy worker is on its branch (the
// owner's answer 2a of 2026-11-22). A sent mark moves a plan or a question to Sent; ids carry a version
// (a tip, or a short hash of what the router said), so a moved tip or a new question is a new item.

import { ITEM_ID_SOURCE, SLOT_CAP, type GitFacts, type Item, type ItemCounts, type ItemGroup, type ItemKind, type Review, type RouterView } from './contract.ts';

export interface ItemInput {
  git: GitFacts | null;
  router: RouterView;
  /** The ids the owner marked sent, for this project. */
  sent: ReadonlySet<string>;
}

const ITEM_ID = new RegExp(ITEM_ID_SOURCE);
const SUBJECT = /^[A-Za-z0-9._-]{1,41}$/;
const HEX = /^[0-9a-f]{7,}$/;
const EPIC_BRANCH = /^epic\/(E-[A-Z0-9]{1,16})$/;
const WU_BRANCH = /^wu\/(E-[A-Z0-9]{1,16})\/w([0-9]{1,3})(?:[-_]|$)/;
/** A work unit's branch (wu/<epic>/<slug>) and a task's (wu/<epic>/<unit>_<task>), keel's naming. */
const UNIT = /^wu\/[^/]+\/[^/_]+$/;
const TASK = /^(wu\/[^/]+\/[^/_]+)_[^/_]+$/;
const WU_ID = /^E-[A-Z0-9]{1,16}\.W[0-9]{1,3}$/;
const SESSION = /^[A-Za-z0-9][A-Za-z0-9-]{0,63}$/;
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** FNV-1a over the string's UTF-16 units, as 8 hex digits: an id's short hash, not a secret. */
export function shortHash(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, '0');
}

/** kind/subject@version: a subject that is not a plain name, or too long, becomes its short hash; a version that is not hex too. */
export function itemId(kind: ItemKind, subject: string, version: string): string {
  const v = HEX.test(version) ? version.slice(0, 12) : shortHash(version);
  const plainId = SUBJECT.test(subject) ? `${kind}/${subject}@${v}` : null;
  const id = plainId !== null && plainId.length <= SLOT_CAP && ITEM_ID.test(plainId) ? plainId : `${kind}/${shortHash(subject)}@${v}`;
  return id;
}

/** The work unit a branch names by keel's naming (wu/E-APPLIST/w6-list-screens: E-APPLIST.W6), or null. */
export function wuOfBranch(branch: string): string | null {
  const m = WU_BRANCH.exec(branch);
  return m === null ? null : `${m[1] as string}.W${Number(m[2])}`;
}

/** The epic a branch names (epic/E-APPLIST: E-APPLIST), or null. */
export function epicOfBranch(branch: string): string | null {
  return EPIC_BRANCH.exec(branch)?.[1] ?? null;
}

const GROUPS: readonly ItemGroup[] = ['yours', 'not-yours', 'sent'];
const KINDS: readonly ItemKind[] = ['result', 'epic-result', 'question', 'plan', 'worker', 'task', 'push', 'motion'];

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const text = (v: unknown): string | null => (typeof v === 'string' && v.trim() !== '' && !CONTROL.test(v) ? v : null);
const iso = (v: unknown): string | null => (typeof v === 'string' && !Number.isNaN(Date.parse(v)) ? v : null);

function item(over: Partial<Item> & Pick<Item, 'id' | 'kind' | 'title' | 'why'>): Item {
  return { group: 'yours', wu: null, epic: null, branch: null, tip: null, since: null, open: 'none', markable: false, worker: null, line: null, text: [], ...over };
}

/** The counts a list holds. */
export function countsOf(items: readonly Item[]): ItemCounts {
  const n = (g: ItemGroup) => items.filter((i) => i.group === g).length;
  return { yours: n('yours'), notYours: n('not-yours'), sent: n('sent') };
}

/** The open epics and work units, as git says (W5's rules: main holds an epic unless it has no commit of its own yet; an epic holds its units). */
function openBranches(git: GitFacts): { epics: Set<string>; units: Set<string> } {
  const names = git.branches.map((b) => b.name);
  const inMain = new Set(git.merged.main ?? []);
  const epics = new Set(names.filter((n) => EPIC_BRANCH.test(n) && (!inMain.has(n) || Object.hasOwn(git.merged, n))));
  const units = new Set(
    names.filter((n) => {
      if (!UNIT.test(n)) return false;
      const epic = `epic/${n.split('/')[1] as string}`;
      return epics.has(epic) && !(git.merged[epic] ?? []).includes(n);
    }),
  );
  return { epics, units };
}

/** The router's workers: name, and its value when it is an object. */
function workers(values: Record<string, unknown>): Array<[string, Record<string, unknown>]> {
  return Object.entries(values)
    .filter(([k, v]) => k.startsWith('worker/') && !k.endsWith('/report') && isObject(v))
    .map(([k, v]) => [k.slice('worker/'.length), v as Record<string, unknown>] as [string, Record<string, unknown>])
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
}

function resultItems(git: GitFacts, epics: Set<string>, units: Set<string>, workerOn: (branch: string) => string | null): Item[] {
  const out: Item[] = [];
  for (const b of git.branches) {
    const isEpic = epics.has(b.name);
    if (!isEpic && !units.has(b.name)) continue;
    if (!git.notes.cr.includes(b.tip)) continue;
    const review: Review | undefined = git.reviews[b.tip];
    const kind: ItemKind = isEpic ? 'epic-result' : 'result';
    const wu = isEpic ? null : wuOfBranch(b.name);
    const epic = isEpic ? epicOfBranch(b.name) : (epicOfBranch(`epic/${b.name.split('/')[1] as string}`) ?? null);
    const target = isEpic ? 'main' : `epic/${b.name.split('/')[1] as string}`;
    const base = { id: itemId(kind, (isEpic ? epic : wu) ?? b.name, b.tip), kind, wu, epic, branch: b.name, tip: b.tip.slice(0, 7), since: git.times[b.tip] ?? null };
    if (review === undefined || review.verdict === null) {
      out.push(item({ ...base, group: 'not-yours', title: 'Review note unreadable', why: 'Keel cannot read the review note on this tip.', worker: workerOn(b.name) }));
    } else if (review.verdict === 'FAIL') {
      out.push(item({ ...base, group: 'not-yours', title: 'Review failed', why: `${plural(review.blockers, 'blocker')} and ${plural(review.importants, 'important')} open: its worker fixes them.`, worker: workerOn(b.name) }));
    } else if (git.notes.mr.includes(b.tip)) {
      out.push(item({ ...base, title: isEpic ? 'Epic result to merge' : 'Result to merge', why: `Signed. The merge into ${target} is yours, in your terminal.`, open: isEpic ? 'archive' : 'page', line: `git merge --no-ff --no-edit ${b.name}` }));
    } else {
      const why = review.verdict === 'PASS' ? 'The review passed. Your sign-off is missing.' : `The review was waived with ${plural(review.blockers, 'blocker')} and ${plural(review.importants, 'important')} open. Your sign-off is missing.`;
      out.push(item({ ...base, title: isEpic ? 'Epic result to sign' : 'Result to sign', why, open: isEpic ? 'archive' : 'page' }));
    }
  }
  return out;
}

const MOTION: Record<string, { title: string; why: string }> = {
  'plan in work': { title: 'Plan being written', why: 'Its worker writes the plan.' },
  building: { title: 'Building', why: 'Its worker builds it.' },
  'in review': { title: 'In review', why: 'A reviewer reads it.' },
};

/** The list the facts make, ordered yours, not yours, sent; then by kind; then by id. */
export function itemsOf(input: ItemInput): Item[] {
  const { git, router, sent } = input;
  const out: Item[] = [];
  const heard = router.run !== null && router.heard;
  const values = heard ? router.values : {};
  const ws = workers(values);
  const workerOn = (branch: string): string | null => ws.find(([, v]) => v.branch === branch)?.[0] ?? null;
  const branchOf = (wu: string): { name: string; tip: string } | null => {
    const found = (git?.branches ?? []).filter((b) => UNIT.test(b.name) && wuOfBranch(b.name) === wu).sort((a, b) => (a.name < b.name ? -1 : 1))[0];
    return found === undefined ? null : found;
  };

  if (git !== null) {
    const { epics, units } = openBranches(git);
    out.push(...resultItems(git, epics, units, workerOn));

    const main = git.branches.find((b) => b.name === 'main');
    if (main !== undefined && main.upstream !== null && !main.gone && main.ahead > 0) {
      const remote = main.upstream.split('/')[0] as string;
      out.push(item({ id: itemId('push', 'main', main.tip), kind: 'push', title: 'Main not pushed', branch: 'main', tip: main.tip.slice(0, 7), since: git.times[main.tip] ?? null, why: `main is ${plural(main.ahead, 'commit')} ahead of ${main.upstream}.`, line: `git push ${remote} main` }));
    }

    for (const b of git.branches) {
      const m = TASK.exec(b.name);
      if (m === null) continue;
      const unit = m[1] as string;
      if (!units.has(unit) || (git.merged[unit] ?? []).includes(b.name)) continue;
      if (heard && ws.some(([, v]) => v.branch === b.name && v.state === 'busy')) continue;
      const first = heard ? 'No worker is busy on it.' : 'The router is not heard, so a worker may still be on it.';
      const wu = wuOfBranch(unit);
      out.push(item({ id: itemId('task', b.name, b.tip), kind: 'task', title: 'Task into its work unit', wu, epic: wu === null ? null : (wu.split('.')[0] as string), branch: b.name, tip: b.tip.slice(0, 7), since: git.times[b.tip] ?? null, why: `${first} The merge into ${unit} is yours, in its folder.`, line: `git merge --no-ff ${b.name}` }));
    }
  }

  if (heard) {
    const resultWus = new Set(out.filter((i) => i.kind === 'result').map((i) => i.wu));
    for (const [key, value] of Object.entries(values)) {
      const stage = /^wu\/(E-[A-Z0-9]{1,16}\.W[0-9]{1,3})\/stage$/.exec(key);
      if (stage !== null && isObject(value) && typeof value.stage === 'string') {
        const wu = stage[1] as string;
        const epic = wu.split('.')[0] as string;
        const branch = branchOf(wu);
        const tip = typeof value.tip === 'string' && /^[0-9a-f]{7,40}$/.test(value.tip) ? value.tip : null;
        if (value.stage === 'plan to read') {
          // The backstop: the branch moved past the plan's tip.
          if (branch !== null && tip !== null && !branch.tip.startsWith(tip)) continue;
          const since = branch !== null && tip !== null ? (git?.times[branch.tip] ?? null) : null;
          out.push(item({ id: itemId('plan', wu, tip ?? `${router.run as string}\n${JSON.stringify(value)}`), kind: 'plan', title: 'Plan to read', wu, epic, branch: branch?.name ?? null, tip: tip === null ? null : tip.slice(0, 7), since, why: 'The router waits for your word on the plan.', open: 'page', markable: true }));
        } else if (Object.hasOwn(MOTION, value.stage) && !resultWus.has(wu)) {
          const words = MOTION[value.stage] as { title: string; why: string };
          out.push(item({ id: itemId('motion', wu, `${router.run as string}\n${JSON.stringify(value)}`), kind: 'motion', group: 'not-yours', ...words, wu, epic, branch: branch?.name ?? null, tip: branch === null ? null : branch.tip.slice(0, 7), since: branch === null ? null : (git?.times[branch.tip] ?? null), worker: branch === null ? null : workerOn(branch.name) }));
        }
        continue;
      }
      const question = /^question\/([a-z0-9][a-z0-9-]{0,40})$/.exec(key);
      if (question !== null && isObject(value)) {
        const headline = text(value.headline);
        if (headline === null) continue;
        const lines = Array.isArray(value.lines) ? value.lines.filter((l): l is string => typeof l === 'string' && l !== '') : [];
        const wu = typeof value.wu === 'string' && WU_ID.test(value.wu) ? value.wu : null;
        out.push(item({ id: itemId('question', question[1] as string, `${router.run as string}\n${JSON.stringify(value)}`), kind: 'question', title: 'Question from the router', wu, epic: wu === null ? null : (wu.split('.')[0] as string), why: headline, open: lines.length > 0 ? 'text' : 'none', markable: true, text: lines }));
      }
    }
    for (const [name, v] of ws) {
      const waiting = text(v.waiting);
      if (waiting === null) continue;
      const branch = typeof v.branch === 'string' && v.branch.length <= 200 && !CONTROL.test(v.branch) && v.branch !== '' ? v.branch : null;
      const session = typeof v.session === 'string' && SESSION.test(v.session) ? v.session : null;
      const wu = branch === null ? null : wuOfBranch(branch);
      out.push(item({ id: itemId('worker', name, `${name}\n${waiting}\n${session ?? ''}`), kind: 'worker', title: 'Worker waits on you', wu, epic: wu === null ? null : (wu.split('.')[0] as string), branch, since: iso(v.active), why: `Waits on ${waiting.replace(/[.\s]+$/, '')}: answer it in its session.`, worker: name, line: session === null ? null : `claude attach ${session}` }));
    }
  }

  for (const i of out) {
    if (!i.markable || !sent.has(i.id)) continue;
    i.group = 'sent';
    i.why = i.kind === 'plan' ? 'You marked it sent: it leaves when the router moves the stage on.' : 'You marked it sent: it leaves when the router removes the question.';
  }
  return out.sort((a, b) => GROUPS.indexOf(a.group) - GROUPS.indexOf(b.group) || KINDS.indexOf(a.kind) - KINDS.indexOf(b.kind) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}
