// The app's link scheme, keel:// (K12, the app's side). The router's sender writes only
// keel://wu/<work unit> (scripts/notify-owner.py); the log's own grammar admits more (W1's N5:
// keel://../..), so the app opens exactly that one form and refuses every other, opening nothing.
// Any program may open a keel:// link, so a link only points the window at a work unit: it runs nothing.

import { plain } from '../text.ts';

export const LINK_CAP = 64;
const LINK = /^keel:\/\/wu\/(E-[A-Z0-9]{1,16}\.W[0-9]{1,3})$/;

/** The work unit a link names, or null for anything but keel://wu/E-<CODE>.W<m>. */
export function parseLink(url: unknown): { wu: string } | null {
  if (typeof url !== 'string' || url.length > LINK_CAP) return null;
  const m = LINK.exec(url);
  return m === null ? null : { wu: m[1] as string };
}

/** Point `focus` at the link's work unit; whether the link was taken. */
export function openLink(url: unknown, focus: { point(wu: string | null): void }): boolean {
  const r = parseLink(url);
  if (r === null) return false;
  focus.point(r.wu);
  return true;
}

/** The app macOS opens keel:// links with (app.getApplicationNameForProtocol), in words; null when none. */
export function schemeText(name: unknown): string | null {
  return typeof name === 'string' && name !== '' ? plain(name, 200) : null;
}
