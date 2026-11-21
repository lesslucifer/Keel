// The tests' own promise: a fake that hangs never outlives the process that spawned it, however that
// process ends. A child node stands in for a test process: it spawns a hung fake through capture()
// (its own process group, a 60 s timeout, so the main part never kills it here), then dies by ^C, by
// an uncaught throw, by a failed assertion, or by SIGKILL. The fake's whole group must be gone soon after.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { readFileSync } from 'node:fs';
import { tempDir, program, HANG, groupGone } from '../support/fs.ts';

const COMMANDS = pathToFileURL(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'src', 'main', 'commands.ts')).href;

/** Wait until `done()` holds, checking every 20 ms, for at most `ms`; whether it held. */
async function within(ms: number, done: () => boolean): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (done()) return true;
    await new Promise((r) => setTimeout(r, 20));
  }
  return done();
}

for (const how of ['SIGINT', 'throw', 'assert', 'SIGKILL'] as const) {
  test(`a hung fake's process group is gone soon after its test process dies by ${how}`, async (t) => {
    const dir = tempDir(t, 'leak');
    const fake = program(path.join(dir, 'fake'), `#!/bin/sh\necho $$ > ${dir}/pid\n${HANG}\n`);
    const script = [
      `import { capture } from ${JSON.stringify(COMMANDS)};`,
      `import assert from 'node:assert/strict';`,
      `import { existsSync } from 'node:fs';`,
      // Should the outer test die first, this child follows it within 100 ms.
      `setInterval(() => { try { process.kill(${process.pid}, 0); } catch { process.exit(1); } }, 100);`,
      `capture(${JSON.stringify(fake)}, [], { PATH: '/usr/bin:/bin' }, 60_000);`,
      `const wait = setInterval(() => {`,
      `  if (!existsSync(${JSON.stringify(path.join(dir, 'pid'))})) return;`,
      `  clearInterval(wait);`,
      `  process.stdout.write('ready\\n');`,
      `  if (${JSON.stringify(how)} === 'throw') setTimeout(() => { throw new Error('panic'); }, 10);`,
      `  if (${JSON.stringify(how)} === 'assert') setTimeout(() => assert.equal(1, 2), 10);`,
      `}, 10);`,
    ].join('\n');
    const child = spawn(process.execPath, ['--input-type=module', '-e', script], { stdio: ['ignore', 'pipe', 'pipe'] });
    t.after(() => {
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
    });
    const exited = once(child, 'exit');
    let out = '';
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (s: string) => { out += s; });
    assert.ok(await within(10_000, () => out.includes('ready')), 'the child never started its fake');
    const pid = Number(readFileSync(path.join(dir, 'pid'), 'utf8'));
    assert.ok(!groupGone(pid), 'the fake is not running');
    if (how === 'SIGINT' || how === 'SIGKILL') child.kill(how);
    const [code, signal] = await exited;
    assert.ok(code !== 0 || signal !== null, 'the child did not die');
    assert.ok(await within(3_000, () => groupGone(pid)), `the fake's group ${pid} outlived its test process`);
  });
}
