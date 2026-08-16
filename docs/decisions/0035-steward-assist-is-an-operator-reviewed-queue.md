# 0035: Steward Assist is an operator-reviewed queue

## Status

Accepted for dogfooding.

## Decision

Steward Assist is a distinct, Keeper-canonical request/response channel. It is
not represented as a task and is never delivered by typing into a Queen or
worker terminal.

A Steward with an explicit **Assist** grant may offer short, structured help to
one managed Hive. Her local Hive journals the command before network I/O.
Keeper authenticates the polling Member credential, rechecks the current
Stewardship and target membership, and stores a retry-stable receipt. The
target Hive learns about the request only by polling Keeper outward. Its
operator can accept or decline; that response is also journaled locally and
reconciled outward. The requesting Steward sees the final status on a later
poll.

## Safety boundaries

- Delivery and acceptance do not start a worker, open a terminal, inject text,
  interrupt an operator, replace an engagement lease, or imply takeover.
- Keeper receives public Hive and operator identity plus the bounded assistance
  text and disposition. It receives no workers, repositories, local tasks,
  terminals, transcripts, provider sessions, credentials, or Jira issue body.
- Exact command retries return the original receipt. Revoked or out-of-scope
  authority produces a durable rejection.
- Each Hive can continue locally while Keeper is temporarily unavailable; both
  requests and responses remain in bounded outboxes.

## Consequences

Assist can be dogfooded before takeover because it creates coordination without
breaking context. Accepting help is intentionally only consent to coordinate;
any later task routing or takeover remains a separate, explicitly authorized
operation.
