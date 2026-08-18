# 0037: Bounded Queen conductor

## Status

Accepted for dogfooding. Automatic review is opt-in and disabled by default.

## Context

Queen is the operator's primary coordination surface, but a terminal that only
runs when prompted cannot manage routine work while the operator is away. A
general background agent would be unsafe: it could interrupt operator work,
repeat uncertain actions after a crash, or treat broad model confidence as
authorization for Jira, email, deployment, or other external effects.

## Decision

Swarm owns one durable, event-driven Queen automation marker per Hive. Durable
task changes produce a bounded actionable-work fingerprint. When automatic
review is enabled, Swarm claims one exact run only if Queen is running, no local
operator is engaged with her, and no Steward takeover is active. A manual run
uses the same boundary and may be requested while automatic review is off.

The injected prompt carries an exact run identifier. Queen must close that run
through the scoped MCP tool with one explicit outcome: completed, needs the
operator, or no action. Until the marker is closed, Queen may read state and
coordinate local tasks within the current presence policy. Jira, Apiary, email,
deployment, messages, purchases, and every other external side effect remain
denied without a separate recorded operator approval. That approval may be a
durable, narrowly scoped rule established before the run. Night Watch is the
primary journey for these rules: Queen can apply an existing deployment grant
while the operator sleeps, but cannot create, widen, or infer one. Repository
workers and Scout remain the implementation actors; Queen coordinates them.

Delivery and completion are durable:

- operator engagement or Steward takeover defers delivery;
- a crash before confirmed delivery becomes **uncertain** and is never silently
  replayed;
- a running marker expires to **uncertain** after one hour rather than assuming
  completion;
- repeated observations of the same actionable fingerprint do not create
  duplicate runs; and
- disabling automatic review prevents new event-triggered runs without erasing
  the audit state of the latest run.

## Consequences

Queen can perform useful unattended coordination without becoming a second
permission authority. Operators receive visible queued, running, completed,
waiting, and uncertain states plus a manual review control. The conductor does
not yet schedule work by time, wake a sleeping Queen, or define the durable
external-effect grants described above. Each grant requires its own bounded
product decision, explicit scope, expiry/revocation behavior, and audit trail
before Queen may consume it.
