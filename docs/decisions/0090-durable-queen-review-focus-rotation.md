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

Follow-up: the broad initial run completed with 695/697 passing. Its second
failure was the generic previous-schema fixture still choosing the old last
entry; fixture entries now follow their actual migration ceiling, and the
targeted migration test passes. Main's candidate migrates only focus schema 152;
no support-outbox module or migration is included.

The inbox benchmark's universal comparison against the old unbounded query was
invalid: favorable scan order lets SQLite defer its expensive projection already.
Identity-only materialization now reduces work against the deployed full-row
bounded query while retaining explicit bounded evidence evaluation. With
deterministic fixture timestamps, all returned fields/order match both queries.
Unscoped VM steps: unbounded 217370, deployed bounded 245284, identity bounded
225574; worker-scoped: 218906, 246820, 227110. This is about 8% less than the
deployed bounded query, not a claim of universally beating the unbounded query.
All 35 decision tests pass. Exact-main and live verification are still pending.

The application integration test now also proves that an authenticated unchanged
operator deferral is excluded from a newly claimed reservation, a non-Queen
principal cannot reserve focus, and a same-run retry preserves its existing batch
after evidence changes while the full review snapshot exposes that change. It
passes in isolated Linux. The main candidate's complete web suite passes 144
files / 1372 tests, including the separate admission-hold card correction.

The first exact-main Rust run lost SSH mid-suite without a terminal result. The
test process was confirmed absent; the API and engine retained their existing
process IDs. Replacement validation is owned by PID 1662834 and logs to
/tmp/swarm-focus-main.8iuQrn/validation-2.log with a final VALIDATION_EXIT marker.
Build and test concurrency are each capped at two. No deployment is claimed.

Later September 8 acceptance: exact-main validation completed with 667 persistence
tests passing, the API focus test passing and strict API Clippy passing. Main
5dc34137 was pushed and deployed as 1.6.0-dev-5dc34137f952-20260908220617-1667504.
API PID became 1669695; engine PID remained 1547164 and all 15 running sessions
remained. A newer engine build is reported available but was not activated.
Edge confirmed the served runtime and the corrected start-admission card.

Queen run 01a0830a-2d84-7951-8851-97569f84786d delivered once at 1788905457 and
finished with outcome incomplete. Three fresh assessments followed: two external
conditions and insufficient-evidence task 01a05dc6-2041-7281-a366-096742c5b9d5.
The review snapshot still contains 24 items. This proves a delivered review and
fresh persisted assessments, not complete backlog recovery or multi-run fairness.
Subsequent distinct batches and productive investigation remain acceptance gates.
