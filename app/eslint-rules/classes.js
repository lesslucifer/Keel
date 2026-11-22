// @ts-check
// The class vocabulary and the lint's class reader. classify() and the closed context are ported from
// Trawler's ui/eslint-rules/classes.js (logic only, read 2026-11-22) onto Keel's tokens
// (../tailwind.config.js), with Keel's additions: a variant the closed config lacks is refused, and
// the dark: variant above all (dark lives in the tokens, DEC-72). classify() is also what the CSS
// check (emitted.js) judges every emitted class by - that check is the guarantee. collectClasses()
// is the lint's fast helper for honest code: it reads class values through ESLint's scope analysis
// and reports what it cannot read; it is not the guarantee (review round 2). No escape hatch.

import resolveConfig from 'tailwindcss/resolveConfig.js';
import contextUtils from 'tailwindcss/lib/lib/setupContextUtils.js';
import tailwindConfig from '../tailwind.config.js';

/** @typedef {{ type: string, [key: string]: any }} Node */
/** @typedef {{ cls: string, offset: number, node: Node, partial: boolean, unread?: boolean, style?: boolean }} Found */

const theme = /** @type {Record<string, unknown>} */ (tailwindConfig.theme ?? {});
const resolved = /** @type {Record<string, unknown>} */ (resolveConfig(tailwindConfig).theme);

// A scale still inheriting Tailwind's default is invisible to this rule: every member emits, so
// nothing drawn from it could ever be refused. Legality is membership of a scale Keel DECLARES, so
// every scale it leaves undeclared and unchanged is emptied here.
const stock = /** @type {Record<string, unknown>} */ (resolveConfig({ content: [] }).theme);
const declared = new Set(Object.keys(theme));
// A scale Tailwind derives from spacing (width, height, inset, margin, translate, basis...) differs
// from stock once spacing is Keel's, yet still carries Tailwind's own fractions and keywords (w-1/3,
// h-screen, inset-1/2): every undeclared one is emptied too. Found by probing, not listed by hand.
const probe = /** @type {Record<string, unknown>} */ (resolveConfig({ content: [], theme: { spacing: { 'keel-probe': '1px' } } }).theme);
export const spacingDerived = Object.keys(probe).filter((key) => key !== 'spacing' && JSON.stringify(probe[key]).includes('keel-probe'));
export const inheritedScales = Object.keys(stock).filter(
  (key) => !declared.has(key) && (JSON.stringify(resolved[key]) === JSON.stringify(stock[key]) || spacingDerived.includes(key)),
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

/** group, peer, and a named one (group/item), when the closed config has their variants. */
const MARKER = new RegExp(`^(?:${['group', 'peer'].filter((m) => VARIANTS.has(`${m}-hover`)).join('|') || '(?!)'})(?:/[a-z][a-z0-9-]*)?$`);

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
  // The group and peer markers emit no CSS of their own; their variants (group-hover:...) need them.
  if (MARKER.test(base)) return { legal: true };

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

/** @typedef {{ ast: Node, scopeManager: any, text: string }} Parsed */

/** A prop or key that carries classes to the page: class, className, or any name ending in ClassName. */
const CLASS_PROP = /^(?:class|className)$|ClassName$/;

/** The functions whose arguments are classes (clsx and its kin). */
const CLASS_CALLS = new Set(['cx', 'clsx', 'classNames']);

/** The calls that take a props object (React's createElement and cloneElement, the JSX runtime's jsx). */
const PROPS_CALLS = new Map([['createElement', 1], ['cloneElement', 1], ['jsx', 1], ['jsxs', 1], ['jsxDEV', 1]]);

/** The TypeScript nodes that wrap a value (x as const, x satisfies T, x as T, <T>x, x!): read through. */
const VALUE_WRAPPERS = new Set(['TSAsExpression', 'TSSatisfiesExpression', 'TSTypeAssertion', 'TSNonNullExpression']);

/** Calls that write into the table handed to them as their first argument. */
const MUTATING_CALLS = new Set([
  'Object.assign',
  'Object.defineProperty',
  'Object.defineProperties',
  'Object.setPrototypeOf',
  'Reflect.set',
  'Reflect.defineProperty',
  'Reflect.deleteProperty',
  'Reflect.setPrototypeOf',
]);

/** Methods that change the array, map or set they are called on. */
const MUTATING_METHODS = new Set(['push', 'pop', 'shift', 'unshift', 'splice', 'sort', 'reverse', 'fill', 'copyWithin', 'set', 'add', 'delete', 'clear']);

/** The name of an object key or a JSX attribute, when it is a plain name or a string. */
function keyName(/** @type {Node | undefined} */ key) {
  if (!key) return undefined;
  if (key.type === 'Identifier' || key.type === 'JSXIdentifier') return key.name;
  if (key.type === 'Literal' && typeof key.value === 'string') return key.value;
  return undefined;
}

/** The name a callee is called by: f, or obj.f (last part), and its dotted form for Object.keys. */
function calleeName(/** @type {Node} */ callee) {
  if (callee.type === 'Identifier') return { last: callee.name, dotted: callee.name };
  if (callee.type === 'MemberExpression' && !callee.computed && callee.property.type === 'Identifier') {
    const obj = callee.object.type === 'Identifier' ? `${callee.object.name}.` : '';
    return { last: callee.property.name, dotted: `${obj}${callee.property.name}` };
  }
  return { last: '', dotted: '' };
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

/** The values a function returns: its concise body, or each return in its body (not in a nested function). */
function returnsOf(/** @type {Node} */ fn) {
  if (fn.body?.type !== 'BlockStatement') return fn.body ? [fn.body] : [];
  /** @type {Node[]} */
  const out = [];
  const visit = (/** @type {any} */ node) => {
    if (!node || typeof node.type !== 'string') return;
    if (node !== fn.body && /Function/.test(node.type)) return;
    if (node.type === 'ReturnStatement') {
      if (node.argument) out.push(node.argument);
      return;
    }
    for (const key of Object.keys(node)) {
      if (key === 'parent') continue;
      const child = node[key];
      if (Array.isArray(child)) child.forEach(visit);
      else if (child && typeof child.type === 'string') visit(child);
    }
  };
  visit(fn.body);
  return out;
}

/**
 * Every class token a file can render, and what the reader cannot read. Class values are found from
 * each class-carrying JSX attribute, JSX spread, props object (createElement, cloneElement) and
 * class-keyed object literal; every name is resolved through the scope manager (shadowing, parameters,
 * imports and catch variables included) and read with every value written to it; a function is read
 * by what it returns. Found entries: a whole class (partial false); a class built from pieces
 * (partial true: a + or an interpolation glued to text or to another one); a value the reader cannot
 * read (unread: an import, a parameter, a global, a call it does not know, a table written after its
 * declaration, this., a props object by reference, a spread); a style in a props object (style).
 * @param {Parsed} parsed
 * @returns {Found[]}
 */
export function collectClasses(parsed) {
  const { ast, scopeManager, text } = parsed;
  /** @type {Map<Node, Node | null>} */
  const parents = new Map();
  /** @type {Node[]} */
  const all = [];
  const index = (/** @type {any} */ node, /** @type {any} */ parent) => {
    if (!node || typeof node.type !== 'string' || parents.has(node)) return;
    parents.set(node, parent);
    all.push(node);
    for (const key of Object.keys(node)) {
      if (key === 'parent' || key === 'loc' || key === 'range') continue;
      const child = node[key];
      if (Array.isArray(child)) child.forEach((c) => index(c, node));
      else if (child && typeof child.type === 'string') index(child, node);
    }
  };
  index(ast, null);

  /** @type {Map<Node, any>} */
  const refOf = new Map();
  for (const scope of scopeManager.scopes) for (const ref of scope.references) refOf.set(ref.identifier, ref);

  /** @type {Found[]} */
  const found = [];
  const quote = (/** @type {Node} */ node) => text.slice(node.range[0], node.range[1]);
  const piece = (/** @type {Node} */ node) => {
    found.push({ cls: quote(node), offset: node.range[0], node, partial: true });
  };
  const unread = (/** @type {Node} */ node) => {
    found.push({ cls: quote(node), offset: node.range[0], node, partial: true, unread: true });
  };
  const style = (/** @type {Node} */ node) => {
    found.push({ cls: quote(node), offset: node.range[0], node, partial: true, style: true });
  };
  const whole = (/** @type {string} */ value, /** @type {number} */ start, /** @type {Node} */ node) =>
    tokensOf(value, start).forEach((t) => found.push({ ...t, node, partial: false }));

  /** @type {Map<any, boolean>} */
  const taintCache = new Map();
  /** Whether a name holds only strings (or other primitives): nothing done to it later can change them. */
  const primitive = (/** @type {any} */ variable) => {
    const writes = variable.references.filter((/** @type {any} */ ref) => ref.writeExpr);
    return writes.length > 0 && writes.every((/** @type {any} */ ref) => ref.writeExpr.type === 'Literal' || ref.writeExpr.type === 'TemplateLiteral');
  };
  /** The function a callee names when its body is in this file and the name is never reassigned, 'global' for an unresolved name, else null. */
  const functionOf = (/** @type {any} */ callee) => {
    if (callee.type === 'FunctionExpression' || callee.type === 'ArrowFunctionExpression') return callee;
    if (callee.type !== 'Identifier') return null;
    const variable = refOf.get(callee)?.resolved;
    // Unresolved, or a library global with no declaration here (TypeScript's lib: Object, String).
    if (!variable || variable.defs.length === 0) return 'global';
    const def = variable.defs[0];
    if (def?.type === 'FunctionName') return def.node;
    const writes = variable.references.filter((/** @type {any} */ ref) => ref.writeExpr);
    if (def?.type === 'Variable' && writes.length === 1 && /^(?:Arrow)?Function(?:Expression)?$/.test(writes[0].writeExpr.type)) return writes[0].writeExpr;
    return null;
  };
  /** Whether a call changes the table it is handed at argument `at`. */
  const callChanges = (/** @type {any} */ call, /** @type {number} */ at) => {
    const callee = call.callee;
    const dotted = calleeName(callee).dotted;
    if (MUTATING_CALLS.has(dotted)) return at === 0;
    // A mutating method borrowed: Array.prototype.push.call(t, ...), [].push.apply(t, ...), Reflect.apply(Array.prototype.push, t, ...).
    if (callee.type === 'MemberExpression' && ['call', 'apply'].includes(calleeName(callee).last) && callee.object.type === 'MemberExpression') {
      if (MUTATING_METHODS.has(calleeName(callee.object).last)) return at === 0;
    }
    if (dotted === 'Reflect.apply') {
      const fn = call.arguments[0];
      return at === 1 && fn?.type === 'MemberExpression' && MUTATING_METHODS.has(calleeName(fn).last);
    }
    if (callee.type === 'MemberExpression') {
      // A method of a global (console.info, JSON.stringify) reads; of a local or imported object, its body is unseen.
      let root = callee;
      while (root.type === 'MemberExpression') root = root.object;
      const owner = root.type === 'Identifier' ? refOf.get(root)?.resolved : undefined;
      return !(root.type === 'Identifier' && (!owner || owner.defs.length === 0));
    }
    const fn = functionOf(callee);
    if (fn === 'global') return false;
    if (fn === null) return true;
    // Its parameter at `at`, followed: changed there (or anything a pattern or rest takes from it) changes the table.
    for (let i = 0; i < fn.params.length; i += 1) {
      const param = fn.params[i];
      if (i > at && param.type !== 'RestElement') break;
      if (i !== at && param.type !== 'RestElement') continue;
      const [start, end] = param.range;
      const vars = scopeManager.getDeclaredVariables(fn).filter((/** @type {any} */ v) => v.defs.some((/** @type {any} */ d) => d.name.range[0] >= start && d.name.range[1] <= end));
      if (param.type === 'RestElement') return true;
      return vars.some((/** @type {any} */ v) => tainted(v));
    }
    return false;
  };
  /**
   * Whether a variable's table or array is changed after its declaration, so its values cannot be read:
   * a member written, updated or deleted, a mutating method called on it or on a member (or borrowed:
   * Array.prototype.push.call, [].push.apply, Reflect.apply), the table handed to a call that writes
   * into its first argument (Object.assign and kin), to a local function whose parameter is changed
   * (followed through), or to a function whose body is not in this file (an import, a parameter, a
   * method on a local or imported object) - directly or through an alias. A string is never changed;
   * a global (console, JSON, String), a local function that only reads, and an alias nobody writes are
   * reads, so honest code stays silent (review rounds 3 and 4; DEC-72 refuses classes built at run time).
   */
  const tainted = (/** @type {any} */ variable) => {
    if (taintCache.has(variable)) return /** @type {boolean} */ (taintCache.get(variable));
    // Provisionally clean, so an alias cycle (A = B, B = A) ends.
    taintCache.set(variable, false);
    if (primitive(variable)) return false;
    let bad = false;
    for (const ref of variable.references) {
      if (ref.isWrite()) continue;
      let cur = ref.identifier;
      let up = parents.get(cur);
      while (up && ((up.type === 'MemberExpression' && up.object === cur) || VALUE_WRAPPERS.has(up.type) || up.type === 'ChainExpression')) {
        cur = up;
        up = parents.get(cur);
      }
      if (!up) continue;
      if (cur !== ref.identifier && cur.type === 'MemberExpression') {
        if (up.type === 'AssignmentExpression' && up.left === cur) bad = true;
        else if (up.type === 'UpdateExpression' || (up.type === 'UnaryExpression' && up.operator === 'delete')) bad = true;
        else if (up.type === 'CallExpression' && up.callee === cur && MUTATING_METHODS.has(calleeName(cur).last)) bad = true;
      } else if (up.type === 'CallExpression' && up.arguments.includes(cur)) {
        if (callChanges(up, up.arguments.indexOf(cur))) bad = true;
      } else if (up.type === 'VariableDeclarator' && up.init === cur && up.id.type === 'Identifier') {
        // An alias: a read, unless the alias itself is changed.
        const alias = scopeManager.getDeclaredVariables(up)[0];
        if (alias && tainted(alias)) bad = true;
      } else if (up.type === 'AssignmentExpression' && up.right === cur && up.left.type === 'Identifier') {
        const alias = refOf.get(up.left)?.resolved;
        if (alias && tainted(alias)) bad = true;
      }
      if (bad) break;
    }
    taintCache.set(variable, bad);
    return bad;
  };

  /** The values a name can hold, or null when they cannot be read here. */
  const valuesOf = (/** @type {Node} */ id) => {
    if (id.name === 'undefined') return [];
    const variable = refOf.get(id)?.resolved;
    if (!variable || variable.defs.length === 0) return null;
    const def = variable.defs[0];
    // A declared function with no body (declare function tw) returns nothing the reader can see.
    if (def.type === 'FunctionName') return def.node.body ? [def.node] : null;
    if (def.type !== 'Variable') return null;
    const declarator = def.node;
    if (declarator.id.type !== 'Identifier') return null;
    const loop = parents.get(parents.get(declarator) ?? declarator);
    if (loop && (loop.type === 'ForOfStatement' || loop.type === 'ForInStatement')) return null;
    if (tainted(variable)) return null;
    /** @type {Node[]} */
    const values = [];
    for (const ref of variable.references) {
      if (!ref.writeExpr) continue;
      const assign = parents.get(ref.writeExpr);
      if (assign?.type === 'AssignmentExpression' && assign.operator !== '=') return null;
      values.push(ref.writeExpr);
    }
    return values;
  };

  const seen = new Set();

  /** Read `node`, which sits where its value becomes classes. @type {(node: any) => void} */
  const read = (node) => {
    if (!node || seen.has(node)) return;
    seen.add(node);
    if (VALUE_WRAPPERS.has(node.type) || node.type === 'ChainExpression' || node.type === 'JSXExpressionContainer') return read(node.expression);
    switch (node.type) {
      case 'JSXEmptyExpression':
        return;
      case 'Literal':
        if (typeof node.value === 'string') whole(node.value, node.range[0] + 1, node);
        return;
      case 'TemplateLiteral':
        return readTemplate(node);
      case 'ConditionalExpression':
        read(node.consequent);
        return read(node.alternate);
      case 'LogicalExpression':
        // a && 'p-xs' renders a only when it is falsy, never a class: a is a test.
        if (node.operator !== '&&') read(node.left);
        return read(node.right);
      case 'BinaryExpression':
        if (node.operator === '+') piece(node);
        return;
      case 'UnaryExpression':
      case 'UpdateExpression':
        return;
      case 'SequenceExpression':
        return read(node.expressions[node.expressions.length - 1]);
      case 'AssignmentExpression':
        return node.operator === '=' ? read(node.right) : piece(node);
      case 'ArrayExpression':
        for (const el of node.elements) read(el?.type === 'SpreadElement' ? el.argument : el);
        return;
      case 'ObjectExpression':
        // A table: its values are classes, its keys are not.
        for (const prop of node.properties) {
          if (prop.type === 'SpreadElement') read(prop.argument);
          else read(prop.value);
        }
        return;
      case 'Identifier': {
        const values = valuesOf(node);
        if (values === null) return unread(node);
        values.forEach(read);
        return;
      }
      case 'MemberExpression': {
        let root = node;
        while (root.type === 'MemberExpression' || VALUE_WRAPPERS.has(root.type)) root = root.type === 'MemberExpression' ? root.object : root.expression;
        if (root.type !== 'Identifier') return unread(node);
        const values = valuesOf(root);
        if (values === null) return unread(node);
        values.forEach(read);
        return;
      }
      case 'CallExpression':
        return readCall(node);
      case 'FunctionDeclaration':
      case 'FunctionExpression':
      case 'ArrowFunctionExpression':
        returnsOf(node).forEach(read);
        return;
      default:
        return unread(node);
    }
  };

  /** @type {(node: any) => void} */
  const readTemplate = (node) => {
    const last = node.quasis.length - 1;
    node.quasis.forEach((/** @type {any} */ quasi, /** @type {number} */ i) => {
      const value = quasi.value.cooked ?? quasi.value.raw;
      const tokens = tokensOf(value, quasi.range[0] + 1);
      const openEnd = i < last && !/\s$/.test(value);
      const openStart = i > 0 && !/^\s/.test(value);
      tokens.forEach((t, j) => {
        const partial = (openStart && j === 0) || (openEnd && j === tokens.length - 1);
        found.push({ ...t, node: quasi, partial });
      });
    });
    node.expressions.forEach((/** @type {any} */ expr, /** @type {number} */ i) => {
      const before = node.quasis[i].value.cooked ?? node.quasis[i].value.raw;
      const after = node.quasis[i + 1].value.cooked ?? node.quasis[i + 1].value.raw;
      const textGlued = (before !== '' && !/\s$/.test(before)) || (after !== '' && !/^\s/.test(after));
      const exprGlued = (before === '' && i > 0) || (after === '' && i + 1 < last);
      // Glued to text, the text's partial token already says so; glued only to another interpolation, say it here.
      if (textGlued) return;
      if (exprGlued) return piece(expr);
      read(expr);
    });
  };

  /** @type {(node: any) => void} */
  const readCall = (node) => {
    const { last } = calleeName(node.callee);
    if (node.callee.type === 'Identifier' && CLASS_CALLS.has(last)) {
      for (const arg of node.arguments) {
        if (arg.type !== 'ObjectExpression') {
          read(arg.type === 'SpreadElement' ? arg.argument : arg);
          continue;
        }
        // The keys are classes; the values are only conditions.
        for (const prop of arg.properties) {
          if (prop.type === 'SpreadElement') unread(prop);
          else if (prop.computed) read(prop.key);
          else if (prop.key.type === 'Identifier') whole(prop.key.name, prop.key.range[0], prop.key);
          else if (prop.key.type === 'Literal' && typeof prop.key.value === 'string') whole(prop.key.value, prop.key.range[0] + 1, prop.key);
        }
      }
      return;
    }
    if (node.callee.type === 'Identifier') {
      const values = valuesOf(node.callee);
      if (values === null || values.length === 0) return unread(node);
      values.forEach(read);
      return;
    }
    if (node.callee.type === 'MemberExpression' && last === 'join') return read(node.callee.object);
    return unread(node);
  };

  /** A props object: its class keys are read, a style is reported, a spread or computed key it cannot read is unread. */
  const readProps = (/** @type {any} */ obj) => {
    for (const prop of obj.properties) {
      if (prop.type === 'SpreadElement') {
        if (prop.argument.type === 'ObjectExpression') readProps(prop.argument);
        else unread(prop);
      } else if (prop.computed && prop.key.type !== 'Literal') {
        unread(prop);
      } else if (prop.computed && !CLASS_PROP.test(keyName(prop.key) ?? '') && keyName(prop.key) !== 'style') {
        continue;
      } else if (prop.computed) {
        unread(prop);
      } else if (CLASS_PROP.test(keyName(prop.key) ?? '')) {
        read(prop.value);
      } else if (keyName(prop.key) === 'style') {
        style(prop);
      }
    }
  };

  for (const node of /** @type {any[]} */ (all)) {
    if (node.type === 'JSXAttribute' && CLASS_PROP.test(keyName(node.name) ?? '') && node.value) read(node.value);
    else if (node.type === 'JSXSpreadAttribute') {
      if (node.argument.type === 'ObjectExpression') readProps(node.argument);
      else unread(node);
    } else if (node.type === 'CallExpression' && PROPS_CALLS.has(calleeName(node.callee).last)) {
      const props = node.arguments[/** @type {number} */ (PROPS_CALLS.get(calleeName(node.callee).last))];
      if (!props || (props.type === 'Literal' && props.value === null) || (props.type === 'Identifier' && props.name === 'undefined')) continue;
      if (props.type === 'ObjectExpression') readProps(props);
      else unread(props);
    } else if (node.type === 'ObjectExpression') {
      // A class-carrying key in any other object literal (a props table handed on later).
      for (const prop of node.properties) {
        if (prop.type === 'Property' && !prop.computed && CLASS_PROP.test(keyName(prop.key) ?? '')) read(prop.value);
      }
    }
  }
  return found;
}
