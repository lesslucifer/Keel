// settings.json in the app's folder: {version: 1, tools: {keel?, git?, claude?}}. A tool is set only
// from a path the main part's own file dialog returned, and only when the tool check passes. Each
// change saved calls `changed` once (the shell pushes state.changed); a refusal saves nothing.

import path from 'node:path';
import { TOOL_NAMES, type Tool } from '../shared/contract.ts';
import { load, writeAtomic } from './store.ts';
import { toolShapeOk, toolProblem } from './tools.ts';

export const SETTINGS_FILE = 'settings.json';

export interface Settings {
  version: 1;
  tools: Partial<Record<Tool, string>>;
}

export function defaultSettings(): Settings {
  return { version: 1, tools: {} };
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v) && Object.getPrototypeOf(v) === Object.prototype;
}

/** The settings, or null for any shape the app does not know (another version, extra keys, a bad path). */
export function parseSettings(v: unknown): Settings | null {
  if (!isPlainObject(v) || Object.keys(v).sort().join(',') !== 'tools,version' || v.version !== 1) return null;
  const tools = v.tools;
  if (!isPlainObject(tools)) return null;
  const out: Partial<Record<Tool, string>> = {};
  for (const [k, p] of Object.entries(tools)) {
    if (!(TOOL_NAMES as readonly string[]).includes(k) || !toolShapeOk(k as Tool, p)) return null;
    out[k as Tool] = p;
  }
  return { version: 1, tools: out };
}

export function loadSettings(appDir: string, now?: Date): Promise<{ value: Settings; setAside: string | null }> {
  return load(path.join(appDir, SETTINGS_FILE), parseSettings, defaultSettings, now);
}

export async function saveSettings(appDir: string, s: Settings): Promise<void> {
  await writeAtomic(path.join(appDir, SETTINGS_FILE), s);
}

/** Set `tool` to `file` (from the main part's dialog) when the tool check passes; else nothing is saved. */
export async function chooseTool(
  appDir: string,
  tool: Tool,
  file: string,
  projects: readonly string[],
  changed: () => void = () => undefined,
): Promise<{ ok: true; settings: Settings } | { ok: false; why: string }> {
  const why = await toolProblem(tool, file, projects);
  if (why !== null) return { ok: false, why };
  const s = (await loadSettings(appDir)).value;
  const next: Settings = { version: 1, tools: { ...s.tools, [tool]: file } };
  await saveSettings(appDir, next);
  changed();
  return { ok: true, settings: next };
}

/** Drop `tool`'s setting, so it is looked up on the PATH again. */
export async function resetTool(appDir: string, tool: Tool, changed: () => void = () => undefined): Promise<Settings> {
  const s = (await loadSettings(appDir)).value;
  const tools = { ...s.tools };
  delete tools[tool];
  const next: Settings = { version: 1, tools };
  await saveSettings(appDir, next);
  changed();
  return next;
}
