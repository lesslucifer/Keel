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
  for (const cls of ['p-xs', 'px-md', 'gap-sm', 'bg-canvas', 'bg-card', 'text-ink', 'text-ink-secondary', 'text-body', 'font-ui', 'font-display', 'rounded-pill', 'border-t', 'border-line', 'h-control', 'max-w-column', 'font-mono', 'flex', 'items-center', 'break-all', 'uppercase', 'bg-tint/tint', 'hover:bg-tint/tint-hover', 'disabled:opacity-disabled', 'focus-visible:outline']) {
    expect(classify(cls), cls).toEqual({ legal: true });
  }
});

test('classify: an off-list class from a closed scale or from Tailwind\'s default is refused as off-list', () => {
  for (const cls of ['text-sm', 'bg-red-500', 'p-5', 'p-2', 'rounded-2xl', 'font-bold', 'font-semibold', 'leading-7', 'border-2', 'opacity-50', 'z-10', 'duration-150', 'shadow-lg', 'text-blue', 'text-ok', 'bg-surface']) {
    expect(classify(cls), cls).toMatchObject({ legal: false, kind: 'offlist' });
  }
});

test('classify: an arbitrary value is refused as arbitrary, with its value', () => {
  expect(classify('p-[13px]')).toEqual({ legal: false, kind: 'arbitrary', value: '13px' });
  expect(classify('bg-[#ff0000]')).toMatchObject({ legal: false, kind: 'arbitrary', value: '#ff0000' });
});

test('classify: a modifier step not on its owner\'s scale is refused as modifier, naming the steps', () => {
  expect(classify('bg-ink/50')).toMatchObject({ legal: false, kind: 'modifier', value: '50', owner: 'bg-ink', steps: ['disabled', 'tint', 'tint-hover'] });
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
  expect(list).toContain('p-xs');
  expect(list.some((c) => /^(z|duration|delay|blur|scale|rotate)-/.test(c))).toBe(false);
  for (const cls of ['shadow', 'shadow-lg', 'shadow-md']) expect(list).not.toContain(cls);
  expect(closedContext({ spacing: {} }).getClassList()).not.toContain('p-xs');
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

test('collectClasses reads a table through as const, satisfies, as T and the non-null !, never the type itself', () => {
  expect(classes(`const T = { a: 'p-xs' } as const;\nconst A = ({ k }) => <i className={T[k]} />;`)).toEqual(['p-xs']);
  expect(classes(`const T = { a: 'p-sm' } satisfies Record<string, 'x-type'>;\nconst A = ({ k }) => <i className={T[k]} />;`)).toEqual(['p-sm']);
  expect(classes(`const T = { a: 'p-md' } as Record<string, 'x-type'>;\nconst A = ({ k }) => <i className={T[k]} />;`)).toEqual(['p-md']);
  expect(classes(`const T: Record<string, string | undefined> = { a: 'p-lg' };\nconst A = ({ k }) => <i className={T[k]!} />;`)).toEqual(['p-lg']);
  expect(classes(`const A = ({ on }) => <i className={(on ? 'p-xl' : 'p-xxl') as string} />;`)).toEqual(['p-xl', 'p-xxl']);
});

test('collectClasses reads through an old-style <T> type assertion in a .ts file', () => {
  const found = collectClasses(parseSource(`const T = <Record<string, string>>{ a: 'p-huge' };\nexport const el = { className: T.a };`, false)).map((f: { cls: string }) => f.cls);
  expect(found).toContain('p-huge');
});

/** Every finding in `code` as kind:cls (whole, partial or unread), sorted. */
const findings = (code: string) =>
  collectClasses(parseSource(code))
    .map((f: { cls: string; partial: boolean; unread?: boolean }) => `${f.unread ? 'unread' : f.partial ? 'partial' : 'whole'}:${f.cls}`)
    .sort();

test('collectClasses seeds from class=, className= and any prop ending in ClassName', () => {
  expect(classes(`const A = () => <B class="p-xs" innerClassName="p-sm" labelClassName={'p-md'} title="p-9" />;`)).toEqual(['p-md', 'p-sm', 'p-xs']);
});

test('collectClasses seeds from a className key in any object literal: a spread object, createElement\'s props', () => {
  expect(classes(`const A = () => <i {...{ className: 'p-xs', title: 'p-9' }} />;`)).toEqual(['p-xs']);
  expect(classes(`const A = () => createElement('i', { className: 'p-sm', 'innerClassName': 'p-md' });`)).toEqual(['p-md', 'p-sm']);
});

test('collectClasses reports a spread it cannot read as unread, whole', () => {
  expect(findings(`const A = (props) => <i {...props} />;`)).toEqual(['unread:{...props}']);
});

test('collectClasses reports an unknown call reaching a class as unread: tw(), a method on a value', () => {
  expect(findings(`const A = () => <i className={tw('p-[3px] text-sm')} />;`)).toEqual(['unread:tw(…)']);
  expect(findings(`const x = 'p-xs';\nconst A = () => <i className={x.trim()} />;`)).toEqual(['unread:x.trim(…)', 'whole:p-xs']);
});

test('collectClasses reads a local function\'s strings and an array literal\'s join, and reports neither as unread', () => {
  expect(findings(`function pick(on) { return on ? 'p-xs' : 'p-sm'; }\nconst A = () => <i className={pick(true)} />;`)).toEqual(['whole:p-sm', 'whole:p-xs']);
  expect(findings(`const A = () => <i className={['p-xs', 'p-sm'].join(' ')} />;`)).toEqual(['whole:p-sm', 'whole:p-xs']);
});

test('collectClasses reports an imported name or a parameter reaching a class as unread, whole', () => {
  expect(findings(`import { LOOK } from './look.ts';\nconst A = () => <i className={LOOK.bad} />;`)).toEqual(['unread:LOOK.bad']);
  expect(findings(`const A = ({ cls }) => <i className={cls} />;`)).toEqual(['unread:cls']);
  expect(findings(`const A = (props) => <i className={props.className} />;`)).toEqual(['unread:props.className']);
  expect(findings(`const A = ({ on }) => <i className={on ? 'p-xs' : undefined} />;`)).toEqual(['whole:p-xs']);
});

test('collectClasses reads every value a reassigned binding takes', () => {
  expect(classes(`let m = 'flex';\nm = 'p-xs';\nconst A = () => <i className={m} />;`)).toEqual(['flex', 'p-xs']);
});
