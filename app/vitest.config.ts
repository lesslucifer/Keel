// The window's tests: Vitest from source, in jsdom, with Testing Library (DEC-73). Run by hand
// (npm run test:window), never by keel check, whose node --test reads app/test/ only. This config
// reads app/test-window/ only, so no node test is ever run here either.
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    include: ['test-window/**/*.test.{ts,tsx}'],
    setupFiles: ['test-window/support/setup.ts'],
    globals: false,
  },
});
