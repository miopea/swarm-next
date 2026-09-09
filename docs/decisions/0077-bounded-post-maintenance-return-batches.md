# ADR 0077: Bounded post-maintenance worker return batches

Status: Accepted implementation design under the approved operational continuity scope.

## Context

The September 5 live engine replacement returned eleven workers roughly thirty
seconds apart despite normal capacity. The existing supervisor attempts one owed
return per thirty-second pass. This scheduling gap is separate from provider startup.

## Decision

The API supervisor owns a sequential batch of at most four actual return attempts
per pass, drawn from the existing persistence-owned queue (bounded to 256 entries).
Each attempt retains the lifecycle-locked cancellation, provider admission,
fresh resource admission and engine drain checks. Starts are not concurrent.
Deferred/cancelled candidates do not consume attempt slots or prevent eligible
workers later in the bounded queue from being considered. An attempted failure
consumes one slot but does not abandon the remaining batch.

The per-worker outcome owns promise settlement and failure reporting.
The batch changes neither crash-recovery retry policy nor its one-attempt-per-pass
limit, and it does not wake workers absent from the return queue. No new timer,
unbounded task, detached launch or elapsed-time claim of recovery is introduced.
The batch limit bounds scheduling work; it is not evidence of available capacity.

## Durable outcome checkpoint (schema 158)

Before a provider launch, the lifecycle owner claims the return with a unique
attempt ID. At most one attempt exists per promise, within the existing 256-worker
bound. A claimed promise is no longer an automatic queue candidate. A confirmed
engine refusal retains failed attention; an ambiguous response retains unconfirmed
attention. Neither clears the source record or silently returns to the queue.
After API replacement or cancellation, exclusive lifecycle ownership converts
abandoned started claims to unconfirmed without an elapsed-time threshold.

Normal supervisor recovery, Queen startup and task-dispatch startup respect the
durable hold. Explicit operator recovery remains separate. A confirmed new session
binding settles its return in the same transaction; stale attempt replies cannot
settle a newer promise. Explicit stand-down/archive cancels the promise. A new
explicit maintenance operation creates a fresh attempt opportunity. Repeating the
strict source-preparation request alone does not erase a failed outcome.

Needs You presents a concise failed/unconfirmed return notice and a diagnostic
review action, not an automatic or disguised wake. Settlement and attention
changes emit transactional content-free events. Raw provider output is not stored
in these records. This does not prove intended provider-conversation resumption
or finish engine-owned maintenance admission (ADR 0085).

## Verification

Prove a failed worker does not prevent later owed attempts in the same pass,
no more than four attempts occur, and the remaining promise survives for the next
pass. Retain existing capacity/cancellation/drain and lifecycle-deadlock tests.
Live return timing and correct provider-conversation restoration remain separate
acceptance requirements; a returned process alone proves neither.
