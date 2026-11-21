// The window's page, read as text: its content policy is the packaged one, and it loads one module
// script of the app's own and nothing inline, so the strict policy is what the built page runs under.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { CSP } from '../../src/main/window-options.ts';

const PAGE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'src', 'window', 'index.html');
const html = () => readFileSync(PAGE, 'utf8');

test('the window\'s page carries exactly one CSP meta, and it is the packaged CSP', () => {
  const metas = [...html().matchAll(/<meta\s+http-equiv="Content-Security-Policy"\s+content="([^"]*)"\s*\/?>/gi)];
  assert.equal(metas.length, 1);
  assert.equal(metas[0]?.[1], CSP);
  assert.equal(html().match(/Content-Security-Policy/gi)?.length, 1);
});

test('the window\'s page loads one module script, its own, and holds no inline script, style or remote URL', () => {
  const scripts = [...html().matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)];
  assert.equal(scripts.length, 1);
  assert.match(scripts[0]?.[1] ?? '', /^\s*type="module"\s+src="\.\/main\.tsx"\s*$/);
  assert.equal(scripts[0]?.[2], '');
  assert.doesNotMatch(html(), /<style\b|\sstyle=|\son\w+=|https?:\/\/|\/\/[a-z]/i);
});
