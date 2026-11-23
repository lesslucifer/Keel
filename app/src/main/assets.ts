// The app:// scheme's resolver: a request URL to a file under the renderer folder, or null. It
// refuses rather than normalises: any climbing, encoded slash or backslash, empty or dot segment,
// hidden file, NUL or control character (raw or encoded), other host, or a file type the window
// does not use.

import path from 'node:path';

const PREFIX = 'app://keel/';

const TYPES: Readonly<Record<string, string>> = Object.freeze(
  Object.assign(Object.create(null) as Record<string, string>, {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.css': 'text/css; charset=utf-8',
    '.svg': 'image/svg+xml',
    '.png': 'image/png',
  }),
);

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** The file an app:// URL names under `root` (absolute) and its content type, or null when refused. */
export function resolveAsset(url: string, root: string): { file: string; type: string } | null {
  if (!path.isAbsolute(root)) throw new Error('the renderer folder must be an absolute path');
  if (typeof url !== 'string' || !url.startsWith(PREFIX)) return null;
  let rest = url.slice(PREFIX.length);
  const cut = rest.search(/[?#]/);
  if (cut >= 0) rest = rest.slice(0, cut);
  if (rest === '') rest = 'index.html';
  if (/%(2f|5c)/i.test(rest) || rest.includes('\\') || CONTROL.test(rest)) return null;
  const segments: string[] = [];
  for (const raw of rest.split('/')) {
    let seg: string;
    try {
      seg = decodeURIComponent(raw);
    } catch {
      return null;
    }
    if (seg === '' || seg.startsWith('.') || seg.includes('/') || seg.includes('\\') || CONTROL.test(seg)) return null;
    segments.push(seg);
  }
  const file = path.join(root, ...segments);
  if (!file.startsWith(path.join(root) + path.sep)) return null;
  const type = TYPES[path.extname(file).toLowerCase()];
  return type === undefined ? null : { file, type };
}
