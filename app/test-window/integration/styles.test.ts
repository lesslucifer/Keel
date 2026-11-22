// The renderer's Vite config turns styles.css into Keel's tokens through Tailwind: transformed in
// memory by Vite itself (middleware mode, no websocket, no port, no build), it holds the colour
// variables for light and dark, the classes screen 6 authors, and nothing from Tailwind's default scales.
import path from 'node:path';
import { createServer } from 'vite';
import { expect, test } from 'vitest';

const APP = path.resolve(import.meta.dirname, '..', '..');

test('vite.renderer.config.ts runs Tailwind on styles.css: tokens in, defaults out, no font file', async () => {
  const server = await createServer({ configFile: path.join(APP, 'vite.renderer.config.ts'), logLevel: 'silent', server: { middlewareMode: true, hmr: false, ws: false } });
  try {
    const out = await server.transformRequest('/styles.css?direct');
    const css = out?.code ?? '';
    expect(css).toContain('--accent:');
    expect(css).toMatch(/prefers-color-scheme:\s*dark/);
    for (const cls of ['.bg-canvas', '.text-body', '.rounded-panel', '.h-control', '.max-w-column', '.disabled\\:opacity-disabled:disabled']) expect(css, cls).toContain(cls);
    for (const cls of ['.text-sm', '.bg-red-500', '.rounded-lg', '.p-5', '.dark\\:']) expect(css, cls).not.toContain(cls);
    expect(css).not.toMatch(/@font-face|url\(/);
  } finally {
    await server.close();
  }
}, 20_000);
