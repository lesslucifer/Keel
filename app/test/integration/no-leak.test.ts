// The tests' own promise: a fake that hangs never outlives the process that spawned it, however that
// process ends. A child node stands in for a test process: it spawns a hung fake through capture()
// (its own process group, a 60 s timeout, so the main part never kills it here), then dies by ^C, by
// an uncaught throw, by a failed assertion, or by SIGKILL. The fake's whole group must be gone soon after.
// The same holds for the background jobs that holder() starts, whether or not they left their group.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { readFileSync } from 'node:fs';
import { tempDir, program, HANG, groupGone, within, alive } from '../support/fs.ts';

const SUPPORT = pathToFileURL(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'support', 'fs.ts')).href;
const COMMANDS = pathToFileURL(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'src', 'main', 'commands.ts')).href;

type How = 'SIGINT' | 'throw' | 'assert' | 'SIGKILL';

/** Spawn a child node that starts a hung fake through capture(), end the child by `how`, and hold the fake's group gone. */
async function dies(t: { after: (fn: () => void) => void }, how: How): Promise<void> {
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
}

test('a hung fake\'s process group is gone soon after its test process dies by ^C (SIGINT)', (t) => dies(t, 'SIGINT'));
test('a hung fake\'s process group is gone soon after its test process dies by an uncaught throw (a panic)', (t) => dies(t, 'throw'));
test('a hung fake\'s process group is gone soon after its test process dies by a failed assertion', (t) => dies(t, 'assert'));
test('a hung fake\'s process group is gone soon after its test process dies by SIGKILL', (t) => dies(t, 'SIGKILL'));

/** As dies(), for a background job from holder() (one that left its group, or not) started by the child: gone soon after the child dies. */
async function holderDies(t: { after: (fn: () => void) => void }, how: How, escape: boolean): Promise<void> {
  const dir = tempDir(t, 'leak');
  const pidFile = path.join(dir, 'pid');
  const script = [
    `import { holder } from ${JSON.stringify(SUPPORT)};`,
    `import { spawn } from 'node:child_process';`,
    `import assert from 'node:assert/strict';`,
    `import { existsSync, readFileSync } from 'node:fs';`,
    `setInterval(() => { try { process.kill(${process.pid}, 0); } catch { process.exit(1); } }, 100);`,
    `spawn('/bin/sh', ['-c', holder(${JSON.stringify(pidFile)}, ${escape})], { stdio: 'ignore' });`,
    `const wait = setInterval(() => {`,
    `  if (!existsSync(${JSON.stringify(pidFile)}) || readFileSync(${JSON.stringify(pidFile)}, 'utf8').trim() === '') return;`,
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
  assert.ok(await within(10_000, () => out.includes('ready')), 'the child never started its background job');
  const pid = Number(readFileSync(pidFile, 'utf8'));
  assert.ok(alive(pid), 'the background job is not running');
  if (how === 'SIGINT' || how === 'SIGKILL') child.kill(how);
  const [code, signal] = await exited;
  assert.ok(code !== 0 || signal !== null, 'the child did not die');
  assert.ok(await within(3_000, () => !alive(pid)), `the background job ${pid} outlived its test process`);
}

for (const how of ['SIGINT', 'throw', 'assert', 'SIGKILL'] as const) {
  test(`a background job that left its process group is gone soon after its test process dies by ${how}`, (t) => holderDies(t, how, true));
  test(`a background job in its shell's process group is gone soon after its test process dies by ${how}`, (t) => holderDies(t, how, false));
}
