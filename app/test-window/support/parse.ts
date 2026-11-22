// A source file parsed the way ESLint's typescript-eslint parser reads it, typed for collectClasses.
import tseslint from 'typescript-eslint';
import type { collectClasses } from '../../eslint-rules/classes.js';

type Program = Parameters<typeof collectClasses>[0];
type Parse = (code: string, options: Record<string, unknown>) => { ast: unknown };

export function parseSource(code: string, jsx = true): Program {
  const parser = tseslint.parser as unknown as { parseForESLint: Parse };
  return parser.parseForESLint(code, { range: true, loc: true, ecmaFeatures: { jsx } }).ast as Program;
}
