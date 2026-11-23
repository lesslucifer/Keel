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

test('every module a node test reaches imports only node: built-ins and relative .ts files inside app/, so keel check needs no package install', () => {
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

test('every package is pinned to one exact version, and pnpm\'s lockfile is committed beside package.json, npm\'s is not', () => {
  const pkg = JSON.parse(readFileSync(path.join(APP, 'package.json'), 'utf8')) as Record<string, Record<string, string> | undefined>;
  const pins = { ...pkg.dependencies, ...pkg.devDependencies };
  assert.ok(Object.keys(pins).length > 0, 'no package listed');
  for (const [name, v] of Object.entries(pins)) assert.match(v, /^\d+\.\d+\.\d+$/, name);
  assert.ok(existsSync(path.join(APP, 'pnpm-lock.yaml')), 'no pnpm-lock.yaml');
  assert.ok(!existsSync(path.join(APP, 'package-lock.json')), 'package-lock.json is still there: the app installs with pnpm');
});

test('package.json pins pnpm to one exact version in packageManager', () => {
  const pkg = JSON.parse(readFileSync(path.join(APP, 'package.json'), 'utf8')) as { packageManager?: unknown };
  assert.match(String(pkg.packageManager), /^pnpm@\d+\.\d+\.\d+$/);
});

test('pnpm-workspace.yaml lays node_modules out flat (Forge packages it) and names each build it allows, each a locked package', () => {
  const file = path.join(APP, 'pnpm-workspace.yaml');
  assert.ok(existsSync(file), 'no pnpm-workspace.yaml');
  const text = readFileSync(file, 'utf8');
  assert.match(text, /^nodeLinker: hoisted$/m);
  const block = /^allowBuilds:\n((?: {2}.+\n?)+)/m.exec(text);
  assert.ok(block, 'no allowBuilds block');
  const entries = [...(block[1] as string).matchAll(/^ {2}'?([^':\s]+)'?: (true|false)$/gm)].map((m) => m[1] as string);
  assert.ok(entries.length > 0, 'allowBuilds names no package');
  const lock = readFileSync(path.join(APP, 'pnpm-lock.yaml'), 'utf8');
  for (const name of entries) assert.ok(lock.includes(`\n  ${name}@`) || lock.includes(`\n  '${name}@`), `${name} is not in pnpm-lock.yaml`);
});

// Each form is spelled in pieces in the self-test below, so this file's own text reads none of them.
const KEEL_READS: readonly RegExp[] = [
  /\b(?:window|globalThis|self)\s*(?:\?\.|\.)\s*keel\b/,
  /\b(?:window|globalThis|self)\s*(?:\?\.)?\s*\[\s*['"`]keel['"`]\s*\]/,
  /\(\s*(?:window|globalThis|self)\b[^()]*\)\s*(?:\?\.|\.)\s*keel\b/,
  /\(\s*(?:window|globalThis|self)\b[^()]*\)\s*(?:\?\.)?\s*\[\s*['"`]keel['"`]\s*\]/,
  /\{[^{}]*\bkeel\b[^{}]*\}\s*=\s*(?:window|globalThis|self)\b/,
  /\bReflect\s*\.\s*get\s*\(\s*(?:window|globalThis|self)\b/,
];

/** Whether `text` reads the preload's bridge off the page's global object, in any of the forms above. */
function readsKeel(text: string): boolean {
  return KEEL_READS.some((re) => re.test(text));
}

test('only src/window/bridge.ts reads window.keel or globalThis.keel; it does, so the guard is not vacuous', () => {
  const readers = walk(SRC)
    .filter((f) => /\.(ts|tsx)$/.test(f) && f.startsWith(path.join(SRC, 'window') + path.sep))
    .filter((f) => readsKeel(readFileSync(f, 'utf8')))
    .map((f) => path.relative(SRC, f));
  assert.deepEqual(readers, [path.join('window', 'bridge.ts')]);
});

test('the window.keel reader sees each form of reading it, and not a tool named keel', () => {
  const [w, g, s, k] = ['win' + 'dow', 'global' + 'This', 'se' + 'lf', 'ke' + 'el'];
  for (const text of [
    `${w}.${k}.ask()`,
    `${g}?.${k}`,
    `${s}.${k}`,
    `${w}['${k}']`,
    `${g}["${k}"]`,
    `(${g} as { ${k}?: KeelBridge }).${k}`,
    `(${w} as any)['${k}']`,
    `const { ${k} } = ${w};`,
    `const { ${k}: b } = ${g};`,
    `Reflect.get(${g}, '${k}')`,
  ]) {
    assert.equal(readsKeel(text), true, text);
  }
  for (const text of [`state.tools.${k}`, `TOOL_NAMES.includes('${k}')`, `${w}.addEventListener('x', f)`, `const ${k} = 1;`]) {
    assert.equal(readsKeel(text), false, text);
  }
});

const FAKE = path.join(SRC, 'window', 'fake');
const inside = (f: string, dir: string) => f.startsWith(dir + path.sep);

/** The files under app/src and app/test whose imports resolve into src/window/fake/, fake/ itself left out. */
function fakeImporters(): string[] {
  const sources = [...walk(SRC), ...walk(path.join(APP, 'test'))].filter((f) => /\.(ts|tsx)$/.test(f) && !inside(f, FAKE));
  return sources
    .filter((f) => imports(readFileSync(f, 'utf8')).some((s) => s.startsWith('.') && (path.resolve(path.dirname(f), s) === FAKE || inside(path.resolve(path.dirname(f), s), FAKE))))
    .map((f) => path.relative(APP, f));
}

test('only src/window/browser.tsx (and app/test-window/, outside this walk) imports src/window/fake/; the window\'s tests do, so the fake exists', () => {
  assert.deepEqual(fakeImporters().filter((f) => f !== path.join('src', 'window', 'browser.tsx')), []);
  assert.ok(existsSync(path.join(FAKE, 'fake.ts')), 'src/window/fake/fake.ts is missing');
  const windowTests = walk(path.join(APP, 'test-window')).filter((f) => /\.(ts|tsx)$/.test(f));
  assert.ok(windowTests.some((f) => imports(readFileSync(f, 'utf8')).some((s) => s.includes('/window/fake/'))), 'no window test imports the fake');
});

const WINDOW = path.join(SRC, 'window');

/** Every file reachable from `entry` by relative static imports (and side-effect imports such as a stylesheet). */
function staticGraph(entry: string): string[] {
  const seen = new Set<string>();
  const queue = [entry];
  while (queue.length > 0) {
    const f = queue.pop() as string;
    if (seen.has(f)) continue;
    seen.add(f);
    if (!/\.(ts|tsx)$/.test(f)) continue;
    for (const spec of imports(readFileSync(f, 'utf8'))) {
      if (!spec.startsWith('.')) continue;
      const target = path.resolve(path.dirname(f), spec);
      assert.ok(existsSync(target), `${path.relative(APP, f)} imports ${spec}, which does not exist`);
      queue.push(target);
    }
  }
  return [...seen].map((f) => path.relative(APP, f)).sort();
}

test('the shipped page never reaches the fake or the browser entry: index.html loads main.tsx, whose static imports reach neither, and the renderer builds index.html alone', () => {
  const html = readFileSync(path.join(WINDOW, 'index.html'), 'utf8');
  assert.match(html, /<script type="module" src="\.\/main\.tsx"><\/script>/);
  const graph = staticGraph(path.join(WINDOW, 'main.tsx'));
  for (const part of ['main.tsx', 'App.tsx', 'bridge.ts', 'hooks.ts', 'styles.css']) assert.ok(graph.includes(path.join('src', 'window', part)), `the walk reached src/window/${part}`);
  const leaks = graph.filter((f) => f.startsWith(path.join('src', 'window', 'fake') + path.sep) || f === path.join('src', 'window', 'browser.tsx'));
  assert.deepEqual(leaks, []);
  assert.doesNotMatch(readFileSync(path.join(APP, 'vite.renderer.config.ts'), 'utf8'), /rollupOptions|browser\.(html|tsx)/);
});

test('the graph walk follows each relative import, a stylesheet included, so the guard above sees a leak', () => {
  const graph = staticGraph(path.join(WINDOW, 'browser.tsx'));
  for (const part of ['browser.tsx', 'styles.css', path.join('fake', 'fake.ts'), path.join('fake', 'fixtures.ts'), 'App.tsx']) assert.ok(graph.includes(path.join('src', 'window', part)), part);
});

test('the browser mode\'s page carries the same CSP meta as the shipped page and loads only browser.tsx', () => {
  const page = readFileSync(path.join(WINDOW, 'browser.html'), 'utf8');
  const shipped = readFileSync(path.join(WINDOW, 'index.html'), 'utf8');
  const meta = (html: string) => [...html.matchAll(/<meta\s+http-equiv="Content-Security-Policy"[^>]*>/gi)].map((m) => m[0]);
  assert.deepEqual(meta(page), meta(shipped));
  assert.equal(meta(page).length, 1);
  const scripts = [...page.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)];
  assert.equal(scripts.length, 1);
  assert.match(scripts[0]?.[1] ?? '', /^\s*type="module"\s+src="\.\/browser\.tsx"\s*$/);
});

/** Whether `text` asks Vite to glob modules in (import.meta.glob, in any spacing), which a static walk cannot follow. */
function globsModules(text: string): boolean {
  return /\bimport\s*\.\s*meta\s*\.\s*glob/.test(text);
}

test('no file in the window globs modules in, so the static walk above sees every import the shipped page makes', () => {
  const globbers = walk(WINDOW)
    .filter((f) => /\.(ts|tsx)$/.test(f) && globsModules(readFileSync(f, 'utf8')))
    .map((f) => path.relative(APP, f));
  assert.deepEqual(globbers, []);
});

test('the glob reader sees import.meta.glob in each spacing, and not a word that merely contains it', () => {
  const [I, M, G] = ['imp' + 'ort', 'me' + 'ta', 'gl' + 'ob'];
  for (const text of [`${I}.${M}.${G}('./fake/*.ts', { eager: true })`, `${I} . ${M} . ${G}Eager('./x')`, `${I}.${M}.${G}<Mod>('./y')`]) assert.equal(globsModules(text), true, text);
  for (const text of [`${I}.${M}.url`, `const ${G}al = 1`, `${M}.${G}`]) assert.equal(globsModules(text), false, text);
});

test('one source per grammar: commands.ts takes the note kinds from the contract, link.ts the work-unit pattern from shared/names.ts; neither writes its own copy', () => {
  const commands = readFileSync(path.join(SRC, 'main', 'commands.ts'), 'utf8');
  // The import forms are spelled in pieces, so the import reader above does not take them for this file's own imports.
  const importOf = (name: string, file: string) => new RegExp(String.raw`import \{[^}]*\b${name}\b[^}]*\} ` + 'fr' + `om '${file.replace(/[./]/g, (c) => `\\${c}`)}'`);
  assert.match(commands, importOf('NOTE_KINDS', '../shared/contract.ts'));
  assert.doesNotMatch(commands, /\[\s*'mr'\s*,\s*'cr'\s*,\s*'dod'\s*\]/);
  const link = readFileSync(path.join(SRC, 'main', 'alerter', 'link.ts'), 'utf8');
  assert.match(link, importOf('WU_SOURCE', '../../shared/names.ts'));
  assert.doesNotMatch(link, /E-\[A-Z0-9\]\{1,16\}/);
});

test('epic review round 1 (git.ts:106, items.ts:30, items.ts:172): the id patterns, the router keys and the epic of a work unit are written in shared/names.ts only; the log reader, the git reader and the item rules import them', () => {
  // Each copy as it was written before, spelled in pieces so this file holds none of them.
  const copies = ['E-[A-Z0-9]' + '{1,16}', '[a-z0-9][a-z0-9-]' + '{0,40}', "split('/')" + '[1]', "split('.')" + '[0]', "slice('epic/'" + '.length)', "startsWith('wor" + "ker/')"];
  const holders = walk(SRC)
    .filter((f) => /\.(ts|tsx)$/.test(f) && f !== path.join(SRC, 'shared', 'names.ts'))
    .flatMap((f) => copies.filter((c) => readFileSync(f, 'utf8').includes(c)).map((c) => `${path.relative(APP, f)}: ${c}`));
  assert.deepEqual(holders, []);
  const names = readFileSync(path.join(SRC, 'shared', 'names.ts'), 'utf8');
  for (const c of copies.slice(0, 2)) assert.ok(names.includes(c), c);
  const source = (file: string) => readFileSync(path.join(SRC, file), 'utf8');
  // The import form spelled in pieces, as above, so the import reader does not take it for this file's own.
  const imports = (names: string[], file: string) => new RegExp(String.raw`import \{[^}]*` + names.map((n) => String.raw`\b${n}\b`).join('[^}]*') + String.raw`[^}]*\} ` + 'fr' + `om '${file.replace(/[./]/g, (c) => `\\${c}`)}'`);
  assert.match(source('main/reader/log.ts'), imports(['KEY', 'WU_ID', 'WU_SOURCE'], '../../shared/names.ts'));
  assert.match(source('shared/items.ts'), imports(['epicOfUnit', 'epicOfWu', 'routerKey', 'WU_ID'], './names.ts'));
  assert.match(source('main/reader/git.ts'), imports(['epicOfUnit', 'unitPrefix'], '../../shared/names.ts'));
});
