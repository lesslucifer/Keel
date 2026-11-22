// The seven requests' handlers: everything the shell does for the window except Electron itself. The
// shell hands in its parts (the app folder, the PATH it read at start, its two dialogs, its test
// alert and its push); the handlers read and write only the app folder, run git only through the
// fixed commands, and run their reads and writes one at a time, so two changes never interleave.
// Nothing slow holds that queue: neither an open dialog nor a project's git checks (state.get, a
// reload's, answers meanwhile). The reads before them and the checks and write after them each
// queue on their own, and the dialog flows take turns in a queue of their own. The dispatcher (bridge.ts) checks every call before a handler runs and passes
// every string in the answer through plain().

import path from 'node:path';
import { TOOL_NAMES, type AlertOutcome, type AppState, type Change, type LoginPathState, type PushEvent, type Tool, type View } from '../shared/contract.ts';
import type { Handlers } from './bridge.ts';
import { loadSettings, chooseTool, resetTool } from './settings.ts';
import { loadProjects, checkProject, saveProject, removeProject, fullWhy, MAX_PROJECTS } from './projects.ts';
import { toolState, toolStates } from './tools.ts';
import type { ViewReaderParts } from './reader/view.ts';

/** What the Electron shell hands the handlers. */
export interface Parts {
  /** The app's own folder (Electron's userData). */
  appDir: string;
  version: string;
  /** The PATH read once at start: git runs with it and unset tools are looked up on it. */
  loginPath: LoginPathState;
  /** Where git takes HOME, USER, LOGNAME, LANG and TMPDIR from; the app's own environment when unset. */
  env?: Record<string, string | undefined>;
  /** The main part's folder dialog: an absolute path, or null when cancelled. */
  chooseFolder(): Promise<string | null>;
  /** The main part's file dialog for `tool`: an absolute path, or null when cancelled. */
  chooseFile(tool: Tool): Promise<string | null>;
  /** One test notification, and what became of it. */
  testAlert(): Promise<AlertOutcome>;
  /** Push an event to the app's window (bridge.ts's pusher). */
  push(event: PushEvent): void;
  /** The reader's view as it stands (reader/view.ts); reading it runs nothing. */
  view(): View;
}

/** A queue: each job starts once the one before it has settled, passed or failed. */
export function serial(): <T>(job: () => Promise<T>) => Promise<T> {
  let tail: Promise<unknown> = Promise.resolve();
  return <T>(job: () => Promise<T>): Promise<T> => {
    const next = tail.then(job, job);
    tail = next.catch(() => undefined);
    return next;
  };
}

const DONE: Change = Object.freeze({ done: true });
const notDone = (why: string): Change => ({ done: false, why });

/** The set tools' folders: what the PATH's fallback adds to the system PATH. */
function toolDirs(tools: Partial<Record<Tool, string>>): string[] {
  return TOOL_NAMES.map((t) => tools[t]).filter((x): x is string => x !== undefined).map((x) => path.dirname(x));
}

/** At start: load the settings, then read the PATH, handing it the set tools' folders for its fallback. */
export async function prepare(appDir: string, readPath: (toolDirs: readonly string[]) => Promise<LoginPathState>): Promise<LoginPathState> {
  const s = await loadSettings(appDir);
  return readPath(toolDirs(s.value.tools));
}

/** What the view reader needs from the app's folder: the listed project, the usable git, and git's run options. */
export function readerParts(p: Pick<Parts, 'appDir' | 'loginPath' | 'env'>): ViewReaderParts {
  return {
    project: async () => (await loadProjects(p.appDir)).value.projects[0] ?? null,
    git: async () => {
      const settings = (await loadSettings(p.appDir)).value;
      const projects = (await loadProjects(p.appDir)).value.projects;
      const git = await toolState('git', settings, p.loginPath.entries, projects.map((x) => x.path));
      return git.problem === null ? git.path : null;
    },
    runOptions: { path: p.loginPath.entries, ...(p.env === undefined ? {} : { source: p.env }) },
  };
}

/** The handlers the dispatcher runs once a call has passed its checks. */
export function makeHandlers(parts: Parts): Handlers {
  const { appDir } = parts;
  const queue = serial();
  // One dialog flow at a time, outside `queue`: a dialog may stay open as long as the owner likes.
  const dialogs = serial();
  const changed = () => parts.push('state.changed');
  const runOpts = () => ({ path: parts.loginPath.entries, ...(parts.env === undefined ? {} : { source: parts.env }) });

  async function read() {
    const settings = (await loadSettings(appDir)).value;
    const projects = (await loadProjects(appDir)).value.projects;
    const tools = await toolStates(settings, parts.loginPath.entries, projects.map((p) => p.path));
    return { projects, tools };
  }

  return {
    'state.get': () =>
      queue(async (): Promise<AppState> => {
        const { projects, tools } = await read();
        return { version: parts.version, path: parts.loginPath, tools, projects };
      }),

    // Refused before the dialog when the list is full or git is unusable: the owner picks nothing for nothing.
    // After the dialog, git is read again (a setting may have changed while it was open); the folder's git
    // checks run outside the queue (each may take up to git's timeout); saveProject checks the list again.
    'projects.add': () =>
      dialogs(async (): Promise<Change> => {
        const usableGit = async (): Promise<Change | string> => {
          const settings = (await loadSettings(appDir)).value;
          const projects = (await loadProjects(appDir)).value.projects;
          if (projects.length >= MAX_PROJECTS) return notDone(fullWhy({ version: 1, projects }));
          const git = await toolState('git', settings, parts.loginPath.entries, projects.map((p) => p.path));
          return git.path === null || git.problem !== null ? notDone('git was not found: choose it in the settings first') : git.path;
        };
        const before = await queue(usableGit);
        if (typeof before !== 'string') return before;
        const folder = await parts.chooseFolder();
        if (folder === null) return notDone('cancelled');
        const git = await queue(usableGit);
        if (typeof git !== 'string') return git;
        const c = await checkProject(folder, git, runOpts());
        if (!c.ok) return notDone(c.why);
        return queue(async (): Promise<Change> => {
          const r = await saveProject(appDir, c.path, changed);
          return r.ok ? DONE : notDone(r.why);
        });
      }),

    'projects.remove': (args) =>
      queue(async (): Promise<Change> => {
        const r = await removeProject(appDir, args.project as string, changed);
        return r.ok ? DONE : notDone(r.why);
      }),

    'settings.choose': (args) =>
      dialogs(async (): Promise<Change> => {
        const tool = args.tool as Tool;
        const file = await parts.chooseFile(tool);
        if (file === null) return notDone('cancelled');
        return queue(async (): Promise<Change> => {
          // Only the listed projects' paths are needed: the tool check refuses a tool inside one.
          const projects = (await loadProjects(appDir)).value.projects;
          const r = await chooseTool(appDir, tool, file, projects.map((p) => p.path), changed);
          return r.ok ? DONE : notDone(r.why);
        });
      }),

    'settings.reset': (args) =>
      queue(async (): Promise<Change> => {
        await resetTool(appDir, args.tool as Tool, changed);
        return DONE;
      }),

    // Not queued: it touches no file, and a change need not wait 5 s behind an unseen alert.
    'alert.test': () => parts.testAlert(),

    // Not queued: the reader holds the view in memory; asking for it reads no file and runs no git.
    'waiting.get': () => parts.view(),
  };
}
