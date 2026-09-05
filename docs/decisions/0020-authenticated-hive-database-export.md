# ADR 0020: Authenticated Hive database export

Status: **Accepted and implemented**

## Context

Dogfooding needs a user-visible way to leave the installation with its durable
Hive data. Copying the live SQLite files is not a consistent backup, and a
browser export must not expose the operator credential or repository contents.

## Decision

The operator-only API creates an online SQLite backup through the persistence
owner and returns it as a no-store attachment. The Settings surface downloads
that consistent file with an explicit private-data warning. It includes the
canonical Hive database: identity, workers, provider conversation bindings,
tasks, decisions, policies, notification keys, and audit state. It excludes
repositories, terminal output/history, provider login material, deployment
secrets, and machine-specific workspace-root configuration.

Restore is deliberately not performed by the running web process. The package
lifecycle provides a verified, offline restore that checks integrity and
compatibility, creates a rollback snapshot, replaces state while the API is
stopped, restarts only the API, and rolls back on failed health. A later full
encrypted export may combine the database with explicitly selected portable
configuration; it must never silently copy machine credentials.

## Consequences

### Bounded export delivery — 2026-09-04

One per-API export permit admits preparation and transfer without an application
wait queue. SQLite snapshot preparation runs on a blocking executor, not inline
in an async request. The preparation job owns its permit even after request
timeout/disconnect, preventing overlapping detached backups. The request waits
at most 60 seconds; this does not forcibly cancel an in-flight SQLite/filesystem
operation or remove contention on the shared persistence connection.

Completed snapshots up to 2 GiB stream through two queued 64 KiB chunks rather
than a whole-file allocation. One producer owns the temporary file and permit
with a 120-second transfer deadline; disconnect releases its receiver. The body
checks expected length and reports premature EOF as failure. These bounds apply
to export delivery, not total API memory, SQLite lock latency or measured CPU.

- The first useful backup is consistent and immediately available to the
  operator without interrupting workers.
- The downloaded database is sensitive and unencrypted, so the UI warns the
  operator and the response cannot be cached.
- Repositories and host-specific secrets cannot be mistaken for portable Hive
  state.
- Restore is an explicit package command and preserves the terminal host and
  repositories while restarting only the API.

## Validation

- The route requires operator authentication.
- SQLite's online backup produces a reopenable database that passes integrity
  checks.
- The response is a no-store attachment with a stable SQLite media type.
- Browser tests verify the Settings action downloads the returned snapshot.
- Package lifecycle tests prove restore verification, API-only restart, and
  terminal-host preservation.

## Restore safety refinement — 2026-09-04

Restore stages a private copy before invoking the installed verifier, because
verification may migrate an older schema. The selected backup is never opened
for mutation by restore. The rollback download remains an unarmed candidate
until fully downloaded and verified; failure before replacement cannot activate
rollback with incomplete bytes. After a successful API stop, replacement arms
the existing rollback path. Successful restoration retains the previous database
and prints its path, keeping the current pre-restore snapshot plus the two newest
other managed pre-restore snapshots. Manual backups are outside this retention.
Failed restores retain their recovery snapshot rather than pruning evidence.

Before replacement or rollback, the package requires a successful API stop and
an explicit inactive/failed ActiveState reading; missing or unreadable state
refuses database replacement. Rollback stages a complete replacement before
renaming it, restarts only the API, and verifies health. An incomplete rollback
is reported as such with the retained recovery path, not hidden behind ignored
command failures. No terminal-host stop is authorized by this recovery path.

### Explicit offline corruption recovery — 2026-09-05

`restore-offline DATABASE` is a separate operator command, never an automatic
fallback from a failed online export. It verifies a private copy of the selected
backup before stopping anything, confirms the API is inactive, and copies the
original database and any WAL/SHM sidecars into a private recovery archive before
replacement. The original files are unverified evidence, not a healthy rollback
snapshot. A copy-complete marker distinguishes a finished archive from a partial
copy failure. The selected backup remains unchanged.

Both restore modes share a nonblocking process-owned restore lock. Offline
archives are bounded to three; a fourth recovery refuses before stopping the
API and asks the operator to inspect and move an archive. No damaged evidence
is automatically pruned. Normal backup retention does not touch these archives.
Offline recovery must not overlap installation/update operations; the restore
lock serializes restores, not all package actions.

After replacement, only the API starts. Failed startup or an interrupted restore
attempts to stop it and reports whether that stop was confirmed. Damaged files
are never automatically reinstalled. Failed recovery leaves the candidate and
available archive for inspection and does not claim rollback succeeded. The
terminal host and repositories remain outside the restore operation.

Package lifecycle tests cover invalid input, competing restore, unreadable stop
state, unavailable export, source preservation during migration, raw sidecar
preservation, archive permissions/bounds, failed health and refused failure stop.
The real isolated API/SQLite drill additionally proves corrupt startup refusal,
restored task identity, unchanged source backup and preserved damaged bytes.
It uses an owned-process service adapter, not the live systemd units, and leaves
the production Hive untouched. Corruption consequence routing and containment
remain separate REC-02 acceptance gates.

Normal file-backed startup also checks SQLite integrity before changing journal
mode or running schema migrations. Post-migration validation remains in place.
A regression seeds an invalid checked value at an older schema and proves that
refusal preserves the exact database bytes and old schema version. This is a
startup safeguard, not a runtime integrity-failure latch or operator notification.
