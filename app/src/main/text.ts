// plain(): every string the main part sends to the window passes here first. React renders it as
// text, so markup is harmless; what is left is text that misleads the owner: control characters,
// bidi controls that reorder a line, separators that split one, lone surrogates, and no end.

/** The longest string, in code points, the main part sends to the window. */
export const TEXT_CAP = 2000;

// C0, DEL and C1; the bidi marks, embeddings, overrides and isolates; the line and paragraph separators.
const BLANKED = /[\u0000-\u001f\u007f-\u009f\u200e\u200f\u202a-\u202e\u2066-\u2069\u2028\u2029]/gu;

/** `s` with each control character blanked to a space, lone surrogates replaced, cut to `cap` code points. */
export function plain(s: string, cap: number = TEXT_CAP): string {
  const blanked = String(s).toWellFormed().replace(BLANKED, ' ');
  const points = Array.from(blanked);
  return points.length <= cap ? blanked : points.slice(0, Math.max(0, cap)).join('');
}

/** `s`'s length in code points (an astral character is one), as jq's length and Python's len count it. */
export function codePoints(s: string): number {
  let n = 0;
  for (const _ of s) n++;
  return n;
}
