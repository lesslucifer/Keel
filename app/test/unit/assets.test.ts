import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { resolveAsset } from '../../src/main/assets.ts';

const ROOT = '/Applications/Keel.app/Contents/Resources/app.asar/.vite/renderer/main_window';

test('the page and its files resolve under the renderer folder with their type', () => {
  assert.deepEqual(resolveAsset('app://keel/index.html', ROOT), { file: path.join(ROOT, 'index.html'), type: 'text/html; charset=utf-8' });
  assert.deepEqual(resolveAsset('app://keel/', ROOT), { file: path.join(ROOT, 'index.html'), type: 'text/html; charset=utf-8' });
  assert.deepEqual(resolveAsset('app://keel/assets/index-1a2b.js?v=1#x', ROOT), { file: path.join(ROOT, 'assets', 'index-1a2b.js'), type: 'text/javascript; charset=utf-8' });
  assert.deepEqual(resolveAsset('app://keel/assets/index-1a2b.css', ROOT), { file: path.join(ROOT, 'assets', 'index-1a2b.css'), type: 'text/css; charset=utf-8' });
  assert.deepEqual(resolveAsset('app://keel/a%20b.svg', ROOT), { file: path.join(ROOT, 'a b.svg'), type: 'image/svg+xml' });
  assert.deepEqual(resolveAsset('app://keel/icon.png', ROOT), { file: path.join(ROOT, 'icon.png'), type: 'image/png' });
});

test('a path that climbs out, climbs encoded, is absolute, or holds NUL or a control character is refused', () => {
  for (const url of [
    'app://keel/../etc/passwd', 'app://keel/assets/../../x.js', 'app://keel/%2e%2e/x.js', 'app://keel/%2E%2E/x.js', 'app://keel/.%2e/x.js',
    'app://keel/assets%2f..%2f..%2fx.js', 'app://keel/a%2Fb.js', 'app://keel/a%5cb.js', 'app://keel/a\\b.js', 'app://keel//etc/x.js',
    'app://keel/a.js%00.html', 'app://keel/a\u0000.js', 'app://keel/a%0a.js', 'app://keel/./x.js', 'app://keel/%zz.js', 'app://keel/.env', 'app://keel/assets/.hidden.js',
  ]) {
    assert.equal(resolveAsset(url, ROOT), null, url);
  }
});

test('another scheme or host, or a file type the window does not use, is refused', () => {
  for (const url of ['file:///etc/passwd', 'app://evil/index.html', 'app://KEEL/index.html', 'https://keel/index.html', 'app:keel/index.html', 'app://keel/x.map', 'app://keel/x', 'app://keel/x.sh', 'app://keel/x.woff2', '']) {
    assert.equal(resolveAsset(url, ROOT), null, url);
  }
});

test('a renderer folder that is not absolute is refused outright', () => {
  assert.throws(() => resolveAsset('app://keel/index.html', 'renderer'), /absolute/);
});
