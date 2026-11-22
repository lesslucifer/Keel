// The view waiting.get answers: the one listed project's git facts, the router's values, its alerts
// and where a click points the window. The reader builds it; until it has read anything, this.

import type { View } from '../../shared/contract.ts';

/** What waiting.get answers before anything is read: no project, no git, no router run, not heard. */
export function emptyView(): View {
  return {
    project: null,
    git: null,
    router: { run: null, n: 0, values: {}, heard: false, lastAt: null, problem: null },
    alerts: [],
    focus: null,
    scheme: null,
    readAt: null,
  };
}
