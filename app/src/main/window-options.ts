// The window's options and policies, pinned here so a unit test holds each one (Electron's security
// checklist): no Node in the window, isolation and the sandbox on, a strict CSP, the app's own page
// only, every navigation, new window, webview and permission refused - and the app keeps running
// when its window closes (DEC-77: alerts still come).

import path from 'node:path';

/** The app's own page, served by the app:// scheme (never file://). */
export const APP_PAGE = 'app://keel/index.html';

/** The packaged CSP: the app's own script, style and images; nothing else (no font-src: the system font, DEC-72). */
export const CSP = ["default-src 'none'", "script-src 'self'", "style-src 'self'", "img-src 'self'", "base-uri 'none'", "form-action 'none'"].join('; ');

/** Vite's dev server under electron-forge start: http on localhost, a port, the root path. */
const DEV_SERVER = /^http:\/\/localhost:(\d{1,5})\/$/;

export interface Where {
  /** app.isPackaged */
  packaged: boolean;
  /** Forge's MAIN_WINDOW_VITE_DEV_SERVER_URL, set only under electron-forge start. */
  devServer?: string;
}

function devPort(devServer: string): string {
  const m = DEV_SERVER.exec(devServer);
  if (m === null) throw new Error(`refused dev server: ${JSON.stringify(devServer)}`);
  return m[1] as string;
}

/**
 * The CSP for where the app runs. Packaged, or with no dev server: the packaged one. Unpackaged
 * under Vite's dev server only: React's refresh preamble is an inline script, Vite injects styles
 * as <style> elements, and its live reload talks over a localhost websocket - those three and no more.
 */
export function csp(where: Where): string {
  if (where.packaged || where.devServer === undefined) return CSP;
  const port = devPort(where.devServer);
  return [
    "default-src 'none'",
    "script-src 'self' 'unsafe-inline'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self'",
    `connect-src 'self' ws://localhost:${port}`,
    "base-uri 'none'",
    "form-action 'none'",
  ].join('; ');
}

/**
 * Forge's MAIN_WINDOW_VITE_DEV_SERVER_URL as the page URL the window reports (root slash added), and
 * only when not packaged; undefined in a build. Anything but http on localhost with a port throws.
 */
export function devServerOf(packaged: boolean, raw: string | undefined): string | undefined {
  if (packaged || raw === undefined) return undefined;
  let href: string;
  try {
    href = new URL(raw).href;
  } catch {
    throw new Error(`refused dev server: ${JSON.stringify(raw)}`);
  }
  devPort(href);
  return href;
}

const CSP_META = /(<meta\s+http-equiv="Content-Security-Policy"\s+content=")([^"]*)("\s*\/?>)/gi;

/**
 * `html` with its one CSP meta tag's content set to `policy`. The page ships with the packaged CSP;
 * the renderer's Vite config swaps in the dev one only while serving under electron-forge start.
 */
export function withCsp(html: string, policy: string): string {
  if (/["<>&\u0000-\u001f\u007f]/.test(policy)) throw new Error('refused policy: it holds a double quote, a bracket, an ampersand or a control character');
  const found = html.match(CSP_META)?.length ?? 0;
  if (found !== 1) throw new Error(`the page must hold one CSP meta tag; it holds ${found}`);
  return html.replace(CSP_META, (_m, open: string, _old: string, close: string) => `${open}${policy}${close}`);
}

/** The pages the window may show and ask from: the app page; unpackaged under Vite, the dev server's page instead. */
export function pages(where: Where): string[] {
  if (where.packaged || where.devServer === undefined) return [APP_PAGE];
  devPort(where.devServer);
  return [where.devServer];
}

export interface WebPrefs {
  preload: string;
  nodeIntegration: false;
  nodeIntegrationInWorker: false;
  nodeIntegrationInSubFrames: false;
  contextIsolation: true;
  sandbox: true;
  webSecurity: true;
  allowRunningInsecureContent: false;
  experimentalFeatures: false;
  webviewTag: false;
  navigateOnDragDrop: false;
  spellcheck: false;
  safeDialogs: true;
}

/** The window's webPreferences; `preload` must be the absolute path of the built preload script. */
export function webPreferences(preload: string): WebPrefs {
  if (typeof preload !== 'string' || !path.isAbsolute(preload)) throw new Error('the preload must be an absolute path');
  return {
    preload,
    nodeIntegration: false,
    nodeIntegrationInWorker: false,
    nodeIntegrationInSubFrames: false,
    contextIsolation: true,
    sandbox: true,
    webSecurity: true,
    allowRunningInsecureContent: false,
    experimentalFeatures: false,
    webviewTag: false,
    navigateOnDragDrop: false,
    spellcheck: false,
    safeDialogs: true,
  };
}

/** The BrowserWindow options: hidden until ready to show, the pinned webPreferences. */
export function windowOptions(preload: string) {
  return { width: 760, height: 560, minWidth: 520, minHeight: 400, title: 'Keel', show: false, webPreferences: webPreferences(preload) };
}

/** will-navigate and will-frame-navigate: the window never leaves its page. */
export function mayNavigate(_url: string): boolean {
  return false;
}

/** setWindowOpenHandler: no new windows. */
export function openWindow(_url: string): { action: 'deny' } {
  return { action: 'deny' };
}

/** will-attach-webview: no webviews. */
export function mayAttachWebview(): boolean {
  return false;
}

/** The session's permission request handler: every permission denied. */
export function permissionRequest(_permission: string): boolean {
  return false;
}

/** The session's permission check handler: every permission denied. */
export function permissionCheck(_permission: string): boolean {
  return false;
}

/** window-all-closed: the app does not quit with its last window (DEC-77); alerts still come. */
export function quitWhenAllClosed(): boolean {
  return false;
}

/** activate (the Dock icon clicked): open the window again when none is open, else bring it forward. */
export function onActivate(state: { windowOpen: boolean }): 'open' | 'show' {
  return state.windowOpen ? 'show' : 'open';
}
