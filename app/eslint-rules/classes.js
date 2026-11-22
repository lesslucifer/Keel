// @ts-check
// The class vocabulary's reader, ported from Trawler's ui/eslint-rules/classes.js (logic only, read
// 2026-11-22) onto Keel's tokens (../tailwind.config.js). Keel's own additions: a variant the closed
// config lacks is refused, and the dark: variant above all (dark lives in the tokens, DEC-72); a
// class built with + is reported like one cut by a template's interpolation; a cx() or clsx()
// call's arguments are read as classes. Trawler's design-allow pragma is not ported: no escape hatch.

import resolveConfig from 'tailwindcss/resolveConfig.js';
import contextUtils from 'tailwindcss/lib/lib/setupContextUtils.js';
import tailwindConfig from '../tailwind.config.js';

/** @typedef {{ type: string, [key: string]: any }} Node */
/** @typedef {{ cls: string, offset: number, node: Node, partial: boolean }} Found */

const theme = /** @type {Record<string, unknown>} */ (tailwindConfig.theme ?? {});
const resolved = /** @type {Record<string, unknown>} */ (resolveConfig(tailwindConfig).theme);

// A scale still inheriting Tailwind's default is invisible to this rule: every member emits, so
// nothing drawn from it could ever be refused. Legality is membership of a scale Keel DECLARES, so
// every scale it leaves undeclared and unchanged is emptied here.
const stock = /** @type {Record<string, unknown>} */ (resolveConfig({ content: [] }).theme);
const declared = new Set(Object.keys(theme));
export const inheritedScales = Object.keys(stock).filter(
  (key) => !declared.has(key) && JSON.stringify(resolved[key]) === JSON.stringify(stock[key]),
);

/**
 * Tailwind's context for the closed config: Keel's tokens, every inherited scale emptied, plus
 * `emptied` (a test empties one more to see what that scale owns). Every reader builds it here.
 * @param {Record<string, unknown>} [emptied]
 */
export function closedContext(emptied = {}) {
  return contextUtils.createContext(
    resolveConfig({
      ...tailwindConfig,
      theme: {
        ...theme,
        ...Object.fromEntries(inheritedScales.map((key) => [key, {}])),
        ...emptied,
      },
    }),
  );
}

const context = closedContext();

/** Every class the closed config can emit, listed by Tailwind itself, never transcribed. */
export const emittableClasses = /** @type {string[]} */ (context.getClassList());
const EMITTABLE = new Set(emittableClasses);

// A /step draws from its own base's scale: bg-ink/disabled dims a colour; text-body/none would set
// a line height; Tailwind emits nothing at all for a pairing its scale lacks.
/** @type {Map<string, Set<string>>} */
export const modifierSteps = new Map();
for (const entry of context.getClassList({ includeMetadata: true })) {
  if (!Array.isArray(entry)) continue;
  const [names, meta] = entry;
  for (const name of [names].flat()) modifierSteps.set(name, new Set(meta?.modifiers ?? []));
}

/** The variants the closed config knows (hover, disabled, focus-visible...); no breakpoint, since there are none. */
const VARIANTS = new Set([...context.variantMap.keys()].filter((k) => typeof k === 'string'));

const ARBITRARY = /\[([^\]]*)\]/;

/** The top-level colon positions of `cls`, outside brackets. */
function colons(/** @type {string} */ cls) {
  const at = [];
  let depth = 0;
  for (let i = 0; i < cls.length; i += 1) {
    const c = cls[i];
    if (c === '[') depth += 1;
    else if (c === ']') depth -= 1;
    else if (c === ':' && depth === 0) at.push(i);
  }
  return at;
}

/** The utility of `cls`: after the last top-level colon, the important mark dropped. */
export function stripVariants(/** @type {string} */ cls) {
  const at = colons(cls);
  const base = at.length === 0 ? 0 : /** @type {number} */ (at[at.length - 1]) + 1;
  return cls.slice(base).replace(/^!/, '');
}

/** The variants of `cls`, in order, each whole (an arbitrary one with its brackets). */
export function variantsOf(/** @type {string} */ cls) {
  const out = [];
  let start = 0;
  for (const i of colons(cls)) {
    out.push(cls.slice(start, i));
    start = i + 1;
  }
  return out;
}

/** The bracketed value in the utility of `cls`, or null. */
export function arbitraryValue(/** @type {string} */ cls) {
  const match = ARBITRARY.exec(stripVariants(cls));
  return match ? /** @type {string} */ (match[1]) : null;
}

/**
 * Whether `cls` is in the closed vocabulary, and if not why: dark, variant, arbitrary, modifier, offlist.
 * @param {string} cls
 * @returns {{ legal: true } | { legal: false, kind: string, value?: string, owner?: string, steps?: string[] }}
 */
export function classify(cls) {
  const variants = variantsOf(cls);
  if (variants.includes('dark')) return { legal: false, kind: 'dark' };
  const unknown = variants.find((v) => !VARIANTS.has(v));
  if (unknown !== undefined) return { legal: false, kind: 'variant', value: unknown };

  const base = stripVariants(cls);
  if (base === '' || EMITTABLE.has(base)) return { legal: true };

  const arbitrary = ARBITRARY.exec(base);
  if (arbitrary) return { legal: false, kind: 'arbitrary', value: /** @type {string} */ (arbitrary[1]) };

  const slash = base.lastIndexOf('/');
  if (slash > 0) {
    const owner = base.slice(0, slash);
    const step = base.slice(slash + 1);
    const steps = modifierSteps.get(owner);
    if (steps?.size) {
      return steps.has(step) ? { legal: true } : { legal: false, kind: 'modifier', value: step, owner, steps: [...steps] };
    }
  }
  return { legal: false, kind: 'offlist' };
}

// ---- Finding the classes in a file ----

/** The functions whose arguments are classes (clsx and its kin). */
const CLASS_CALLS = new Set(['cx', 'clsx', 'classNames']);

const isClassCall = (/** @type {Node} */ node) =>
  node.type === 'CallExpression' && node.callee?.type === 'Identifier' && CLASS_CALLS.has(node.callee.name);

// A string becomes a class only by being rendered, so the search follows value positions and stops at
// the ones that cannot produce one (a condition's test, a call's arguments, a key, a property name).
/** @type {Record<string, string[]>} */
const NON_VALUE_KEYS = {
  ConditionalExpression: ['test'],
  IfStatement: ['test'],
  SwitchStatement: ['discriminant'],
  SwitchCase: ['test'],
  WhileStatement: ['test'],
  DoWhileStatement: ['test'],
  ForStatement: ['test'],
  CallExpression: ['arguments'],
  NewExpression: ['arguments'],
  Property: ['key'],
  MemberExpression: ['property'],
};

/** The keys of `node` the search does not follow; a class call's arguments are followed. */
function ignoredKeys(/** @type {Node} */ node) {
  if (isClassCall(node)) return [];
  return NON_VALUE_KEYS[node.type] ?? [];
}

/** A + that builds a string: one side (or a nested +) holds a string or a template literal. */
function isConcat(/** @type {Node} */ node) {
  if (node.type !== 'BinaryExpression' || node.operator !== '+') return false;
  /** @param {Node} n @returns {boolean} */
  const stringy = (n) =>
    (n.type === 'Literal' && typeof n.value === 'string') || n.type === 'TemplateLiteral' || isConcat(n);
  return stringy(node.left) || stringy(node.right);
}

const notAValue = (/** @type {Node} */ node) =>
  (node.type === 'BinaryExpression' && !isConcat(node)) ||
  node.type === 'ImportDeclaration' ||
  node.type === 'JSXText' ||
  node.type.startsWith('TS') ||
  (node.type === 'JSXAttribute' && node.name?.name !== 'className');

/**
 * Visit `node` and its descendants, skipping what `skip` refuses and, when skipping, the keys that hold no value.
 * @param {Node | null | undefined} node
 * @param {(n: Node) => void} visit
 * @param {(n: Node) => boolean} [skip]
 */
function walk(node, visit, skip) {
  if (!node || typeof node.type !== 'string') return;
  if (skip?.(node)) return;
  visit(node);
  const ignored = skip ? ignoredKeys(node) : [];
  for (const key of Object.keys(node)) {
    if (key === 'parent' || ignored.includes(key)) continue;
    const child = node[key];
    if (Array.isArray(child)) child.forEach((c) => walk(c, visit, skip));
    else walk(child, visit, skip);
  }
}

function referencedNames(/** @type {Node} */ node) {
  const names = new Set();
  walk(
    node,
    (n) => {
      if (n.type === 'Identifier') names.add(n.name);
    },
    notAValue,
  );
  return names;
}

/** The whitespace-separated tokens of `text`, each with its offset in the file. */
function tokensOf(/** @type {string} */ text, /** @type {number} */ textStart) {
  const found = [];
  const pattern = /\S+/g;
  let match = pattern.exec(text);
  while (match) {
    found.push({ cls: match[0], offset: textStart + match.index });
    match = pattern.exec(text);
  }
  return found;
}

// A class the rule cannot resolve is reported, never skipped: a quasi running into an interpolation
// holds half a class name, and half a class name is how an off-list value hides from a static check.
/** @returns {Found[]} */
function templateTokens(/** @type {Node} */ node) {
  /** @type {Found[]} */
  const found = [];
  node.quasis.forEach((/** @type {Node} */ quasi, /** @type {number} */ i) => {
    const text = quasi.value.cooked ?? quasi.value.raw;
    const tokens = tokensOf(text, quasi.range[0] + 1);
    const openEnd = i < node.quasis.length - 1 && !/\s$/.test(text);
    const openStart = i > 0 && !/^\s/.test(text);
    tokens.forEach((token, j) => {
      const partial = (openStart && j === 0) || (openEnd && j === tokens.length - 1);
      found.push({ ...token, node: quasi, partial });
    });
  });
  return found;
}

/**
 * Every class token a file can render: className values seed the search, each name they reach is
 * followed to its declarations, transitively (classes held in constants and lookup tables count).
 * A class cut by an interpolation or built with + comes back with partial: true.
 * @param {Node} program
 * @param {{ text?: string }} [source] the file's text, to quote a + expression whole
 * @returns {Found[]}
 */
export function collectClasses(program, source = {}) {
  /** @type {Map<string, Node[]>} */
  const declarations = new Map();
  /** @type {Node[]} */
  const seeds = [];
  const wanted = new Set();

  // Every declaration of a name is kept: two functions each holding `const cls = ...` are both read.
  const declare = (/** @type {string} */ name, /** @type {Node} */ node) => {
    const bound = declarations.get(name);
    if (bound) bound.push(node);
    else declarations.set(name, [node]);
  };

  walk(program, (node) => {
    if (node.type === 'VariableDeclarator' && node.id.type === 'Identifier' && node.init) declare(node.id.name, node.init);
    if (node.type === 'FunctionDeclaration' && node.id) declare(node.id.name, node.body);
    if (node.type === 'JSXAttribute' && node.name?.name === 'className' && node.value) {
      seeds.push(node.value);
      referencedNames(node.value).forEach((n) => wanted.add(n));
    }
  });

  const roots = [...seeds];
  const followed = new Set();
  const pending = [...wanted];
  while (pending.length > 0) {
    const name = pending.pop();
    if (followed.has(name)) continue;
    followed.add(name);
    const bound = declarations.get(name);
    if (!bound) continue;
    roots.push(...bound);
    bound.forEach((init) => pending.push(...[...referencedNames(init)].filter((n) => !followed.has(n))));
  }

  /** @type {Found[]} */
  const found = [];
  const visited = new Set();
  const quote = (/** @type {Node} */ node) =>
    source.text !== undefined ? source.text.slice(node.range[0], node.range[1]) : sourceOf(node);
  const skip = (/** @type {Node} */ node) => {
    if (notAValue(node)) return true;
    if (isConcat(node)) {
      if (!visited.has(node)) {
        visited.add(node);
        found.push({ cls: quote(node), offset: node.range[0], node, partial: true });
      }
      return true;
    }
    return false;
  };
  roots.forEach((root) =>
    walk(
      root,
      (node) => {
        if (visited.has(node)) return;
        visited.add(node);
        if (isClassCall(node)) {
          for (const arg of node.arguments) {
            if (arg.type !== 'ObjectExpression') continue;
            for (const prop of arg.properties) {
              if (prop.type === 'Property' && prop.key.type === 'Literal' && typeof prop.key.value === 'string') {
                tokensOf(prop.key.value, prop.key.range[0] + 1).forEach((t) => found.push({ ...t, node: prop.key, partial: false }));
              }
            }
          }
        } else if (node.type === 'Literal' && typeof node.value === 'string') {
          if (node.parent?.type === 'Property' && node.parent.key === node && isClassCall(node.parent.parent?.parent ?? {})) return;
          tokensOf(node.value, node.range[0] + 1).forEach((t) => found.push({ ...t, node, partial: false }));
        } else if (node.type === 'TemplateLiteral') {
          templateTokens(node).forEach((t) => found.push(t));
        }
      },
      skip,
    ),
  );
  return found;
}

/** A + expression rebuilt from its parts when the file's text is not at hand (strings quoted as written). */
function sourceOf(/** @type {Node} */ node) {
  /** @param {Node} n @returns {string} */
  const text = (n) => {
    if (n.type === 'BinaryExpression') return `${text(n.left)} ${n.operator} ${text(n.right)}`;
    if (n.type === 'Literal') return n.raw ?? JSON.stringify(n.value);
    if (n.type === 'Identifier') return n.name;
    return '…';
  };
  return text(node);
}
