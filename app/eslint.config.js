// @ts-check
// ESLint 10, flat config: one rule, design/vocabulary, at error, over the window's source. Test files
// and the lint fixtures are not judged here: the fixtures are linted only by their own test
// (test-window/integration/lint.test.ts), and tests assert on class names deliberately.
import tseslint from 'typescript-eslint';
import vocabulary from './eslint-rules/vocabulary.js';

export default [
  { ignores: ['.vite/**', 'out/**', 'node_modules/**', 'test-window/lint-fixtures/**'] },
  {
    files: ['src/window/**/*.{ts,tsx}'],
    ignores: ['src/window/**/*.test.{ts,tsx}'],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: { design: { rules: { vocabulary } } },
    rules: { 'design/vocabulary': 'error' },
  },
];
