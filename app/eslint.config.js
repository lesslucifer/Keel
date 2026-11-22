// @ts-check
// ESLint 10, flat config: one rule, design/vocabulary, at error, over the window's script files (the
// one source list, window-sources.js). npm run lint names this file (-c), so a nested config cannot
// replace it, and a test refuses any other eslint config under app/. The rule is the fast helper for
// honest code; the guarantee is the CSS check that runs after it (eslint-rules/check-css.js). The lint
// fixtures are linted only by their own test (test-window/integration/lint.test.ts). Inline
// config is off: an eslint-disable comment turns nothing off (Keel's rule has no escape hatch), and
// npm run lint fails on the warning ESLint gives for one (--max-warnings 0).
import tseslint from 'typescript-eslint';
import vocabulary from './eslint-rules/vocabulary.js';
import { SCRIPT_GLOBS } from './window-sources.js';

export default [
  { ignores: ['.vite/**', 'out/**', 'node_modules/**', 'test-window/lint-fixtures/**'] },
  { linterOptions: { noInlineConfig: true, reportUnusedDisableDirectives: 'error' } },
  {
    files: SCRIPT_GLOBS,
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: { design: { rules: { vocabulary } } },
    rules: { 'design/vocabulary': 'error' },
  },
];
