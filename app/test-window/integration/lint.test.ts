// The lint rule at its boundary, ESLint's own API: each fixture under lint-fixtures/ reports its
// message; the real eslint.config.js holds the rule on src/window and the window's own code is clean;
// and Trawler's reconcile check, two oracles in agreement: every whole class the window authors is
// legal to the rule exactly when Tailwind's PostCSS plugin emits CSS for it (in memory, no build).
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { ESLint } from 'eslint';
import postcss from 'postcss';
import tailwindcss from 'tailwindcss';
import tseslint from 'typescript-eslint';
import { expect, test } from 'vitest';
import { arbitraryValue, classify, collectClasses } from '../../eslint-rules/classes.js';
import vocabulary from '../../eslint-rules/vocabulary.js';
import { parseSource } from '../support/parse.ts';
import tailwindConfig from '../../tailwind.config.js';

const APP = path.resolve(import.meta.dirname, '..', '..');
const FIXTURES = path.join(APP, 'test-window', 'lint-fixtures');

/** ESLint with the vocabulary rule alone, on the fixtures (the real config never reads them). */
const fixtureLinter = () =>
  new ESLint({
    cwd: APP,
    overrideConfigFile: true,
    overrideConfig: [
      {
        files: ['test-window/lint-fixtures/*.tsx'],
        linterOptions: { noInlineConfig: true, reportUnusedDisableDirectives: 'error' },
        languageOptions: { parser: tseslint.parser, parserOptions: { ecmaFeatures: { jsx: true } } },
        plugins: { design: { rules: { vocabulary } } },
        rules: { 'design/vocabulary': 'error' },
      },
    ],
  });

async function messagesOf(fixture: string): Promise<Array<{ id: string | undefined; text: string }>> {
  const [result] = await fixtureLinter().lintFiles([path.join(FIXTURES, fixture)]);
  return (result?.messages ?? []).map((m) => ({ id: m.messageId, text: m.message }));
}

test('an off-list class is reported once per class, naming it and the token config', async () => {
  const got = await messagesOf('offlist.tsx');
  expect(got.map((m) => m.id)).toEqual(['offlist', 'offlist']);
  expect(got[0]?.text).toContain('`text-sm`');
  expect(got[1]?.text).toContain('`bg-red-500`');
  expect(got[0]?.text).toContain('app/tailwind.config.js');
});

test('an arbitrary value is reported as arbitrary, naming its value', async () => {
  const got = await messagesOf('arbitrary.tsx');
  expect(got.map((m) => m.id)).toEqual(['arbitrary']);
  expect(got[0]?.text).toContain('`13px`');
});

test('a modifier step not on its scale is reported, naming the steps it has', async () => {
  const got = await messagesOf('modifier.tsx');
  expect(got.map((m) => m.id)).toEqual(['modifier']);
  expect(got[0]?.text).toContain('disabled');
});

test('a class built with + and one cut by an interpolation are each reported as unresolvable', async () => {
  const got = await messagesOf('fragments.tsx');
  expect(got.map((m) => m.id)).toEqual(['unresolvable', 'unresolvable']);
  expect(got[0]?.text).toContain("'p-' + n");
  expect(got[1]?.text).toContain('`gap-`');
});

test('a dark: class is reported as dark even on a legal token', async () => {
  const got = await messagesOf('dark.tsx');
  expect(got.map((m) => m.id)).toEqual(['dark']);
  expect(got[0]?.text).toContain('dark:bg-card');
});

test('classes in tables wrapped in as const, satisfies, as T and ! are each read and refused', async () => {
  const got = await messagesOf('wrapped.tsx');
  expect(got.map((m) => m.id).sort()).toEqual(['arbitrary', 'dark', 'offlist', 'offlist']);
  const text = got.map((m) => m.text).join('\n');
  for (const cls of ['bg-red-500', 'text-sm', 'p-[13px]', 'dark:bg-card']) expect(text).toContain(cls);
});

test('an eslint-disable comment turns nothing off: each class under one is still refused, and the comment itself is flagged', async () => {
  const [result] = await fixtureLinter().lintFiles([path.join(FIXTURES, 'disabled.tsx')]);
  const messages = result?.messages ?? [];
  expect(messages.filter((m) => m.ruleId === 'design/vocabulary').map((m) => m.messageId).sort()).toEqual(['arbitrary', 'dark', 'offlist']);
  expect(messages.filter((m) => m.ruleId === null).length).toBeGreaterThanOrEqual(3);
});

test('the real eslint.config.js has inline config off, and npm run lint fails on any warning', async () => {
  const eslint = new ESLint({ cwd: APP });
  const code = 'export const A = () => (\n  // eslint-disable-next-line design/vocabulary\n  <p className="text-sm">a</p>\n);\n';
  const [result] = await eslint.lintText(code, { filePath: path.join(APP, 'src', 'window', 'planted.tsx') });
  expect(result?.messages.filter((m) => m.ruleId === 'design/vocabulary').map((m) => m.messageId)).toEqual(['offlist']);
  expect(result?.warningCount ?? 0).toBeGreaterThan(0);
  const pkg = JSON.parse(readFileSync(path.join(APP, 'package.json'), 'utf8')) as { scripts: Record<string, string> };
  expect(pkg.scripts.lint).toBe('eslint --max-warnings 0 .');
});

test('the clean fixture reports nothing', async () => {
  expect(await messagesOf('clean.tsx')).toEqual([]);
});

test('the real eslint.config.js holds the rule on src/window: a planted off-list class there is an error', async () => {
  const eslint = new ESLint({ cwd: APP });
  const [result] = await eslint.lintText('export const A = () => <p className="text-sm">a</p>;\n', { filePath: path.join(APP, 'src', 'window', 'planted.tsx') });
  expect(result?.messages.map((m) => [m.ruleId, m.severity, m.messageId])).toEqual([['design/vocabulary', 2, 'offlist']]);
});

test('the real eslint.config.js reports nothing on the whole of src/window', async () => {
  const eslint = new ESLint({ cwd: APP });
  const results = await eslint.lintFiles(['src/window']);
  expect(results.length).toBeGreaterThan(0);
  expect(results.flatMap((r) => r.messages.map((m) => `${path.relative(APP, r.filePath)}:${m.line} ${m.message}`))).toEqual([]);
});

test('reconcile: every whole class the window and the clean fixture author is legal to the rule exactly when Tailwind emits it', async () => {
  const eslint = new ESLint({ cwd: APP });
  const files = (await eslint.lintFiles(['src/window'])).map((r) => r.filePath).filter((f) => /\.tsx?$/.test(f));
  files.push(path.join(FIXTURES, 'clean.tsx'));
  const authored = new Set<string>();
  for (const file of files) {
    const code = readFileSync(file, 'utf8');
    for (const { cls, partial } of collectClasses(parseSource(code, file.endsWith('x')))) if (!partial) authored.add(cls);
  }
  expect(authored.size).toBeGreaterThan(10);
  const raw = [...authored].join(' ');
  const out = await postcss([tailwindcss({ ...tailwindConfig, content: [{ raw, extension: 'html' }] })]).process('@tailwind utilities;', { from: undefined });
  const built = new Set([...out.css.matchAll(/\.((?:\\.|[^\s{},:>~+[\]().])+)/g)].map((m) => (m[1] as string).replace(/\\/g, '')));
  const disagree = [...authored].filter((cls) => classify(cls).legal !== built.has(cls) && arbitraryValue(cls) === null);
  expect(disagree).toEqual([]);
});
