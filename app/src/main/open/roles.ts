// Open's one question to keel (W12 review round 1, blockers docs.ts:172 and docs.ts:127; owner ruling
// 2026-11-23): which roles, id and page each data file gives, as keel doc-roles reads them - the same
// reading keel verify-doc and keel mr confirm use - so Open finds the page keel signs and refuses what keel
// refuses. The app keeps no YAML reader of its own. Asked in batches of DOC_ROLES_BATCH files.

import { runDocRoles, DOC_ROLES_BATCH, type RunOptions } from '../commands.ts';
import { docRolesWhy, parseDocRoles, type DataHead } from './docs.ts';

/** Each file's head as keel reads it (absolute .yaml paths on disk), or why keel gave none. */
export async function docHeads(keel: string, files: readonly string[], opts: Omit<RunOptions, 'before245'>): Promise<Map<string, DataHead> | string> {
  const heads = new Map<string, DataHead>();
  for (let i = 0; i < files.length; i += DOC_ROLES_BATCH) {
    const batch = files.slice(i, i + DOC_ROLES_BATCH);
    const r = await runDocRoles(keel, batch, opts);
    const why = docRolesWhy(r);
    if (why !== null) return why;
    const got = parseDocRoles(r.stdout, batch);
    if (got === null) return "keel's doc-roles answer is not one this app reads (doc-roles/v1): update keel";
    for (const [file, head] of got) heads.set(file, head);
  }
  return heads;
}
