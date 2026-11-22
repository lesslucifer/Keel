// The text-on-ground pairs the window draws, each measured for AA in both worlds by
// test-window/unit/contrast.test.ts; test-window/integration/grounds.test.tsx proves the rendered
// screens draw no other pair.
export const PAIRS: Array<[string, string]> = [
  ['ink', 'canvas'],
  ['ink', 'card'],
  ['ink-secondary', 'canvas'],
  ['ink-secondary', 'card'],
  ['on-primary', 'primary'],
  ['on-set', 'set'],
  ['on-found', 'found'],
  ['on-missing', 'missing'],
  ['problem', 'canvas'],
  ['problem', 'card'],
];
