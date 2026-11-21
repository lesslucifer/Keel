import path from 'node:path';
import type { AddressInfo } from 'node:net';
import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import { csp, withCsp } from './src/main/window-options.ts';

// https://vitejs.dev/config
// The window's page lives in src/window/, not at the app's root: Vite serves and builds from there,
// into the folder Forge's plugin reads (.vite/renderer/<name>, from the app's root).
const app = import.meta.dirname;

// The page ships with the packaged CSP. Only while serving under electron-forge start is it swapped
// for the dev one, which adds what Vite's live reload and React's refresh need on this port.
const devCsp: Plugin = {
  name: 'keel:dev-csp',
  apply: 'serve',
  transformIndexHtml: {
    order: 'post',
    handler(html, ctx) {
      const address = ctx.server?.httpServer?.address() as AddressInfo | null | undefined;
      if (address === null || address === undefined || typeof address !== 'object') throw new Error('the dev server has no port yet');
      return withCsp(html, csp({ packaged: false, devServer: `http://localhost:${address.port}/` }));
    },
  },
};

export default defineConfig({
  root: path.join(app, 'src', 'window'),
  build: { outDir: path.join(app, '.vite', 'renderer', 'main_window'), emptyOutDir: true },
  plugins: [react(), devCsp],
});
