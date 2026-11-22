// @ts-check
// npm run lint's second step: exit 1, naming each class, when the CSS Tailwind emits for the window
// holds a class outside Keel's closed vocabulary (eslint-rules/emitted.js). Silent and exit 0 otherwise.
import { offVocabulary } from './emitted.js';

const off = await offVocabulary();
if (off.length > 0) {
  for (const cls of off) process.stderr.write(`design/css: \`${cls}\` reaches the shipped CSS but is not in the closed vocabulary (app/tailwind.config.js)\n`);
  process.exit(1);
}
