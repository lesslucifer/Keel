import { test } from 'node:test';
import assert from 'node:assert/strict';
import { APP_PAGE, CSP, csp, pages, withCsp, devServerOf, webPreferences, windowOptions, mayNavigate, openWindow, mayAttachWebview, permissionRequest, permissionCheck, quitWhenAllClosed, onActivate } from '../../src/main/window-options.ts';

test('webPreferences pin every security value of Electron\'s checklist', () => {
  assert.deepEqual(webPreferences('/app/.vite/build/preload.cjs'), {
    preload: '/app/.vite/build/preload.cjs',
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
  });
});

test('webPreferences refuse a preload that is not an absolute path', () => {
  for (const p of ['preload.cjs', '', './preload.cjs', 'file:///x/preload.cjs']) assert.throws(() => webPreferences(p), /preload/, p);
});

test('the window options carry those webPreferences, open hidden, and set no enableBlinkFeatures', () => {
  const o = windowOptions('/app/preload.cjs');
  assert.deepEqual(o.webPreferences, webPreferences('/app/preload.cjs'));
  assert.equal(o.show, false);
  assert.equal(o.title, 'Keel');
  assert.ok(!('enableBlinkFeatures' in o.webPreferences));
  assert.ok(!('enableBlinkFeatures' in o));
});

test('the packaged CSP is exactly the plan\'s: nothing but the app\'s own script, style and images, and no font-src', () => {
  assert.equal(CSP, "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'");
  assert.equal(csp({ packaged: true }), CSP);
  assert.equal(csp({ packaged: true, devServer: 'http://localhost:5173/' }), CSP);
  assert.equal(csp({ packaged: false }), CSP);
});

test('the dev CSP is used only when not packaged, and adds only what Vite\'s dev server and React\'s refresh need', () => {
  assert.equal(
    csp({ packaged: false, devServer: 'http://localhost:5173/' }),
    "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self'; connect-src 'self' ws://localhost:5173; base-uri 'none'; form-action 'none'",
  );
});

test('a dev server that is not http on localhost with a port is refused for the CSP', () => {
  for (const bad of ['http://evil.example:5173/', 'https://localhost:5173/', 'http://localhost:5173/x', 'http://localhost/', "http://localhost:5173/; script-src *", 'ws://localhost:5173', '']) {
    assert.throws(() => csp({ packaged: false, devServer: bad }), /dev server/, bad);
  }
});

test('the window\'s page is app://keel/index.html, and the Vite URL only when not packaged', () => {
  assert.equal(APP_PAGE, 'app://keel/index.html');
  assert.deepEqual(pages({ packaged: true }), [APP_PAGE]);
  assert.deepEqual(pages({ packaged: true, devServer: 'http://localhost:5173/' }), [APP_PAGE]);
  assert.deepEqual(pages({ packaged: false, devServer: 'http://localhost:5173/' }), ['http://localhost:5173/']);
  assert.deepEqual(pages({ packaged: false }), [APP_PAGE]);
  assert.throws(() => pages({ packaged: false, devServer: 'http://evil.example/' }), /dev server/);
});

test('navigation, new windows and webviews are refused, whatever the URL', () => {
  for (const url of ['app://keel/index.html', 'https://example.com/', 'file:///etc/passwd', 'keel://x', 'javascript:alert(1)', '']) {
    assert.equal(mayNavigate(url), false, url);
    assert.deepEqual(openWindow(url), { action: 'deny' }, url);
  }
  assert.equal(mayAttachWebview(), false);
});

test('every permission request and check is denied', () => {
  for (const p of ['notifications', 'media', 'geolocation', 'clipboard-read', 'clipboard-sanitized-write', 'openExternal', 'fullscreen', 'pointerLock', 'unknown']) {
    assert.equal(permissionRequest(p), false, p);
    assert.equal(permissionCheck(p), false, p);
  }
});

test('closing the last window leaves the app running, and the Dock opens the window again or brings it forward', () => {
  assert.equal(quitWhenAllClosed(), false);
  assert.equal(onActivate({ windowOpen: false }), 'open');
  assert.equal(onActivate({ windowOpen: true }), 'show');
});

test('Forge\'s dev URL is taken only when not packaged, as the page URL the window reports (with its root slash)', () => {
  assert.equal(devServerOf(false, 'http://localhost:5173'), 'http://localhost:5173/');
  assert.equal(devServerOf(false, 'http://localhost:5173/'), 'http://localhost:5173/');
  assert.equal(devServerOf(true, 'http://localhost:5173'), undefined);
  assert.equal(devServerOf(false, undefined), undefined);
  for (const bad of ['http://evil.example:5173', 'https://localhost:5173', 'http://localhost:5173/x', 'http://user@localhost:5173', 'not a url']) {
    assert.throws(() => devServerOf(false, bad), /dev server/, bad);
  }
});

const PAGE = (policy: string) => `<!doctype html>\n<html>\n<head>\n<meta http-equiv="Content-Security-Policy" content="${policy}" />\n<title>Keel</title>\n</head>\n<body></body>\n</html>\n`;

test('withCsp() sets the page\'s one CSP meta to the policy and changes nothing else', () => {
  const dev = csp({ packaged: false, devServer: 'http://localhost:5173/' });
  assert.equal(withCsp(PAGE(CSP), dev), PAGE(dev));
  assert.equal(withCsp(PAGE(dev), CSP), PAGE(CSP));
});

test('withCsp() refuses a page with no CSP meta or more than one, and a policy that could break out of the attribute', () => {
  assert.throws(() => withCsp('<html><head></head></html>', CSP), /one CSP meta/);
  assert.throws(() => withCsp(PAGE(CSP) + PAGE(CSP), CSP), /one CSP meta/);
  for (const bad of ['a" onload="x', 'a<b', 'a>b', 'a&b', 'a\nb']) assert.throws(() => withCsp(PAGE(CSP), bad), /policy/, bad);
});
