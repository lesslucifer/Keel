// Tailwind 3's context builder ships no types; the rule uses only these members of what it returns.
declare module 'tailwindcss/lib/lib/setupContextUtils.js' {
  interface Context {
    getClassList(options?: { includeMetadata?: boolean }): Array<string | [string, { modifiers?: string[] }]>;
    variantMap: Map<unknown, unknown>;
    candidateRuleMap: Map<unknown, unknown>;
  }
  const utils: { createContext(config: unknown): Context };
  export default utils;
}
