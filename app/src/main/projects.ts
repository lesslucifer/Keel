// projects.json in the app's folder: {version: 1, projects: [{id, path, name}]}. A project is a
// Keel project's main checkout, judged only by reading git with the fixed commands: its top folder
// is the folder chosen, its common git folder is that folder's .git (not a linked worktree or a
// submodule), and its HEAD commits .keel/config.toml as a regular file. The git that reads it must
// not be inside the folder: a project's own files never become the program the app runs. The app
// never writes inside a project.
// E-APPLIST keeps one project; the file already holds a list, so A5 lifts the cap with no migration.
// Each change saved calls `changed` once (the shell pushes state.changed); a refusal saves nothing.

import { realpath, stat } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
import type { ProjectEntry } from '../shared/contract.ts';
import { PROJECT_ID } from './bridge.ts';
import { run, MIN_GIT, type Captured, type RunOptions } from './commands.ts';
import { load, writeAtomic } from './store.ts';
import { plain } from './text.ts';
import { toolProblem } from './tools.ts';

export const PROJECTS_FILE = 'projects.json';
export const MAX_PROJECTS = 1;
export const NAME_CAP = 200;
/** The most projects a list file may hold and still parse (a later version's list, read by this one). */
const LIST_CAP = 100;

export type Project = ProjectEntry;

export interface ProjectList {
  version: 1;
  projects: Project[];
}

export function defaultProjects(): ProjectList {
  return { version: 1, projects: [] };
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v) && Object.getPrototypeOf(v) === Object.prototype;
}

const keys = (v: Record<string, unknown>) => Object.keys(v).sort().join(',');
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** The list, or null for any shape the app does not know. */
export function parseProjects(v: unknown): ProjectList | null {
  if (!isPlainObject(v) || keys(v) !== 'projects,version' || v.version !== 1 || !Array.isArray(v.projects) || v.projects.length > LIST_CAP) return null;
  const out: Project[] = [];
  for (const p of v.projects as unknown[]) {
    if (!isPlainObject(p) || keys(p) !== 'id,name,path') return null;
    const { id, path: dir, name } = p;
    if (typeof id !== 'string' || !PROJECT_ID.test(id)) return null;
    if (typeof dir !== 'string' || !path.isAbsolute(dir) || CONTROL.test(dir)) return null;
    if (typeof name !== 'string' || name !== plain(name, NAME_CAP)) return null;
    if (out.some((q) => q.id === id || q.path === dir)) return null;
    out.push({ id, path: dir, name });
  }
  return { version: 1, projects: out };
}

/** A new id, 'p' and 8 hex digits, not one already `taken`. */
export function mintId(taken: readonly string[], random: (n: number) => Buffer = randomBytes): string {
  for (;;) {
    const id = `p${random(4).toString('hex')}`;
    if (!taken.includes(id)) return id;
  }
}

export function loadProjects(appDir: string, now?: Date): Promise<{ value: ProjectList; setAside: string | null }> {
  return load(path.join(appDir, PROJECTS_FILE), parseProjects, defaultProjects, now);
}

async function real(p: string): Promise<string | null> {
  try {
    return await realpath(p);
  } catch {
    return null;
  }
}

/** Whether `ls-tree HEAD -- .keel/config.toml` printed exactly one entry: that path, a regular file (mode 100644 or 100755). */
export function commitsConfig(out: string): boolean {
  return /^100(644|755) blob ([0-9a-f]{40}|[0-9a-f]{64})\t\.keel\/config\.toml\n$/.test(out);
}

/** Why a git run did not answer, if it did not: timed out, or a git too old for the locked flags. */
function gitFailed(r: Captured): string | null {
  if (r.timedOut) return 'git did not answer';
  if (r.code === 129 && r.stderr.includes('--no-lazy-fetch')) return `this git is older than ${MIN_GIT}: choose a newer git in the settings`;
  return null;
}

/** The folder's real path when it is a Keel project's main checkout, else why not. Reads only. */
export async function checkProject(folder: string, git: string, opts: RunOptions): Promise<{ ok: true; path: string } | { ok: false; why: string }> {
  if (typeof folder !== 'string' || !path.isAbsolute(folder)) return { ok: false, why: 'the folder must be an absolute path' };
  const dir = await real(folder);
  if (dir === null) return { ok: false, why: 'not found' };
  if (!(await stat(dir)).isDirectory()) return { ok: false, why: 'not a folder' };
  // Before any run: a git inside the folder would be the folder's own program run on it.
  const gitWhy = await toolProblem('git', git, [dir]);
  if (gitWhy !== null) return { ok: false, why: `git is ${gitWhy}: choose a git outside it in the settings` };
  const top = await run(git, 'toplevel', { project: dir }, opts);
  const topFailed = gitFailed(top);
  if (topFailed !== null) return { ok: false, why: topFailed };
  if (top.code !== 0) return { ok: false, why: 'not a git checkout' };
  if ((await real(top.stdout.trim())) !== dir) return { ok: false, why: 'not the top folder of its git checkout' };
  const common = await run(git, 'common-dir', { project: dir }, opts);
  const commonFailed = gitFailed(common);
  if (commonFailed !== null) return { ok: false, why: commonFailed };
  if (common.code !== 0 || (await real(common.stdout.trim())) !== path.join(dir, '.git')) {
    return { ok: false, why: 'a linked worktree or a submodule: add its main checkout' };
  }
  const config = await run(git, 'has-config', { project: dir }, opts);
  const configFailed = gitFailed(config);
  if (configFailed !== null) return { ok: false, why: configFailed };
  if (config.code !== 0 || !commitsConfig(config.stdout)) return { ok: false, why: 'its HEAD does not commit .keel/config.toml' };
  return { ok: true, path: dir };
}

/** Add `folder` (from the main part's dialog) when the list has room and the checks pass. */
export async function addProject(
  appDir: string,
  folder: string,
  git: string,
  opts: RunOptions,
  changed: () => void = () => undefined,
): Promise<{ ok: true; list: ProjectList; project: Project } | { ok: false; why: string }> {
  if (typeof folder !== 'string' || !path.isAbsolute(folder)) return { ok: false, why: 'the folder must be an absolute path' };
  const list = (await loadProjects(appDir)).value;
  if (list.projects.length >= MAX_PROJECTS) {
    return { ok: false, why: `this version keeps one project: remove ${list.projects[0]?.name ?? 'it'} first` };
  }
  const c = await checkProject(folder, git, opts);
  if (!c.ok) return c;
  if (list.projects.some((p) => p.path === c.path)) return { ok: false, why: 'already listed' };
  const project: Project = { id: mintId(list.projects.map((p) => p.id)), path: c.path, name: plain(path.basename(c.path), NAME_CAP) };
  const next: ProjectList = { version: 1, projects: [...list.projects, project] };
  await writeAtomic(path.join(appDir, PROJECTS_FILE), next);
  changed();
  return { ok: true, list: next, project };
}

/** Drop a project from the list; its folder is not touched. */
export async function removeProject(appDir: string, id: string, changed: () => void = () => undefined): Promise<{ ok: true; list: ProjectList } | { ok: false; why: string }> {
  if (typeof id !== 'string' || !PROJECT_ID.test(id)) return { ok: false, why: 'no such project' };
  const list = (await loadProjects(appDir)).value;
  if (!list.projects.some((p) => p.id === id)) return { ok: false, why: 'no such project' };
  const next: ProjectList = { version: 1, projects: list.projects.filter((p) => p.id !== id) };
  await writeAtomic(path.join(appDir, PROJECTS_FILE), next);
  changed();
  return { ok: true, list: next };
}
