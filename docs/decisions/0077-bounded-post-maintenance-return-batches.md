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

The existing per-worker outcome owns promise settlement and failure reporting.
This changes neither crash-recovery retry policy nor its one-attempt-per-pass
limit, and it does not wake workers absent from the return queue. No new timer,
unbounded task, detached launch or elapsed-time claim of recovery is introduced.
The batch limit bounds scheduling work; it is not evidence of available capacity.

## Verification

Prove a failed worker does not prevent later owed attempts in the same pass,
no more than four attempts occur, and the remaining promise survives for the next
pass. Retain existing capacity/cancellation/drain and lifecycle-deadlock tests.
Live return timing and correct provider-conversation restoration remain separate
acceptance requirements; a returned process alone proves neither.
