// The lint rule's helpers, ported from Trawler's ui/eslint-rules/classes.js (logic only) with Keel's
// additions: the dark: variant and any variant the closed config lacks are refused, a '+' that
// builds a class is reported, and a cx()/clsx() call's arguments are read as classes.
import { expect, test } from 'vitest';
import { parseSource } from '../support/parse.ts';
import { arbitraryValue, classify, closedContext, collectClasses, emittableClasses, stripVariants, variantsOf } from '../../eslint-rules/classes.js';

/** The classes found in `code`, sorted (a partial one marked ~). */
const classes = (code: string) => collectClasses(parseSource(code)).map((f: { cls: string; partial: boolean }) => (f.partial ? `~${f.cls}` : f.cls)).sort();

test('stripVariants keeps the utility after the last top-level colon and drops the important mark', () => {
  expect(stripVariants('p-2')).toBe('p-2');
  expect(stripVariants('hover:disabled:bg-accent')).toBe('bg-accent');
  expect(stripVariants('!p-2')).toBe('p-2');
  expect(stripVariants('[&:hover]:p-2')).toBe('p-2');
  expect(stripVariants('p-[a:b]')).toBe('p-[a:b]');
});

test('variantsOf lists the variants before the utility, an arbitrary one whole', () => {
  expect(variantsOf('p-2')).toEqual([]);
  expect(variantsOf('hover:disabled:bg-accent')).toEqual(['hover', 'disabled']);
  expect(variantsOf('[&:hover]:p-2')).toEqual(['[&:hover]']);
});

test('arbitraryValue finds the bracketed value in the utility, not in a variant', () => {
  expect(arbitraryValue('p-[13px]')).toBe('13px');
  expect(arbitraryValue('hover:bg-[#fff]')).toBe('#fff');
  expect(arbitraryValue('p-2')).toBeNull();
});

test('classify: Keel\'s tokens and structural utilities are legal', () => {
  for (const cls of ['p-2', 'px-4', 'gap-3', 'bg-canvas', 'text-ink', 'text-ink-muted', 'text-body', 'font-semibold', 'rounded-control', 'border', 'border-line', 'h-control', 'max-w-column', 'font-mono', 'flex', 'items-center', 'break-all', 'hover:bg-accent', 'disabled:opacity-disabled', 'bg-accent/disabled', 'focus-visible:outline']) {
    expect(classify(cls), cls).toEqual({ legal: true });
  }
});

test('classify: an off-list class from a closed scale or from Tailwind\'s default is refused as off-list', () => {
  for (const cls of ['text-sm', 'bg-red-500', 'p-5', 'rounded-lg', 'font-bold', 'leading-7', 'border-2', 'opacity-50', 'z-10', 'duration-150', 'shadow-lg', 'text-blue']) {
    expect(classify(cls), cls).toMatchObject({ legal: false, kind: 'offlist' });
  }
});

test('classify: an arbitrary value is refused as arbitrary, with its value', () => {
  expect(classify('p-[13px]')).toEqual({ legal: false, kind: 'arbitrary', value: '13px' });
  expect(classify('bg-[#ff0000]')).toMatchObject({ legal: false, kind: 'arbitrary', value: '#ff0000' });
});

test('classify: a modifier step not on its owner\'s scale is refused as modifier, naming the steps', () => {
  expect(classify('bg-ink/50')).toMatchObject({ legal: false, kind: 'modifier', value: '50', owner: 'bg-ink', steps: ['disabled'] });
});

test('classify: the dark: variant is refused anywhere in the variant chain, even on a legal utility', () => {
  expect(classify('dark:bg-canvas')).toMatchObject({ legal: false, kind: 'dark' });
  expect(classify('hover:dark:text-ink')).toMatchObject({ legal: false, kind: 'dark' });
});

test('classify: a variant the closed config does not have (a breakpoint, an arbitrary one) is refused as variant', () => {
  expect(classify('md:p-2')).toMatchObject({ legal: false, kind: 'variant', value: 'md' });
  expect(classify('[&>*]:p-2')).toMatchObject({ legal: false, kind: 'variant', value: '[&>*]' });
});

test('the closed context empties every scale Keel does not declare: no z-, duration- or shadow- class is emittable', () => {
  const list: string[] = emittableClasses;
  expect(list).toContain('p-2');
  expect(list.some((c) => /^(z|duration|delay|blur|scale|rotate)-/.test(c))).toBe(false);
  for (const cls of ['shadow', 'shadow-lg', 'shadow-md']) expect(list).not.toContain(cls);
  expect(closedContext({ spacing: {} }).getClassList()).not.toContain('p-2');
});

test('collectClasses reads className strings, ternaries and logical branches, never the test', () => {
  expect(classes(`const A = ({ on }) => <i className={on === 'yes' ? 'p-2 flex' : 'p-4'} />;`)).toEqual(['flex', 'p-2', 'p-4']);
  expect(classes(`const A = ({ on }) => <i className={on && 'text-ok'} aria-label="p-9" />;`)).toEqual(['text-ok']);
});

test('collectClasses follows a name to its declaration, a lookup table\'s values and never its keys', () => {
  expect(classes(`const TONE = { 'p-9': 'text-ok', fail: 'text-fail' };\nconst A = ({ t }) => <i className={TONE[t]} />;`)).toEqual(['text-fail', 'text-ok']);
  expect(classes(`const base = 'flex gap-2';\nconst A = () => <i className={base} />;`)).toEqual(['flex', 'gap-2']);
});

test('collectClasses reads a cx() or clsx() call\'s strings and object keys as classes', () => {
  expect(classes(`const A = ({ on }) => <i className={cx('flex', on && 'p-2', { 'text-ok': on })} />;`)).toEqual(['flex', 'p-2', 'text-ok']);
  expect(classes(`const A = ({ on }) => <i className={clsx('gap-2')} />;`)).toEqual(['gap-2']);
});

test('collectClasses reads a template literal\'s whole classes and marks a class cut by an interpolation as partial', () => {
  expect(classes('const A = ({ on }) => <i className={`flex ${on ? "p-2" : ""} gap-2`} />;')).toEqual(['flex', 'gap-2', 'p-2']);
  expect(classes('const A = ({ n }) => <i className={`p-${n}`} />;')).toEqual(['~p-']);
});

test('collectClasses reports a class built with + as partial, whole, once', () => {
  expect(classes(`const A = ({ n }) => <i className={'p-' + n} />;`)).toEqual([`~'p-' + n`]);
  expect(classes(`const cls = 'bg-' + 'accent';\nconst A = () => <i className={cls} />;`)).toEqual([`~'bg-' + 'accent'`]);
});
