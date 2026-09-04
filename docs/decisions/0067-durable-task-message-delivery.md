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
Uncertain and rejected message deliveries contribute to the existing conductor's
actionable count and bounded fingerprint (at most 64 delivery identities, never
their text). A changed claim is new evidence even at the same timestamp. Settled
deliveries disappear from that input. Existing enablement, active-run, engagement,
and autonomy gates remain in force; reconciliation requires coordination authority
during an unattended run. The per-run brief names this recovery responsibility.
Committed non-deferred delivery results wake control-feed waiters once per batch;
an unchanged queue or ordinary deferral does not manufacture a refresh event.
At the beginning of an exclusively owned dispatcher pass, remaining dispatching
claims are abandoned by the previous owner and become uncertain transactionally
with a feed event. This also recovers a cancelled pass or failed result save
without restarting the API. The owner lock, not a timeout, proves the earlier
pass is no longer submitting. Recovery failure prevents new submissions.

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

### Terminal observation is not optional

Before any coordination input, the host must return a full snapshot of the exact
requested, running session. Delta-only output, a stopped process, a mismatched
identity or an unexpected response is a pre-write rejection, not evidence of an
empty prompt. Host errors retain their cause. A subsequent valid observation can
establish readiness; rejection never fabricates acknowledgement or starts a
delivery cooldown.

Recovery of an already-rendered unsent message also requires its exact marker
to belong after the latest prompt marker. A matching marker in older transcript
output cannot authorize Enter on the operator's current draft.

### Task-aware delivery holds

Queue selection leaves a worker's cross-task messages queued while that worker
owns another Active task. A question about that same Active task remains eligible,
so continuing current work is not blocked behind an unrelated question. Queen's
inbox is exempt from this worker-task restriction because coordination is her job.

Immediately before contacting each terminal, the dispatcher rechecks every
group member's exact claim, current recipient/session, supersession, operator
engagement and unrelated Active work. Any held member defers the group without
writing, preserving the selected group's order. A failed evidence read rejects
the delivery for Queen reconciliation, not optimistic submission. These normal
holds do not create operator attention or a timer-based escalation.

This durable preflight is not a reservation held across asynchronous terminal
observation and input. The engine still owns live input/engagement protection.
An atomic task-admission fence across that interval, and explicit Scout-only
second-opinion admission, remain open; passing these tests does not prove that
all cross-task races are eliminated.

Verify transactional admission and rollback, bounded exclusive claims, session
and claim fencing, deferral, uncertain writes and restart recovery, explicit
Queen reconciliation, supersession without erased history, readable Queues
exceptions, and failure injection through the actual delivery adapter. Persistence
tests alone do not establish live provider receipt or complete this capability.
