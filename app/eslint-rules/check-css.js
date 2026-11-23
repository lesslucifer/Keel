// @ts-check
// pnpm lint's second step: exit 1, naming each problem, when the window's shipped CSS holds
// anything outside Keel's closed vocabulary - a second stylesheet, a raw declaration in styles.css, an
// emitted class off the list - or when nothing could be judged (eslint-rules/emitted.js). Silent and
// exit 0 otherwise.
import { shippedCssProblems } from './emitted.js';

const problems = await shippedCssProblems();
if (problems.length > 0) {
  for (const line of problems) process.stderr.write(`${line}\n`);
  process.exit(1);
}
