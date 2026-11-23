// A source file parsed the way ESLint's typescript-eslint parser reads it - its tree, its scopes and
// its text - as collectClasses takes it.
import tseslint from 'typescript-eslint';
import type { collectClasses } from '../../eslint-rules/classes.js';

type Parsed = Parameters<typeof collectClasses>[0];
type Parse = (code: string, options: Record<string, unknown>) => { ast: unknown; scopeManager: unknown };

export function parseSource(code: string, jsx = true): Parsed {
  const parser = tseslint.parser as unknown as { parseForESLint: Parse };
  const { ast, scopeManager } = parser.parseForESLint(code, { range: true, loc: true, sourceType: 'module', ecmaFeatures: { jsx } });
  return { ast, scopeManager, text: code } as Parsed;
}
