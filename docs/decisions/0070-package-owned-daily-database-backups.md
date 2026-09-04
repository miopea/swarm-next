# ADR 0070: Package-owned daily database snapshots

Status: Accepted implementation of REC-02; runtime attention and real SQLite
recovery acceptance remain open.

## Decision

The Linux package owns one systemd calendar job at 03:00 UTC with up to fifteen
minutes of jitter and persistent catch-up. It runs `backup-daily`, uses the
existing authenticated online SQLite export, and never stops or starts workers.
It does not require/start the API when unavailable. A failure remains a failed
job, not permission to copy a live database's raw files or delete older backups.

One nonblocking process-owned flock prevents duplicate manual/timer work. A UTC
date names one published snapshot; a repeated successful day performs no new
download. Private fixed staging paths bound remnants after abrupt termination;
normal exit/signals clean them. Download is limited to 120 seconds and 2 GiB,
verification to 60 seconds, and the systemd job to 190 seconds. Oversized or
unverifiable databases fail explicitly and leave retained copies unchanged.

Verification runs on the candidate before atomic publication. Only afterward
does pruning retain today's snapshot plus six newest other managed daily files.
Pre-update, pre-restore and manually named snapshots are separate ownership
classes and are not pruned by the daily job. Database exports contain sensitive
unencrypted Hive state; files use 0600 and the backup directory uses 0700.

Package unit installation enables the timer when the bundle supplies its units.
This package removes/disables the units when installing an older bundle without
them. The service also checks for its template in the current release, so an
older updater switching to a pre-feature release cannot execute an unsupported
command. Uninstall removes the units while preserving state. This compatibility
guard is package-owned until pre-feature release rollback is no longer supported.

## Verification and remaining work

The package publishes one atomic `daily-backup.status` file under managed state.
Failures use the existing bounded failure shape (step, sanitized detail, changed);
success replaces it with `state=ready` and the available UTC snapshot day. A
same-day no-op reports the existing snapshot, not a new verification. Contending
jobs do not modify the status. The file is a subsystem observation, not an
operator decision or proof that older snapshots remain uncorrupted. Runtime UI
consumption uses the existing authenticated resource response, with no new browser
poll. The API bounds the file read to 1 KiB, rejects duplicate fields and invalid
dates, and projects only failed/unavailable/not-reported or ready plus snapshot
day. Raw failure details are not exported. The runtime area shows failure or
unavailable evidence and links to Backup settings; ready removes the notice.
Missing status is not represented as proven backup health. Live scheduling and
UI acceptance remain separate from this implementation wiring.

Isolated package smoke covers verification failure without pruning, exact seven
retained files, manual-file preservation, same-day idempotence, lock contention,
unit installation and a bundle without the feature. Services/SQLite files are
fakes in this smoke. Real systemd scheduling, measured backup cost, runtime
failure presentation, corruption quarantine and real SQLite restore drills are
not established by these tests. No release or deployment is implied.
