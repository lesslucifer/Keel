// Temp folders, fake programs and snapshots for the integration tests. Everything lives under
// os.tmpdir() and is removed when its test ends. A fake that can hang never outlives the test
// process: it polls its parent and exits once the parent is gone (a panic, a failed assertion, ^C).
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

/**
 * Shell lines that block "forever" but never outlive the process that spawned this script: the
 * loop ends within a tenth of a second of that parent's death, however the parent died.
 */
export const HANG = 'while kill -0 "$PPID" 2>/dev/null; do /bin/sleep 0.1; done';

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
