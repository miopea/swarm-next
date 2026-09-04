# ADR 0071: Pace ordinary control-room invalidations

Status: Accepted for dogfooding in the operator-approved maturity program,
2026-09-04.

## Context

The durable control-room feed is content-free, but handling an event is not
free. A worker or task event can rebuild the complete workers, sessions, tasks,
decisions, workspaces and Hive snapshot. Provider turns can emit several worker
and task transitions close together, making the browser and API repeatedly
rebuild state that is already superseded by the time it renders.

Operator decisions, runtime pressure/failure, session input availability,
presence and notification changes can alter an available action or safety state.
Those updates must not wait behind cosmetic roster pacing.

## Decision

The one browser-owned `ControlRoomLiveFeed` waits 250 milliseconds before
invalidating a page containing only worker and task changes. The authoritative
snapshot read after that settling window includes state changes committed during
the window. Pages containing a reset or any decision, runtime, session, presence,
notification or unfamiliar event invalidate immediately.

The feed owns the settling delay. Stop, page hide, authentication replacement or
feed restart aborts it. The cursor advances only after the resulting invalidation
succeeds; a failed read retries from the previous durable cursor. The existing
bounded retry backoff and poll deadline remain unchanged.

This is pacing, not loss or mutation of event history. Events remain ordered and
bounded on the server, and the browser's bounded recent-event merge still sees
the complete page. The 250 millisecond value is the approved initial policy and
must be assessed with Dogfood evidence rather than treated as a universal budget.

## Consequences

- Bursts of ordinary lifecycle changes drive at most one full snapshot rebuild
  per settling interval in this feed path.
- Action and safety changes keep their immediate behavior.
- No new poll, subscription, hidden-page work or unbounded queue is introduced.
- Live CPU and interaction improvement remains an operator-soak measurement, not
  something this implementation or its tests can claim by itself.

## Verification

Tests hold the settling owner open and prove ordinary invalidation cannot run
early, while every action/safety event kind remains immediate. Existing tests
retain cursor retry, cancellation, restart and hung-poll guarantees. Browser and
server Dogfood evidence remains the acceptance gate for measured improvement.
