// Every window test runs after this: jest-dom's matchers on Vitest's expect, and each test's
// rendered tree unmounted after it (Testing Library cleans up by itself only with globals on).
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

afterEach(() => {
  cleanup();
});
