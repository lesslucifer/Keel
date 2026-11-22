// The fixed commands at their real boundary, a spawned program: a fake git records its argv and
// environment; real git in temp repos answers from the repo named, never from an inherited GIT_*.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync, writeFileSync, mkdirSync, readFile } from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { run, capture, LOCKED, OUTPUT_CAP, LATE_OUTPUT_CAP } from '../../src/main/commands.ts';
import { commitsConfig } from '../../src/main/projects.ts';
import { tempDir, recorder, program, realGit, sh, HANG, groupGone, holder, endHolder, bounded, alive, within } from '../support/fs.ts';

const COMMANDS = pathToFileURL(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'src', 'main', 'commands.ts')).href;

// The app's own environment carries git settings that would redirect or hijack git if they reached it.
process.env.GIT_DIR = '/nonexistent/hijack/.git';
process.env.GIT_WORK_TREE = '/nonexistent';
process.env.GIT_CONFIG_PARAMETERS = "'core.fsmonitor'='true'";
process.env.GIT_EXTERNAL_DIFF = '/nonexistent/diff';
process.env.KEEL_TEST_SECRET = 'must-not-leak';

test('a fake git gets the locked flags first, the slot whole, no shell, and no GIT_* or other inherited variable', async (t) => {
  const dir = tempDir(t, 'run');
  const record = path.join(dir, 'record.json');
  const git = recorder(path.join(dir, 'git'), record, 0, '/somewhere\n');
  const project = path.join(dir, `$(touch pwned); x \`touch ${dir}/pwned2\``);
  const r = await run(git, 'toplevel', { project }, { path: ['/usr/bin', '/bin'] });
  assert.deepEqual([r.code, r.stdout, r.timedOut, r.startError], [0, '/somewhere\n', false, null]);
  const seen = JSON.parse(readFileSync(record, 'utf8')) as { argv: string[]; env: Record<string, string> };
  assert.deepEqual(seen.argv, ['-c', 'core.fsmonitor=false', '--no-optional-locks', '--no-lazy-fetch', '-C', project, 'rev-parse', '--show-toplevel']);
  assert.deepEqual(seen.argv.slice(0, LOCKED.length), LOCKED);
  assert.ok(!existsSync(path.join(dir, 'pwned')) && !existsSync(path.join('/', 'pwned')) && !existsSync(path.join(dir, 'pwned2')), 'a shell ran the slot');
  assert.equal(seen.env.PATH, '/usr/bin:/bin');
  // git's messages in English whatever the Mac's language: the app reads some of them.
  assert.equal(seen.env.LC_ALL, 'C');
  // __CF_USER_TEXT_ENCODING is set by macOS's CoreFoundation inside the child itself (env -i node shows it too).
  for (const key of Object.keys(seen.env)) {
    assert.ok(!key.startsWith('GIT_'), `${key} reached git`);
    assert.ok(['PATH', 'HOME', 'USER', 'LOGNAME', 'LANG', 'TMPDIR', 'LC_ALL', '__CF_USER_TEXT_ENCODING'].includes(key), `${key} reached git`);
  }
});

test('a fake git that exits non-zero is reported with its code and stderr, not thrown', async (t) => {
  const dir = tempDir(t, 'run');
  const git = program(path.join(dir, 'git'), "#!/bin/sh\necho 'fatal: not a git repository' >&2\nexit 128\n");
  const r = await run(git, 'toplevel', { project: dir }, { path: ['/usr/bin', '/bin'] });
  assert.deepEqual([r.code, r.timedOut], [128, false]);
  assert.match(r.stderr, /not a git repository/);
});

test('a git that hangs is killed at the timeout with its whole process group, and reported so', async (t) => {
  const dir = tempDir(t, 'run');
  const git = program(path.join(dir, 'git'), `#!/bin/sh\necho $$ > ${dir}/pid\n${HANG}\n`);
  const r = await run(git, 'toplevel', { project: dir }, { path: ['/usr/bin', '/bin'], timeoutMs: 300 });
  assert.deepEqual([r.timedOut, r.code], [true, null]);
  assert.ok(groupGone(Number(readFileSync(path.join(dir, 'pid'), 'utf8'))), 'the hung git or its child is still running');
});

test('a program that exits while a descendant that left its process group still holds its output is answered from its own exit, within the timeout', async (t) => {
  const dir = tempDir(t, 'run');
  const pidFile = path.join(dir, 'holder');
  endHolder(t, pidFile);
  const fake = program(path.join(dir, 'fake'), `#!/bin/sh\necho answer\n${holder(pidFile, true)}\nexit 0\n`);
  const r = await bounded(6_000, 'capture', capture(fake, [], { PATH: '/usr/bin:/bin' }, 2_000));
  assert.deepEqual([r.code, r.timedOut, r.startError, r.stdout], [0, false, null, 'answer\n']);
});

test('a program that exits 0 while its own background job holds its output is answered with its exit code, and the job is never killed', async (t) => {
  const dir = tempDir(t, 'run');
  const pidFile = path.join(dir, 'holder');
  endHolder(t, pidFile);
  const fake = program(path.join(dir, 'fake'), `#!/bin/sh\necho answer\n${holder(pidFile, false)}\nexit 0\n`);
  const start = Date.now();
  const r = await capture(fake, [], { PATH: '/usr/bin:/bin' }, 1_500);
  assert.deepEqual([r.code, r.timedOut, r.startError, r.stdout], [0, false, null, 'answer\n']);
  assert.ok(await within(3_000, () => existsSync(pidFile) && readFileSync(pidFile, 'utf8').trim() !== ''), 'the background job never started');
  const pid = Number(readFileSync(pidFile, 'utf8'));
  // Past the timeout the program had: its group is never killed after it exited by itself.
  await new Promise((resolve) => setTimeout(resolve, Math.max(0, start + 2_000 - Date.now())));
  assert.ok(alive(pid), 'the background job was killed after the program had exited by itself');
});

test('a background job the program left keeps writing to its output after the answer: the pipes are drained, never closed under it', async (t) => {
  const dir = tempDir(t, 'run');
  const after = path.join(dir, 'after');
  // The job ends by itself within half a second; SIGPIPE on a closed pipe would end it before 'after'.
  const fake = program(path.join(dir, 'fake'), `#!/bin/sh\n( /bin/sleep 0.5; echo late; echo late >&2; echo late; /usr/bin/touch ${after} ) &\necho done\n`);
  const r = await capture(fake, [], { PATH: '/usr/bin:/bin' }, 5_000);
  assert.deepEqual([r.code, r.timedOut, r.stdout], [0, false, 'done\n']);
  assert.ok(await within(5_000, () => existsSync(after)), "the job died writing to the program's output after the answer");
});

test('a job the program left that writes without stopping is read only up to a bound after the answer, then waits, unharmed', async (t) => {
  const dir = tempDir(t, 'run');
  const count = path.join(dir, 'count');
  const pidFile = path.join(dir, 'job');
  endHolder(t, pidFile);
  // 8 KiB per write, shell builtins only (no fork per write, so it writes as fast as it is read). It
  // counts its writes, stops after 4000 (32 MiB, far past the bound), and ends within 0.1 s of this
  // test process's death. It starts 0.5 s after launch, after the answer (0.2 s after the program's exit).
  const job = `s=$(/usr/bin/printf '%08192d' 0); /bin/sleep 0.5; echo $$ > ${pidFile}; n=0; while kill -0 ${process.pid} 2>/dev/null && [ $n -lt 4000 ]; do printf '%s\\n' "$s"; n=$((n+1)); echo $n > ${count}; done`;
  const fake = program(path.join(dir, 'fake'), `#!/bin/sh\n/bin/sh -c '${job}' &\necho done\n`);
  const r = await capture(fake, [], { PATH: '/usr/bin:/bin' }, 5_000);
  assert.deepEqual([r.code, r.timedOut, r.stdout], [0, false, 'done\n']);
  const writes = () => Number(existsSync(count) ? readFileSync(count, 'utf8').trim() || '0' : '0');
  // The job stops advancing: its count stays the same for a whole second (its writes block on the
  // full pipe). Read forever, it would run on to 4000 writes before it stood still.
  let last = -1;
  let since = Date.now();
  const still = await within(20_000, () => {
    const n = writes();
    if (n !== last) {
      last = n;
      since = Date.now();
    }
    return n > 0 && Date.now() - since >= 1_000;
  });
  assert.ok(still, `the job never stood still (${writes()} writes of 8 KiB)`);
  // It stood still within the late-output bound plus what the pipe holds, and it is alive.
  assert.ok(writes() * 8192 <= LATE_OUTPUT_CAP + 512 * 1024, `${writes()} writes of 8 KiB were read after the answer`);
  assert.ok(alive(Number(readFileSync(pidFile, 'utf8'))), 'the job was killed');
});

test('a program that exits while the event loop is blocked past its timeout is answered from its exit, not killed and reported timed out', async (t) => {
  const dir = tempDir(t, 'run');
  const fake = program(path.join(dir, 'fake'), '#!/bin/sh\n/bin/sleep 0.05\necho quick\n');
  const p = capture(fake, [], { PATH: '/usr/bin:/bin' }, 150);
  // Block the loop inside an I/O callback: the program exits meanwhile, and on the next turn the
  // timer comes due before the poll that reads that exit.
  await new Promise<void>((resolve) =>
    readFile('/dev/null', () => {
      const until = Date.now() + 500;
      while (Date.now() < until) {
        // blocked
      }
      resolve();
    }),
  );
  const r = await p;
  assert.deepEqual([r.code, r.timedOut, r.stdout], [0, false, 'quick\n']);
});

test('with no file descriptor left, a program that cannot start is reported (EMFILE), never a crash', async (t) => {
  const dir = tempDir(t, 'run');
  const script = [
    `import { capture } from ${JSON.stringify(COMMANDS)};`,
    `import { openSync, writeSync } from 'node:fs';`,
    `setInterval(() => { try { process.kill(${process.pid}, 0); } catch { process.exit(1); } }, 100).unref();`,
    // The answer goes out through a file opened before the descriptors run out.
    `const outFd = openSync(${JSON.stringify(path.join(dir, 'answer.json'))}, 'w');`,
    `const say = (v) => { writeSync(outFd, JSON.stringify(v)); process.exit(0); };`,
    `process.on('uncaughtException', (e) => say({ crash: String(e) }));`,
    `process.on('unhandledRejection', (e) => say({ crash: String(e) }));`,
    `const held = [];`,
    `try { for (;;) held.push(openSync('/dev/null', 'r')); } catch {}`,
    `const r = await capture('/bin/echo', ['x'], { PATH: '/usr/bin:/bin' }, 2000);`,
    `say({ code: r.code, startError: r.startError, timedOut: r.timedOut });`,
  ].join('\n');
  execFileSync('/bin/sh', ['-c', `ulimit -n 128 && exec "$0" --input-type=module -e "$1"`, process.execPath, script], { cwd: dir, stdio: 'ignore', timeout: 20_000 });
  assert.deepEqual(JSON.parse(readFileSync(path.join(dir, 'answer.json'), 'utf8')), { code: null, startError: 'EMFILE', timedOut: false });
});

test('a git that cannot start is reported with the reason, not thrown', async (t) => {
  const dir = tempDir(t, 'run');
  const r = await run(path.join(dir, 'no-such-git'), 'toplevel', { project: dir }, { path: ['/usr/bin'] });
  assert.deepEqual([r.code, r.startError], [null, 'ENOENT']);
});

test('output past the cap is dropped, and the run still ends', async (t) => {
  const dir = tempDir(t, 'run');
  const big = program(path.join(dir, 'big'), `#!/bin/sh\n/usr/bin/head -c ${OUTPUT_CAP + 4096} /dev/zero\n`);
  const r = await capture(big, [], { PATH: '/usr/bin:/bin' }, 10_000);
  assert.equal(r.code, 0);
  assert.equal(r.stdout.length, OUTPUT_CAP);
  assert.equal(r.cut, true, 'a cut answer says so: a reader must not take it as whole');
});

test('an answer within the cap is not cut, on stdout or stderr; one past it on stderr is', async (t) => {
  const dir = tempDir(t, 'run');
  const small = program(path.join(dir, 'small'), '#!/bin/sh\necho out; echo err >&2\n');
  assert.equal((await capture(small, [], { PATH: '/usr/bin:/bin' }, 10_000)).cut, false);
  const exact = program(path.join(dir, 'exact'), `#!/bin/sh\n/usr/bin/head -c ${OUTPUT_CAP} /dev/zero\n`);
  assert.equal((await capture(exact, [], { PATH: '/usr/bin:/bin' }, 10_000)).cut, false);
  const noisy = program(path.join(dir, 'noisy'), `#!/bin/sh\n/usr/bin/head -c ${OUTPUT_CAP + 1} /dev/zero >&2\n`);
  assert.equal((await capture(noisy, [], { PATH: '/usr/bin:/bin' }, 10_000)).cut, true);
});

test('a git path that is not absolute is refused before anything runs', async () => {
  await assert.rejects(run('git', 'toplevel', { project: '/p' }, { path: ['/usr/bin'] }), /absolute/);
});

test('real git answers from the project named, not from the GIT_DIR the app inherited', async (t) => {
  const git = realGit();
  const dir = tempDir(t, 'run');
  const a = path.join(dir, 'a');
  const b = path.join(dir, 'b');
  mkdirSync(a);
  mkdirSync(b);
  sh(git, a, ['init', '-q']);
  sh(git, b, ['init', '-q']);
  process.env.GIT_DIR = path.join(b, '.git');
  const opts = { path: [path.dirname(git), '/usr/bin', '/bin'] };
  const top = await run(git, 'toplevel', { project: a }, opts);
  assert.equal(top.code, 0, top.stderr);
  assert.equal(top.stdout.trim(), a);
  assert.equal(path.resolve(a, (await run(git, 'common-dir', { project: a }, opts)).stdout.trim()), path.join(a, '.git'));
  const hasConfig = async (project: string) => {
    const r = await run(git, 'has-config', { project }, opts);
    return r.code === 0 && commitsConfig(r.stdout);
  };
  assert.equal(await hasConfig(a), false);
  mkdirSync(path.join(a, '.keel'));
  writeFileSync(path.join(a, '.keel', 'config.toml'), 'code_paths = []\n');
  sh(git, a, ['add', '.keel/config.toml']);
  sh(git, a, ['commit', '-q', '-m', 'keel']);
  assert.equal(await hasConfig(a), true);
  assert.equal(await hasConfig(b), false);
});
