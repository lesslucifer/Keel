// Temp folders, fake programs and snapshots for the integration tests. Everything lives under
// os.tmpdir() and is removed when its test ends. A fake that can hang never outlives the test
// process: it polls its parent and exits once the parent is gone (a panic, a failed assertion, ^C);
// a background job from holder() polls the test process itself, since its parent is meant to exit first.
import { mkdtempSync, realpathSync, rmSync, writeFileSync, chmodSync, existsSync, readdirSync, lstatSync, readlinkSync, readFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';

/** A fresh temp folder by its real path (macOS's /var is /private/var), removed after the test. */
export function tempDir(t: { after: (fn: () => void) => void }, tag: string): string {
  const dir = realpathSync(mkdtempSync(path.join(os.tmpdir(), `keel-app-${tag}-`)));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}

/** An executable file holding `body`; returns its path. */
export function program(file: string, body: string): string {
  writeFileSync(file, body);
  chmodSync(file, 0o755);
  return file;
}

/** A fake program (Node) that writes its argv and whole environment as JSON to `record`, prints `stdout`, exits `code`. */
export function recorder(file: string, record: string, code = 0, stdout = ''): string {
  return program(
    file,
    `#!${process.execPath}\nrequire('node:fs').writeFileSync(${JSON.stringify(record)}, JSON.stringify({ argv: process.argv.slice(2), env: process.env }));\nprocess.stdout.write(${JSON.stringify(stdout)});\nprocess.exit(${code});\n`,
  );
}

/** Which old git a fake stands for: its version, and the language its messages come in. */
export interface OldGitAs {
  /** 2.39 and 2.30: "unknown option: %s"; 2.16: "Unknown option: %s"; 2.14: has no --no-optional-locks either; 2.30 and 2.16: no rev-parse --path-format. */
  version?: '2.39' | '2.30' | '2.16' | '2.14';
  /** The translation git's gettext picked (git before 2.45 wraps the line in _()). */
  lang?: 'en' | 'de' | 'fr';
}

/**
 * A fake git older than 2.45: the first locked flag it does not know (--no-lazy-fetch, or before
 * 2.15 --no-optional-locks) is answered as that git's git.c does - its "unknown option" line in its
 * language and case, the usage text, exit 129. Before 2.31 rev-parse does not know --path-format and
 * echoes it as an argument, as git 2.30's rev-parse does. Else it runs `real` with the same argv and
 * environment. Each call appends {argv, env} as one JSON line to `record`.
 */
export function oldGit(file: string, record: string, real: string, as: OldGitAs = {}): string {
  const version = as.version ?? '2.39';
  const usage = 'usage: git [--version] [--help] [-C <path>] [-c <name>=<value>]\n           [--exec-path[=<path>]] [--html-path] [--man-path] [--info-path]\n';
  const line = { en: version === '2.16' || version === '2.14' ? 'Unknown option: %s' : 'unknown option: %s', de: 'Unbekannte Option: %s', fr: 'option inconnue : %s' }[as.lang ?? 'en'];
  const unknown = version === '2.14' ? ['--no-optional-locks', '--no-lazy-fetch'] : ['--no-lazy-fetch'];
  const noPathFormat = version === '2.30' || version === '2.16' || version === '2.14';
  return program(
    file,
    [
      `#!${process.execPath}`,
      `const fs = require('node:fs');`,
      `let argv = process.argv.slice(2);`,
      `fs.appendFileSync(${JSON.stringify(record)}, JSON.stringify({ argv, env: process.env }) + '\\n');`,
      `const bad = argv.find((a) => ${JSON.stringify(unknown)}.includes(a));`,
      `if (bad !== undefined) { process.stderr.write(${JSON.stringify(line)}.replace('%s', bad) + '\\n' + ${JSON.stringify(usage)}); process.exit(129); }`,
      `if (${noPathFormat} && argv.includes('rev-parse') && argv.includes('--path-format=absolute')) { process.stdout.write('--path-format=absolute\\n'); argv = argv.filter((a) => a !== '--path-format=absolute'); }`,
      `const r = require('node:child_process').spawnSync(${JSON.stringify(real)}, argv, { stdio: 'inherit', env: process.env });`,
      `process.exit(r.status === null ? 1 : r.status);`,
    ].join('\n') + '\n',
  );
}

/** The JSON lines a recorder appended, in order. */
export function records(file: string): Array<{ argv: string[]; env: Record<string, string> }> {
  if (!existsSync(file)) return [];
  return readFileSync(file, 'utf8').split('\n').filter((l) => l !== '').map((l) => JSON.parse(l) as { argv: string[]; env: Record<string, string> });
}

/**
 * Shell lines that block "forever" but never outlive the process that spawned this script: the
 * loop ends within a tenth of a second of that parent's death, however the parent died.
 */
export const HANG = 'while kill -0 "$PPID" 2>/dev/null; do /bin/sleep 0.1; done';

/** Wait until `done()` holds, checking every 20 ms, for at most `ms`; whether it held. */
export async function within(ms: number, done: () => boolean): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (done()) return true;
    await new Promise((r) => setTimeout(r, 20));
  }
  return done();
}

/** `p`, or a rejection naming `what` once `ms` pass first: a hang fails the test instead of stalling it. */
export function bounded<T>(ms: number, what: string, p: Promise<T>): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what}: still pending after ${ms} ms`)), ms);
  });
  return Promise.race([p, late]).finally(() => clearTimeout(timer));
}

/**
 * A shell line that starts a background job holding the script's stdout and stderr, as an rc file's
 * `cmd &` does. The job writes its pid to `pidFile` and ends within a tenth of a second of this test
 * process's death, or after 30 s, whichever comes first, so it never outlives the test. With `escape`
 * it first leaves the script's process group (perl's setpgrp, as `setsid cmd &` would), so a group
 * kill cannot reach it. Register `endHolder(t, pidFile)` to end it at the test's end.
 */
export function holder(pidFile: string, escape: boolean): string {
  const loop = `echo $$ > ${pidFile}; n=0; while kill -0 ${process.pid} 2>/dev/null && [ $n -lt 300 ]; do /bin/sleep 0.1; n=$((n+1)); done`;
  return escape ? `/usr/bin/perl -e 'setpgrp(0,0); exec "/bin/sh", "-c", $ARGV[0] or die' '${loop}' &` : `/bin/sh -c '${loop}' &`;
}

/** At the test's end, kill the holder in `pidFile` and its group, if still there. */
export function endHolder(t: { after: (fn: () => void) => void }, pidFile: string): void {
  t.after(() => {
    let pid: number;
    try {
      pid = Number(readFileSync(pidFile, 'utf8'));
    } catch {
      return;
    }
    if (!Number.isInteger(pid) || pid <= 1) return;
    for (const target of [-pid, pid]) {
      try {
        process.kill(target, 'SIGKILL');
      } catch {
        // already gone
      }
    }
  });
}

/** Whether process `pid` is still running. */
export function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'EPERM';
  }
}

/** Whether no process is left in the process group `pgid` (a detached child's group is its pid). */
export function groupGone(pgid: number): boolean {
  try {
    process.kill(-pgid, 0);
    return false;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'ESRCH';
  }
}

/** The real git on this machine's PATH. */
export function realGit(): string {
  for (const dir of (process.env.PATH ?? '').split(':')) {
    if (dir.startsWith('/') && existsSync(path.join(dir, 'git'))) return path.join(dir, 'git');
  }
  throw new Error('no git on PATH');
}

/** Real git run by the test to set repos up: no user or system config, no hooks, no signing. */
export function sh(git: string, cwd: string, args: string[]): string {
  return execFileSync(
    git,
    ['-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'init.defaultBranch=main', ...args],
    { cwd, encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: cwd, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LANG: 'C' } },
  );
}

/** Every path under `root` (.git included) with its type, size, mtime, mode and bytes: equal snapshots mean nothing was written. */
export function snapshot(root: string): Record<string, string> {
  const out: Record<string, string> = {};
  const walk = (dir: string) => {
    for (const name of readdirSync(dir).sort()) {
      const p = path.join(dir, name);
      const st = lstatSync(p);
      const rel = path.relative(root, p);
      if (st.isSymbolicLink()) out[rel] = `link ${readlinkSync(p)} ${st.mtimeMs}`;
      else if (st.isDirectory()) {
        out[rel] = `dir ${st.mtimeMs} ${st.mode}`;
        walk(p);
      } else out[rel] = `file ${st.size} ${st.mtimeMs} ${st.mode} ${createHash('sha256').update(readFileSync(p)).digest('hex')}`;
    }
  };
  walk(root);
  return out;
}

/** mkdir -p, returning the folder. */
export function mk(dir: string): string {
  mkdirSync(dir, { recursive: true });
  return dir;
}
