# ADR 0067: Task messages have durable delivery ownership

Status: Accepted implementation decision under QUEEN-01 and QUEUE-01.

## Decision

Keep immutable task-message history separate from its delivery lifecycle. A
message and its outbox row are committed together. At most 4,096 unresolved
deliveries may be admitted; overflow refuses the new transaction without losing
older work. Each pass claims at most 16 messages, recording a unique claim and
the exact live recipient session before any terminal submission.
Only one batch per terminal may be in flight. Selection gives each recipient a
turn before filling additional slots, retains that recipient's message order,
and rotates equally positioned recipients by prior claim count rather than time.

Queued, dispatching, delivered, uncertain, rejected, cancelled, and resolved are
distinct states. A claim completion must match both its identity and session.
Only a definitive pre-write deferral returns it to queued. Transport ambiguity
and interrupted claims become uncertain, never automatic retries. Rejection is
visible for Queen's inspection rather than an endless retry loop. Terminal
acknowledgement does not prove provider comprehension or task completion.

Queen owns reconciliation. She may explicitly resolve an uncertain or rejected
delivery after retrieving its durable content, or authorize one retry with a
recorded reason and acknowledgement of duplicate risk. Resolution is not a
fabricated terminal acknowledgement. The command is fenced by the observed
claim ID. No elapsed-time threshold promotes this to Needs You; Queen escalates
only when she cannot move the work within existing authority.

Superseding a linked review request cancels only its queued delivery, in the
same transaction as the ownership change. Ordinary task questions are not
assignment-bound review requests. A claimed request may already be in flight;
preserve that uncertainty rather than claiming cancellation recalled bytes.
An old queued request never becomes eligible again when an assignment returns.
An explicitly correlated answer retrieved from history also cancels the queued
question in its transaction; answering before terminal delivery must not cause
the same question to arrive later.

Schema 134 backfills delivered history as delivered and remaining old messages
as queued: the old implementation did not record ambiguous attempts, so their
historical delivery cannot be reconstructed. API startup, not arbitrary age,
recovers interrupted claims. Existing terminal protocol and independent worker
lifetimes remain unchanged.

## Acceptance

Verify transactional admission and rollback, bounded exclusive claims, session
and claim fencing, deferral, uncertain writes and restart recovery, explicit
Queen reconciliation, supersession without erased history, readable Queues
exceptions, and failure injection through the actual delivery adapter. Persistence
tests alone do not establish live provider receipt or complete this capability.
