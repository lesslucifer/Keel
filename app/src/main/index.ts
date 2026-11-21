// The Electron shell: the only main-part file that imports electron. It wires Electron to the pure
// modules and holds no logic of its own worth a test: one instance; the app:// scheme, privileged and
// served by the asset resolver (never file://); one window built from window-options; every
// navigation, new window, webview and permission refused; the request channel through the bridge's
// dispatcher, and the push channel to the app's window only; the two dialogs and the Notification
// handed to the handlers. The app keeps running with no window (DEC-77). Checked live on the owner's Mac.

import { app, BrowserWindow, dialog, ipcMain, Notification, protocol, session, type OpenDialogOptions, type WebContents } from 'electron';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { ASK_CHANNEL, type Tool } from '../shared/contract.ts';
import { dispatcher, pusher, type Expected } from './bridge.ts';
import { resolveAsset } from './assets.ts';
import { APP_PAGE, CSP, devServerOf, mayAttachWebview, mayNavigate, onActivate, openWindow, pages, permissionCheck, permissionRequest, quitWhenAllClosed, windowOptions } from './window-options.ts';
import { readLoginPath } from './login-path.ts';
import { makeHandlers, prepare } from './handlers.ts';
import { ALERT_BODY, ALERT_TITLE, testAlert } from './alert.ts';
import { plain } from './text.ts';

// Set by Forge's Vite plugin: the dev server's URL under electron-forge start (undefined in a
// build), and the renderer's name, which names its build folder.
declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string | undefined;
declare const MAIN_WINDOW_VITE_NAME: string;

const SCHEME = 'app';

// Before ready: the app's own scheme, standard (an origin, relative URLs) and secure. It serves
// only the files under the renderer's build folder.
protocol.registerSchemesAsPrivileged([{ scheme: SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true } }]);

const devServer = devServerOf(app.isPackaged, typeof MAIN_WINDOW_VITE_DEV_SERVER_URL === 'string' ? MAIN_WINDOW_VITE_DEV_SERVER_URL : undefined);
const where = { packaged: app.isPackaged, ...(devServer === undefined ? {} : { devServer }) };
const allowedPages = pages(where);
const rendererRoot = path.join(__dirname, '..', 'renderer', MAIN_WINDOW_VITE_NAME);

let mainWindow: BrowserWindow | null = null;
let started = false;

function liveWindow(): BrowserWindow | null {
  return mainWindow !== null && !mainWindow.isDestroyed() ? mainWindow : null;
}

/** Who may ask: the app window's webContents on the app's page; nobody while no window is open. */
function expected(): Expected | null {
  const w = liveWindow();
  return w === null ? null : { contentsId: w.webContents.id, pages: allowedPages };
}

const push = pusher(liveWindow);

function serveApp(): void {
  protocol.handle(SCHEME, async (request) => {
    if (request.method !== 'GET') return new Response(null, { status: 405 });
    const hit = resolveAsset(request.url, rendererRoot);
    if (hit === null) return new Response(null, { status: 404 });
    try {
      const body = await readFile(hit.file);
      return new Response(body, { status: 200, headers: { 'content-type': hit.type, 'content-security-policy': CSP, 'x-content-type-options': 'nosniff' } });
    } catch {
      return new Response(null, { status: 404 });
    }
  });
}

/** Every webContents the app ever makes: no navigation, no redirect, no new window, no webview. */
function lockDown(contents: WebContents): void {
  contents.on('will-navigate', (event, url) => {
    if (!mayNavigate(url)) event.preventDefault();
  });
  contents.on('will-frame-navigate', (event) => {
    if (!mayNavigate(event.url)) event.preventDefault();
  });
  contents.on('will-redirect', (event, url) => {
    if (!mayNavigate(url)) event.preventDefault();
  });
  contents.on('will-attach-webview', (event) => {
    if (!mayAttachWebview()) event.preventDefault();
  });
  contents.setWindowOpenHandler(({ url }) => openWindow(url));
}

function refusePermissions(): void {
  const s = session.defaultSession;
  s.setPermissionRequestHandler((_contents, permission, callback) => callback(permissionRequest(permission)));
  s.setPermissionCheckHandler((_contents, permission) => permissionCheck(permission));
  s.setDevicePermissionHandler(() => false);
}

function openMain(): void {
  const open = liveWindow();
  if (onActivate({ windowOpen: open !== null }) === 'show' && open !== null) {
    if (open.isMinimized()) open.restore();
    open.show();
    open.focus();
    return;
  }
  const win = new BrowserWindow(windowOptions(path.join(__dirname, 'preload.cjs')));
  mainWindow = win;
  win.once('ready-to-show', () => win.show());
  win.on('closed', () => {
    if (mainWindow === win) mainWindow = null;
  });
  void win.loadURL(devServer ?? APP_PAGE);
}

async function showOpen(options: OpenDialogOptions): Promise<string | null> {
  const w = liveWindow();
  const r = w !== null ? await dialog.showOpenDialog(w, options) : await dialog.showOpenDialog(options);
  const file = r.filePaths[0];
  return r.canceled || r.filePaths.length !== 1 || file === undefined || !path.isAbsolute(file) ? null : file;
}

function chooseFolder(): Promise<string | null> {
  return showOpen({ title: 'Add a Keel project', message: "Choose a Keel project's main checkout", buttonLabel: 'Add', properties: ['openDirectory'] });
}

function chooseFile(tool: Tool): Promise<string | null> {
  // noResolveAliases: a link is taken as the owner chose it, so claude's versioned target still checks.
  return showOpen({ title: `Choose ${tool}`, message: `Choose the ${tool} program`, buttonLabel: 'Choose', properties: ['openFile', 'showHiddenFiles', 'noResolveAliases'] });
}

function notify() {
  return testAlert(Notification.isSupported(), () => new Notification({ title: ALERT_TITLE, body: ALERT_BODY }));
}

async function start(): Promise<void> {
  serveApp();
  refusePermissions();
  const appDir = app.getPath('userData');
  const loginPath = await prepare(appDir, (toolDirs) => readLoginPath({ toolDirs }));
  const handlers = makeHandlers({
    appDir,
    version: app.getVersion(),
    loginPath,
    chooseFolder,
    chooseFile,
    testAlert: notify,
    push: (event) => void push(event),
  });
  ipcMain.handle(ASK_CHANNEL, dispatcher(expected, handlers));
  started = true;
  openMain();
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('web-contents-created', (_event, contents) => lockDown(contents));
  app.on('second-instance', () => {
    if (started) openMain();
  });
  // The Dock icon clicked: the window comes back, or comes forward.
  app.on('activate', () => {
    if (started) openMain();
  });
  // The app stays running with no window, so alerts still come (DEC-77); Cmd-Q quits it.
  app.on('window-all-closed', () => {
    if (quitWhenAllClosed()) app.quit();
  });
  // The keel link scheme is declared in the bundle now and handled in W5; until then a link opens nothing.
  app.on('open-url', (event) => event.preventDefault());
  app
    .whenReady()
    .then(start)
    .catch((e: unknown) => {
      dialog.showErrorBox('Keel could not start', plain(e instanceof Error ? e.message : String(e), 500));
      app.quit();
    });
}
