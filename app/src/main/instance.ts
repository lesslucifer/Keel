// One Keel at a time (Electron's single-instance lock, in the app folder). A launch that loses the lock
// quits; before it does, it says so in one line (E-APPLIST.W9, backlog A3's follow-up from the W5 live
// check: a second `pnpm start` used to quit with no word), and the running Keel's window comes forward.

/** The line a launch that loses the lock writes to stderr. */
export const SECOND_LAUNCH = 'Keel is already running: this launch quits; quit the other first';

/** Write the second launch's line through `write` (the shell hands stderr). */
export function secondLaunch(write: (text: string) => void): void {
  write(`${SECOND_LAUNCH}\n`);
}
