// @ts-check
// design/vocabulary: every class the window renders is in Keel's closed vocabulary (DEC-72). Ported
// from Trawler's ui/eslint-rules/vocabulary.js (logic only, read 2026-11-22) without its
// design-allow pragma: Keel's rule has no escape hatch, a needed value becomes a token. Tailwind
// drops an unknown utility silently; this rule is the alarm.

import { classify, collectClasses } from './classes.js';

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description:
        "Every class must be one of Keel's tokens in the closed Tailwind config (DEC-72): no off-list class, no arbitrary value, no class built at run time, no dark: variant.",
    },
    schema: [],
    messages: {
      offlist:
        '`{{cls}}` is not in the closed vocabulary: Tailwind emits no CSS for it and the element renders unstyled. Use a token from app/tailwind.config.js.',
      arbitrary:
        '`{{cls}}` is an arbitrary value. Name `{{value}}` as a token in app/tailwind.config.js instead.',
      modifier:
        '`{{cls}}` modifies `{{owner}}` with `/{{value}}`, a step that scale does not carry ({{steps}}). Tailwind emits no CSS for the pair, not even `{{owner}}`.',
      unresolvable:
        '`{{cls}}` is built from pieces while the app runs, so no check can read it. Write whole class names, chosen with a condition or a lookup table.',
      dark: '`{{cls}}` uses the dark: variant. Dark lives in the tokens: each colour flips by the Mac\'s setting in src/window/styles.css.',
      variant:
        '`{{cls}}` uses the variant `{{value}}:`, which the closed config does not have (no breakpoints, no arbitrary variants).',
    },
  },

  create(context) {
    const sourceCode = context.sourceCode;
    return {
      'Program:exit'(program) {
        for (const { cls, node, offset, partial } of collectClasses(/** @type {any} */ (program), { text: sourceCode.text })) {
          const loc = { start: sourceCode.getLocFromIndex(offset), end: sourceCode.getLocFromIndex(offset + cls.length) };
          if (partial) {
            context.report({ node: /** @type {any} */ (node), loc, messageId: 'unresolvable', data: { cls } });
            continue;
          }
          const verdict = classify(cls);
          if (verdict.legal) continue;
          context.report({
            node: /** @type {any} */ (node),
            loc,
            messageId: verdict.kind,
            data: { cls, value: verdict.value ?? '', owner: verdict.owner ?? '', steps: verdict.steps?.join(', ') ?? '' },
          });
        }
      },
    };
  },
};
