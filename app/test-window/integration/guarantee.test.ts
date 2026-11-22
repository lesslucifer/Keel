// The guarantee: only Keel's vocabulary reaches the shipped CSS. Tailwind writes CSS only for whole
// class names it finds written out in its content files, however a string is later built, so the
// check reads what Tailwind emits - styles.css through Tailwind's PostCSS plugin with the shipped
// config, over the one content list (window-sources.js) - and refuses every emitted class outside
// the closed vocabulary. pnpm lint runs it after ESLint (eslint-rules/check-css.js). Each probe
// below is a route review round 2 found past the AST reader: the classes it would ship are caught,
// or it ships no CSS at all (pieces Tailwind never sees whole).
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { expect, test } from 'vitest';
import { emittedClasses, offVocabulary } from '../../eslint-rules/emitted.js';
import { SOURCE_GLOBS, SCRIPT_GLOBS } from '../../window-sources.js';
import tailwindConfig from '../../tailwind.config.js';

const APP = path.resolve(import.meta.dirname, '..', '..');
const tsx = (raw: string) => [{ raw, extension: 'tsx' }];

test('the real content set ships only Keel\'s vocabulary', async () => {
  expect(await offVocabulary()).toEqual([]);
  const all = await emittedClasses();
  expect(all).toContain('bg-card');
  expect(all).toContain('rounded-pill');
});

test('pnpm lint\'s CSS step exits 0 on the real tree and prints nothing', () => {
  const run = spawnSync(process.execPath, [path.join(APP, 'eslint-rules', 'check-css.js')], { cwd: APP, encoding: 'utf8' });
  expect([run.status, run.stdout, run.stderr]).toEqual([0, '', '']);
});

test('the CSS step exits 1 naming each class when the content ships one outside the vocabulary', async () => {
  const out = await offVocabulary({ content: tsx('<p className="p-[13px] dark:bg-ink z-50 bg-card" />') });
  expect(out).toEqual(['dark:bg-ink', 'p-[13px]', 'z-50']);
});

test('one list: Tailwind\'s content is the source list, and ESLint reads its script files', async () => {
  expect(tailwindConfig.content).toEqual(SOURCE_GLOBS.map((g: string) => path.join(APP, g)));
  expect(SCRIPT_GLOBS).toEqual(SOURCE_GLOBS.map((g: string) => g.replace('html,', '')));
  expect(SOURCE_GLOBS).toEqual(['src/window/**/*.{html,js,jsx,mjs,cjs,ts,tsx,mts,cts}']);
});

// Round 2's probes, each as the content Tailwind would scan.
const PROBES: Array<[string, string, string[]]> = [
  ['classes.js:427 adjacent interpolations: a piece imported whole is caught, pieces alone ship nothing', "export const PAD = 'p-[13px]';\nconst A = ({ b }) => <i className={`${PAD}${b} flex ${'p-'}${'[14px]'}`} />;", ['p-[13px]']],
  ['classes.js:223 a + of two names', "const WHOLE = 'p-[13px] bg-red-500 dark:bg-ink';\nconst A = ({ MORE }) => <i className={WHOLE + MORE} />;", ['dark:bg-ink', 'p-[13px]']],
  ['eslint.config.js:10 a nested config turning the rule off: the CSS step reads no ESLint config', '<p className="text-sm p-[3px] dark:bg-ink" />', ['dark:bg-ink', 'p-[3px]']],
  ['classes.js:408 identifier, computed and spread keys in cx', "const EXTRA = { 'p-[15px]': true };\nconst A = ({ on, n }) => <i className={cx({ transition: on, blur: on, grow: on, [`p-${n}`]: on, ...EXTRA })} />;", ['blur', 'grow', 'p-[15px]', 'transition']],
  ['classes.js:309 a shadowing destructured local', "const cls = 'flex';\nconst useLook = () => ({ cls: 'text-sm p-[25px]' });\nconst A = () => { const { cls } = useLook(); return <i className={cls} />; };", ['p-[25px]']],
  ['classes.js:321 member writes, push, Object.assign', "const LOOK = {};\nLOOK.a = 'bg-red-500 p-[21px]';\nLOOK['b'] = 'dark:bg-ink';\nconst parts = ['flex'];\nparts.push('p-[22px]');\nconst o = {};\nObject.assign(o, { k: 'z-10' });", ['dark:bg-ink', 'p-[21px]', 'p-[22px]', 'z-10']],
  ['classes.js:195 this. chains', "class B extends Component { look = 'text-sm p-[23px]'; render() { return <i className={this.look} />; } }", ['p-[23px]']],
  ['classes.js:330 props by reference and style in props', "const p = { className: 'p-[24px]' };\nconst A = () => createElement('div', p);", ['p-[24px]']],
  ['eslint.config.js:14 classes in the shipped html', '<body class="z-50 p-[13px] dark:bg-ink grid-cols-3"></body>', ['dark:bg-ink', 'grid-cols-3', 'p-[13px]', 'z-50']],
];

for (const [route, raw, caught] of PROBES) {
  test(`round 2 probe caught by the CSS check: ${route}`, async () => {
    expect(await offVocabulary({ content: tsx(raw) })).toEqual(caught);
  });
}

test('pieces Tailwind never sees whole ship no CSS: the adjacent-literal and computed-key probes emit nothing for their pieces', async () => {
  const emitted = await emittedClasses({ content: tsx("const A = ({ n }) => <i className={`${'p-'}${'[14px]'} ${cx({ [`m-${n}`]: true })}`} />;") });
  expect(emitted.filter((c: string) => /\[/.test(c) || c.startsWith('m-'))).toEqual([]);
});

test('ESLint runs from the one pinned config: no other eslint config exists under app/', () => {
  const skip = new Set(['node_modules', '.vite', 'out']);
  const found: string[] = [];
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (skip.has(e.name)) continue;
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (/^(eslint\.config\.(js|mjs|cjs|ts|mts|cts)|\.eslintrc(\..*)?)$/.test(e.name)) found.push(path.relative(APP, p));
    }
  };
  walk(APP);
  expect(found).toEqual(['eslint.config.js']);
  expect(existsSync(path.join(APP, 'eslint.config.js'))).toBe(true);
});

test('the group and peer markers and their variants pass the CSS check', async () => {
  expect(await offVocabulary({ content: tsx('<i className="group group-hover:text-ink peer peer-checked:bg-primary" />') })).toEqual([]);
});
