# Independent review checklists (router-run, read-only)

For what the cr gate never sees (SKILL.md "Independent review"): a read-only
subagent on the strongest model, set explicitly, fresh per round; prompt from
`messages.md` "Independent review prompt", saying what earlier rounds covered.
Pick the sections the diff touches; the project-router skill lists the paths.

Ported from Trawler's `trawler-router/references/review-checklists.md`
(2026-11-06). ★ = a hunt that found a real defect there, kept as lineage: the
"(Trawler: …)" text is the evidence, the hunt above it is the generic rule.
Trawler's A–C were about its own code (desktop-app quit paths, a native login-item
crate, the updater signing key); here they are generalised. D–F are generic as
they were.

**A. Exit / shutdown paths** (anything that quits, restarts or tears down)
- ★ Every exit path runs the same safety check (Trawler: a failed window reopen exited without restoring the system proxy; a stale quit flag quit later).
- Failure mid-shutdown, stale flags, races between two quit triggers, double requests; the app is never left unreachable.
- Updates and restarts go through the same exit path.

**B. `unsafe` / native / FFI**
- ★ `unsafe` confined to one module; a guard test catches crate-level and item-level `allow(unsafe_code)` (Trawler: DEC-84).
- Each foreign call: lifetimes, null, thread, error mapping, no panic across the boundary.
- ★ OS-version availability guarded at runtime when the API is newer than the minimum supported OS (Trawler: SMAppService is macOS 13+, the app's minimum was 11; the lookup would abort).

**C. Secrets and signing keys**
- ★ Secrets by name, only in the step that needs them; presence checks are booleans; no package install, lifecycle script or tool downloaded at build time runs beside the key (Trawler: AppImage tooling downloads, pnpm's verify-deps-before-run).
- Signatures bind the version; a tampered artifact is refused.
- Updater/installer: TLS and redirects, temp files, privileged install steps, command injection.

**D. Release / CI workflows** (`.github/workflows/*.yml`, release code)
- Trigger + tag == version; `${{ }}` of refs/tags reaches `run:` only through `env:`.
- Every `uses:` pinned to a full SHA matching its tag; no caches; `persist-credentials: false`.
- ★ `pipefail` with `| grep -q` / `| head` → exit 141: capture, then compare with `[ ]` (Trawler: release run 1 died at step 1).
- Runner labels exist; arm64 rows build arm64; `fail-fast: false` until green once.
- Artifact names and flattening match what the release step expects; collisions fail loudly.
- ★ The version string is accepted by every packager it reaches, checked in the packager's source at the pinned tag (Trawler: rpm rejected `0.1.0-1`, WiX rejected `rc.1`). `actionlint` clean.

**E. Tests that could touch the owner's machine**
- ★ HOME / config dir is a temp dir, asserted before any delete or write (Trawler: a spec deleting the real `keep-capturing.json`).
- ★ Fixed ports and single-instance sockets: the test refuses to run when the owner's app holds them (Trawler: a stray copy of the app on its fixed port answered a test).
- Every failure path detaches mounts, kills only its own pids (no broad `pgrep -f`), leaves nothing listening.
- Specs independent of leftover state; wait for state instead of reading immediately.

**F. Back-merges** (main into an epic, an epic into a WU)
- ★ Per conflicted file, compare the result with both parents (Trawler: a dropped guard; `lessons.md` B9).
- No duplicated rows in index docs (decisions, use cases); the merge message's claims match its content.
