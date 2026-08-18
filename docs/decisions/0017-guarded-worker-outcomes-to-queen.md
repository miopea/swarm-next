# ADR 0017: Guarded worker outcomes to Queen

Status: **Accepted**

## Context

Workers need to report blockers and completed implementation back to Queen,
while operators still commonly steer individual workers directly. Legacy-style
injection or fleet broadcasts can break the context currently owned by the
operator and make task truth depend on transient terminal text.

A state-only notification is insufficient for real work: Blocked needs a reason,
and Review needs a concise handoff. The transition, note, and notification must
not diverge across a crash.

## Decision

An assigned worker may transition its own task to Active, Blocked, or Review.
Blocked and Review atomically create a bounded durable outcome for the singleton
Queen in the same transaction as the task state and activity note.

- Handoff notes are optional, stored in task activity, and bounded to 4,000
  bytes. The MCP tool asks workers for a concise blocker reason or review
  handoff.
- Worker authority and active assignment are rechecked inside the transition
  transaction. A stale session cannot change or report a task.
- Operator and Queen transitions are audited but do not notify Queen about her
  own action.
- A newer transition or reassignment cancels an older still-Queued outcome for
  that task. Claimed outcomes are never silently retargeted.
- Delivery targets Queen's current active session. A live Queen operator-
  engagement lease leaves the outcome Queued; merely viewing the terminal does
  not.
- At most 16 outcomes are claimed per pass, with no more than 256 active rows
  and 1,024 retained final rows.
- The terminal payload contains reporting worker, task identity and title,
  target state, bounded note, and an MCP retrieval hint. It is sanitized and
  sent as one terminal submission. The PTY transport writes the prompt, waits
  up to ten seconds for its bounded task marker to remain at the same canonical
  sequence for 300 milliseconds, and only then sends Enter. It observes the
  provider after each of at most three Enter attempts. Active output, an
  operator question, or a new resting prompt after the marker proves
  acceptance; a stalled or unverified submission becomes Uncertain.
- A terminal-host write acknowledgement alone does not mark Delivered. A
  definitive rejection retries at most three times. Unexpected or transport
  outcomes become Uncertain immediately.
- API startup converts interrupted Dispatching rows to Uncertain and never
  automatically replays them. The task and activity history remain canonical.
- The existing MCP wrapper attempts delivery immediately after the worker tool
  call. The supervisor is only a later liveness pass when Queen becomes quiet.
- Delivery uses protocol-7 `Write`; the terminal host and worker PTYs remain
  independent of API/web replacement.

## Consequences

Queen receives actionable Blocked and Review handoffs without worker-to-worker
messaging, fleet broadcasts, or interruptions while the operator is focused in
her terminal. The operator can still talk directly to workers; durable task
state and history, not terminal delivery, define the work.

This slice does not let workers approve Completed. Queen or the operator still
owns completion, including required review and shipping.

## Validation

- Persistence tests cover assignment-atomic authority, note history, Queen
  engagement deferral, stale outcome cancellation, acknowledged delivery, and
  crash recovery to Uncertain.
- A v12-to-v13 migration test proves existing activity and databases survive.
- API integration observes a Review handoff in a real host-owned Queen PTY.
- Browser component tests render Queen delivery state and durable handoff notes.
- Rolling deployment must preserve protocol 7, terminal-host PID, Queen worker
  identity, and Queen session.
