// The three tools the app runs or reports: keel, git, claude. A path is taken as the owner gave it
// (a link's own path, so claude's versioned target still checks) and checked: absolute, named for
// its tool, an existing regular file, executable, and not inside a listed project by its path, by
// its folder's real path (a link inside a project, named through a linked parent folder such as /tmp)
// or by its real path, so a project's own files never become the program the app runs.

import { stat, realpath, access, constants } from 'node:fs/promises';
import path from 'node:path';
import { TOOL_NAMES, type Tool, type ToolState } from '../shared/contract.ts';
import type { Settings } from './settings.ts';

export const PATH_CAP = 1024;
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** The path's shape alone: a string, absolute, within the cap, no control character, its last part the tool's name. */
export function toolShapeOk(tool: Tool, p: unknown): p is string {
  return typeof p === 'string' && p.length <= PATH_CAP && path.isAbsolute(p) && !CONTROL.test(p) && !p.endsWith('/') && path.basename(p) === tool;
}

/** Whether `child` is `parent` or below it, by path segments (never a shared prefix). */
export function inside(child: string, parent: string): boolean {
  const rel = path.relative(parent, child);
  return rel === '' || (rel !== '..' && !rel.startsWith(`..${path.sep}`) && !path.isAbsolute(rel));
}

/** Why `p` is not a usable `tool`, or null. `projects` are the listed projects' real paths. */
export async function toolProblem(tool: Tool, p: string, projects: readonly string[]): Promise<string | null> {
  if (typeof p !== 'string' || !path.isAbsolute(p)) return 'the path must be absolute';
  if (!toolShapeOk(tool, p)) return `the program must be named ${tool}`;
  let real: string;
  try {
    real = await realpath(p);
  } catch {
    return 'not found';
  }
  if (!(await stat(real)).isFile()) return 'not a file';
  try {
    await access(real, constants.X_OK);
  } catch {
    return 'not executable';
  }
  // The path itself with every folder above it resolved, the last part left as named (it may be a link).
  const viaRealFolder = path.join(await realpath(path.dirname(p)), path.basename(p));
  for (const project of projects) {
    if (inside(p, project) || inside(viaRealFolder, project) || inside(real, project)) return `inside the project ${path.basename(project)}`;
  }
  return null;
}

/** The first `tool` in the PATH entries that passes the check, or null. */
export async function lookup(tool: Tool, entries: readonly string[], projects: readonly string[]): Promise<string | null> {
  for (const dir of entries) {
    if (!path.isAbsolute(dir)) continue;
    const candidate = path.join(dir, tool);
    if ((await toolProblem(tool, candidate, projects)) === null) return candidate;
  }
  return null;
}

/** One tool's state: a set path checked where it is; an unset one looked up on the PATH. */
export async function toolState(tool: Tool, settings: Settings, entries: readonly string[], projects: readonly string[]): Promise<ToolState> {
  const set = settings.tools[tool];
  if (set !== undefined) return { path: set, source: 'set', problem: await toolProblem(tool, set, projects) };
  const found = await lookup(tool, entries, projects);
  return found === null ? { path: null, source: 'missing', problem: 'not found on the PATH' } : { path: found, source: 'path', problem: null };
}

/** Each tool's state, as toolState. */
export async function toolStates(settings: Settings, entries: readonly string[], projects: readonly string[]): Promise<Record<Tool, ToolState>> {
  const out = {} as Record<Tool, ToolState>;
  for (const tool of TOOL_NAMES) out[tool] = await toolState(tool, settings, entries, projects);
  return out;
}
