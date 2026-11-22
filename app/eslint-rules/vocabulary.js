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
      unread:
        '`{{cls}}` can carry classes to the page, but the rule cannot read them (a spread, an import, a parameter, a call it does not know). Write the classes in this file as whole strings, chosen with a condition or a lookup table.',
      style:
        'An inline style draws values outside Keel\'s tokens, and the packaged content policy blocks it. Use classes from app/tailwind.config.js.',
      dom: '`{{what}}` writes a class or a style by hand, where the rule cannot read it. Render the class through className in JSX.',
      dark: '`{{cls}}` uses the dark: variant. Dark lives in the tokens: each colour flips by the Mac\'s setting in src/window/styles.css.',
      variant:
        '`{{cls}}` uses the variant `{{value}}:`, which the closed config does not have (no breakpoints, no arbitrary variants).',
    },
  },

  create(context) {
    const sourceCode = context.sourceCode;
    const text = (/** @type {any} */ node) => sourceCode.getText(node);
    /** @param {any} node */
    const dom = (node, /** @type {any} */ what) => context.report({ node, messageId: 'dom', data: { what: text(what) } });
    return {
      /** @param {any} node */
      JSXAttribute(node) {
        if (node.name?.type === 'JSXIdentifier' && node.name.name === 'style') context.report({ node, messageId: 'style' });
      },
      /** @param {any} node */
      AssignmentExpression(node) {
        const left = node.left;
        if (left.type !== 'MemberExpression') return;
        const named = (/** @type {any} */ m, /** @type {string} */ name) => m.type === 'MemberExpression' && !m.computed && m.property.name === name;
        if (named(left, 'className') || named(left.object, 'style') || named(left, 'style')) dom(node, left);
      },
      /** @param {any} node */
      CallExpression(node) {
        const callee = node.callee;
        if (callee.type !== 'MemberExpression' || callee.computed) return;
        const method = callee.property.name;
        const onClassList = callee.object.type === 'MemberExpression' && !callee.object.computed && callee.object.property.name === 'classList';
        const first = node.arguments[0];
        const attr = first?.type === 'Literal' && typeof first.value === 'string' ? first.value.toLowerCase() : null;
        if (onClassList && ['add', 'remove', 'toggle', 'replace'].includes(method)) dom(node, callee);
        else if (method === 'setAttribute' && (attr === null || attr === 'class' || attr === 'style')) dom(node, callee);
      },
      'Program:exit'(program) {
        const parsed = { ast: /** @type {any} */ (program), scopeManager: sourceCode.scopeManager, text: sourceCode.text };
        for (const { cls, node, offset, partial, unread, style } of collectClasses(parsed)) {
          const loc = { start: sourceCode.getLocFromIndex(offset), end: sourceCode.getLocFromIndex(offset + cls.length) };
          if (partial) {
            context.report({ node: /** @type {any} */ (node), loc, messageId: style ? 'style' : unread ? 'unread' : 'unresolvable', data: { cls } });
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
