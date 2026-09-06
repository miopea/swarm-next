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
- the conductor waits for the injected marker to stop redrawing, then requires
  provider output or a fresh resting prompt to prove that Enter was consumed;
- a crash before confirmed delivery becomes **uncertain** and is never silently
  replayed;
- an API restart preserves a confirmed Running review bound to its exact,
  still-open worker-session record. The terminal host owns that execution;
  replacing the API does not undo delivery evidence. A missing or ended session
  still enters recovery, and unconfirmed Delivering always becomes Uncertain.
  This does not declare the review completed, extend its deadline, or permit a
  duplicate run. Existing session reconciliation and completion rules remain;
- a running marker expires to **uncertain** after one hour rather than assuming
  completion;
- repeated observations of the same actionable fingerprint do not create
  duplicate runs; and
- disabling automatic review prevents new event-triggered runs without erasing
  the audit state of the latest run.

## Consequences

The finish command returns the outcome committed by its transaction. In
particular, the existing normalization of `needs_operator` to `no_action` when
Queen has no pending decision must also appear in the MCP response. Echoing the
requested value would give Queen and the operator contradictory accounts of the
same run. This does not change normalization policy or require a later read that
could observe a different run.

### API-owned background shutdown (September 6 maturity pass)

The API owns and joins its six periodic service tasks. On graceful termination,
it closes periodic admission before HTTP shutdown and permits an admitted pass
to finish, rather than dropping its runtime between a terminal paste and Enter.
After HTTP serving ends, remaining background passes share one 45-second join
budget. At expiry they are explicitly aborted and joined; existing durable
uncertain-delivery recovery remains authoritative. A blocked pass cannot prevent
shutdown indefinitely. The worker engine and provider sessions are not stopped.

Periods and initial-run behavior remain unchanged. Missed ticks are skipped,
not replayed as a burst after a slow pass. The owner does not interrupt an
individual pass on the stop signal, authorize a replay, identify an arbitrary
paste as automation, or promise graceful completion after SIGKILL. This closes
detached background-task lifetime, not every network or process shutdown gate.

### Review delivery fairness (September 5 maturity pass)

A queued review that has waited through an acknowledged coordination delivery
to Queen receives first consideration on the next delivery pass. The persisted
run request timestamp and live session's last acknowledged delivery establish
this ordering; a new session does not inherit an ended session's evidence.
Before such evidence exists, notification delivery retains its existing order.
The review still uses the shared cooldown, prompt, engagement, provider and
takeover guards. This is not a cooldown exemption or authorization to unblock
tasks. Other outboxes retain their rows if the review consumes this opportunity.
The sole delivery owner attempts the review at most once per pass. This prevents
continuous notifications from renewing pacing ahead of a queued review forever
without permitting the two submissions per pause previously reported as flooding.
No new timer, retry queue, schema or terminal-host protocol is introduced.

Queen can perform useful unattended coordination without becoming a second
permission authority. Operators receive visible queued, running, completed,
waiting, and uncertain states plus a manual review control. The conductor does
not yet schedule work by time, wake a sleeping Queen, or define the durable
external-effect grants described above. Each grant requires its own bounded
product decision, explicit scope, expiry/revocation behavior, and audit trail
before Queen may consume it.
