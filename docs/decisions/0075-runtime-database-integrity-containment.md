# ADR 0075: Contain confirmed runtime database damage

Status: Accepted for the approved REC-02 outcome; implementation in progress.

## Decision

Persistence owns a process-lifetime recovery-required latch shared by all clones
of the Hive store. A failed SQLite integrity result or an integrity probe's
corrupt/not-a-database driver error sets it before releasing the connection.
Subsequent persistence access fails closed, including new dispatch claims.
Ordinary domain IntegrityFailure errors, busy locks, IO failure, and interrupted
checks are not proof of corruption and do not set this latch.

The API owns one hourly integrity probe, skipping its immediate startup tick
because file-backed opening already verifies integrity. It runs off the async
executor, never queues for the persistence mutex, and installs a SQLite progress
deadline only for the probe. One probe may be in flight; missed ticks are skipped.
The progress deadline is one second, not a guarantee against a stalled kernel IO
operation. Incomplete checks are reported as such, not as healthy or corrupt.
Shutdown cancels future probes and joins any admitted one. No new durable tables,
timers per browser, or worker process operations are involved.

An operator-credential-only POST to `/api/v1/runtime/database/integrity` runs the
same bounded probe on demand. Background and explicit checks share one admission
permit, with no wait queue. The blocking job retains that permit even if its
requester disconnects. A busy probe returns 429; a busy persistence lock returns
`verified: false`, not a healthy claim. Successful results are no-store. The
endpoint never contacts or restarts the terminal host.

Health exposes the latched consequence without reading SQLite. The browser routes
this to Needs You and runtime diagnostics without trying to write a decision into
the damaged database. Recovery is the explicit, verified offline restore in ADR
0020, followed by reopening the store. A later successful query cannot clear the
latch. Existing workers and repository files remain outside this containment;
work already admitted before detection cannot be retroactively canceled.

This is containment after confirmed detection, not continuous corruption detection
on every SQL operation. It does not classify every generic persistence error as
physical damage, automatically restore a backup, or promise to stop provider work.

## Verification

Prove shared-clone write/read refusal after a real SQLite integrity failure,
healthy and unavailable checks without false latching, explicit healthy reopen,
health/attention presentation independent of database reads, bounded probe
ownership and shutdown. Never corrupt the live Hive for a test.
