// Static guards over app/src, read as text: two rules no type checker holds.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const APP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const SRC = path.join(APP, 'src');

function walk(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? walk(p) : [p];
  });
}

// Each sink is spelled in pieces, so this file's own text names none of them whole.
const SINKS: ReadonlyArray<[string, RegExp]> = [
  ['inner' + 'HTML', /\binnerHTML\b/],
  ['outer' + 'HTML', /\bouterHTML\b/],
  ['insertAdjacent' + 'HTML', /\binsertAdjacentHTML\b/],
  ['document' + '.write', /\bdocument\s*\.\s*write(ln)?\b/],
  ['ev' + 'al', /(^|[^.\w])eval\s*\(/],
  ['new ' + 'Function', /\bnew\s+Function\b/],
  ['dangerouslySet' + 'InnerHTML', /\bdangerouslySetInnerHTML\b/],
  ['a string timer', /\bset(Timeout|Interval)\s*\(\s*['"`]/],
];

/** The sinks `text` uses, by name. */
function sinksIn(text: string): string[] {
  return SINKS.filter(([, re]) => re.test(text)).map(([name]) => name);
}

test('no file under app/src (the window included) uses an HTML or code sink: innerHTML, outerHTML, insertAdjacentHTML, document.write, eval, new Function, dangerouslySetInnerHTML', () => {
  const files = walk(SRC).filter((f) => /\.(ts|tsx|html)$/.test(f));
  assert.ok(files.length > 0, 'app/src holds no source');
  for (const f of files) assert.deepEqual(sinksIn(readFileSync(f, 'utf8')), [], path.relative(APP, f));
});

test('the sink reader sees each sink, so the guard above cannot pass by missing one', () => {
  const [ih, oh, ia, dw, ev, nf, ds] = ['inner' + 'HTML', 'outer' + 'HTML', 'insertAdjacent' + 'HTML', 'document' + '.write', 'ev' + 'al', 'new ' + 'Function', 'dangerouslySet' + 'InnerHTML'];
  assert.deepEqual(sinksIn(`el.${ih} = x`), [ih]);
  assert.deepEqual(sinksIn(`el.${oh} = x`), [oh]);
  assert.deepEqual(sinksIn(`el.${ia}('beforeend', x)`), [ia]);
  assert.deepEqual(sinksIn(`${dw}ln(x)`), [dw]);
  assert.deepEqual(sinksIn(`const y = ${ev}(x)`), [ev]);
  assert.deepEqual(sinksIn(`${nf}('return 1')`), [nf]);
  assert.deepEqual(sinksIn(`<div ${ds}={{ __html: x }} />`), [ds]);
  assert.deepEqual(sinksIn("setTimeout('x()', 1)"), ['a string timer']);
  assert.deepEqual(sinksIn('el.textContent = x; retrieval(x); medieval(y)'), []);
});

/** Every import specifier in a source: static imports, re-exports, side-effect imports, import() and require(). */
function imports(text: string): string[] {
  const out: string[] = [];
  const forms = [
    /\b(?:import|export)\b[^'"`;]*?\bfrom\s*['"]([^'"]+)['"]/g,
    /\bimport\s*['"]([^'"]+)['"]/g,
    /\bimport\s*\(\s*['"`]([^'"`]+)['"`]\s*\)/g,
    /\brequire\s*\(\s*['"`]([^'"`]+)['"`]\s*\)/g,
  ];
  for (const re of forms) for (const m of text.matchAll(re)) out.push(m[1] as string);
  return out;
}

test('every module a node test reaches imports only node: built-ins and relative .ts files inside app/, so keel check needs no npm', () => {
  const queue = walk(path.join(APP, 'test')).filter((f) => f.endsWith('.ts'));
  const seen = new Set<string>();
  while (queue.length > 0) {
    const f = queue.pop() as string;
    if (seen.has(f)) continue;
    seen.add(f);
    for (const spec of imports(readFileSync(f, 'utf8'))) {
      if (spec.startsWith('node:')) continue;
      const where = `${path.relative(APP, f)} imports ${spec}`;
      assert.ok(spec.startsWith('./') || spec.startsWith('../'), where);
      assert.ok(spec.endsWith('.ts'), `${where} without its .ts`);
      const target = path.resolve(path.dirname(f), spec);
      assert.ok(target.startsWith(APP + path.sep), `${where}, which leaves app/`);
      assert.ok(existsSync(target), `${where}, which does not exist`);
      queue.push(target);
    }
  }
  for (const part of ['main', 'preload', 'shared']) {
    assert.ok([...seen].some((f) => f.startsWith(path.join(SRC, part) + path.sep)), `the walk reached app/src/${part}`);
  }
});

test('the import reader sees every import form, so the guard above cannot be dodged by a form it misses', () => {
  // Built from pieces, so this file's text holds no import the walk would follow.
  const [I, E, R] = ['imp' + 'ort', 'exp' + 'ort', 'req' + 'uire'];
  const text = [
    `${I} a from 'electron';`,
    `${I} { b } from "./b.ts";`,
    `${I} type { C } from '../c.ts';`,
    `${E} { d } from './d.ts';`,
    `${E} * from './star.ts';`,
    `${I} './side.ts';`,
    `const e = await ${I}('node:fs');`,
    `const f = ${R}('child_process');`,
    `${I} {\n  g,\n  h,\n} from './gh.ts';`,
  ].join('\n');
  assert.deepEqual(imports(text).sort(), ['../c.ts', './b.ts', './d.ts', './gh.ts', './side.ts', './star.ts', 'child_process', 'electron', 'node:fs'].sort());
});

test('only the shell (src/main/index.ts) and the preload entry import electron; the window imports no node: module, no electron and nothing from the main part', () => {
  const sources = walk(SRC).filter((f) => /\.(ts|tsx)$/.test(f));
  const electron = sources.filter((f) => imports(readFileSync(f, 'utf8')).some((s) => s === 'electron' || s.startsWith('electron/')));
  assert.deepEqual(electron.map((f) => path.relative(SRC, f)).sort(), ['main/index.ts', 'preload/index.ts']);
  const window = sources.filter((f) => f.startsWith(path.join(SRC, 'window') + path.sep));
  assert.ok(window.length > 0, 'app/src/window holds no source');
  for (const f of window) {
    for (const spec of imports(readFileSync(f, 'utf8'))) {
      const where = `${path.relative(APP, f)} imports ${spec}`;
      assert.ok(!spec.startsWith('node:') && spec !== 'electron' && !spec.startsWith('electron/'), where);
      if (spec.startsWith('.')) assert.ok(!path.resolve(path.dirname(f), spec).startsWith(path.join(SRC, 'main') + path.sep), where);
    }
  }
});

test('every package is pinned to one exact version, and the lockfile is committed beside package.json', () => {
  const pkg = JSON.parse(readFileSync(path.join(APP, 'package.json'), 'utf8')) as Record<string, Record<string, string> | undefined>;
  const pins = { ...pkg.dependencies, ...pkg.devDependencies };
  assert.ok(Object.keys(pins).length > 0, 'no package listed');
  for (const [name, v] of Object.entries(pins)) assert.match(v, /^\d+\.\d+\.\d+$/, name);
  assert.ok(existsSync(path.join(APP, 'package-lock.json')), 'no package-lock.json');
});
