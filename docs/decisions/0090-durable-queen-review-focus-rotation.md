# ADR 0090: Durable Queen review-focus rotation

Status: Accepted implementation direction within the approved orchestration
maturity scope; implementation and live acceptance remain incomplete.

## Evidence and outcome

September 8 live reviews repeatedly refreshed external waits while nine older
insufficient-evidence investigations remained unchanged. Current ordering puts
all fresh checks before unchanged investigations, and each delivery takes only
the first three. Six recurring external checks can therefore monopolize focus.
Neither more reminders nor fabricated assessment timestamps establishes fairness.

## Decision

Rotate the three-item prompt focus through the bounded uncovered candidate list.
Stable task identity establishes the rotation sequence, independent of changing
last-check timestamps and judgment categories. Urgent new work and
the full backlog remain Queen's responsibility alongside that focus. Covered
operator deferrals remain excluded. Rotation does not certify any assessment,
change task priority/ownership, clear a blocker, or grant execution authority.

Persist a singleton cursor and at most three reserved focus identities for the
current run through the persistence boundary. Reserve once for a claimed review;
retries and same-run continuations reuse that reservation without advancing it.
Never let read-only queue inspection move the cursor. A stale run cannot reserve
or overwrite a newer run's focus. Current state must still be read before acting;
a reserved item that has settled needs no repeated work.

For an unchanged finite candidate order, successive reservations visit every
candidate before starting another cycle. A removed cursor safely restarts at the
current beginning; changing membership does not imply that an old task was read.
Overflow is explicit and cannot certify full-board traversal. No clock, random
selection, timer, unbounded history or task-assessment rewrite is introduced.

The focus migration must ship independently of the inactive support outbox;
support activation remains later in migration order. Existing worker processes
do not need replacement for this application/persistence change.

## Required verification

Test recurring external checks plus unchanged investigations, wraparound, removed
and empty candidates, duplicate/overflow refusal; then durable reopen, same-run
replay, stale-run refusal, transactional rollback and current covered-task fences.
Observe distinct focus batches and resulting investigation actions on the live
Hive. Rotation alone does not prove Queen obtained missing facts or cleared work.

## September 8 implementation checkpoint

The domain selector now uses stable identity order rather than mutable review
timestamps. Four focused tests cover full traversal, changing input order,
wraparound/removal/empty sets, and duplicate/overflow refusal. The isolated Linux
domain suite passed all 153 tests. This is selector evidence only: durable
reservation, application delivery wiring, migration and live acceptance remain
unfinished. No production behavior has changed from this checkpoint.

Separately, replacement main CI run 34278624037 completed successfully after the
browser test bootstrap synchronization correction; no release was created.

The persistence reservation and application/API delivery path are now implemented.
Reservations require the current delivering run and its live Queen session;
same-run retries reuse the saved identities without writes. Four persistence
tests passed for reopen/replay, stale run/session refusal, failed-write rollback,
new-run advance, corrupt payload refusal and migration from schema 150. The
updated API focus/identity/replay test and strict API all-target/all-feature
Clippy passed in the isolated Linux workspace. Focus schema 152 is separate from
the still-inactive support schema, moved to 153 on the development branch.

Deployment remains gated: the broader persistence run reported a reproducible
existing inbox VM-work assertion failure (229418 baseline steps, 245332 bounded
query steps), despite equal projected results. The decisions source matches the
current branch and main; determine the baseline/environment cause before
promoting. Exact-main migration isolation, the full regression result, covered
task integration fences and live backlog acceptance remain outstanding.
