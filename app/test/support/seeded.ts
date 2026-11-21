// A seeded generator: every generated-input test draws the same inputs on every run and machine.

export type Rng = () => number;

/** mulberry32, a small fixed PRNG: one seed, one sequence. */
export function seeded(seed: number): Rng {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function pick<T>(rng: Rng, items: readonly T[]): T {
  const item = items[Math.floor(rng() * items.length)];
  if (item === undefined) throw new Error('pick from an empty list');
  return item;
}

/** What a hostile window might send: printable ASCII, every C0 and C1 control, DEL, bidi controls, separators, astral and lone surrogates. */
export const HOSTILE: readonly string[] = [
  ...Array.from({ length: 95 }, (_, i) => String.fromCharCode(32 + i)),
  ...Array.from({ length: 32 }, (_, i) => String.fromCharCode(i)),
  '\u007f',
  ...Array.from({ length: 32 }, (_, i) => String.fromCharCode(0x80 + i)),
  '\u200e', '\u200f', '\u202a', '\u202e', '\u2066', '\u2069', '\u2028', '\u2029',
  'é', '漢', '😀', '\ud800', '\udfff',
];

/** A string of 0 to `maxLen` characters drawn from `alphabet`. */
export function hostileString(rng: Rng, maxLen: number, alphabet: readonly string[] = HOSTILE): string {
  const n = Math.floor(rng() * (maxLen + 1));
  let s = '';
  for (let i = 0; i < n; i++) s += pick(rng, alphabet);
  return s;
}
