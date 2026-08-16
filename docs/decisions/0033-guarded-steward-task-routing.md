# ADR 0033: Guarded Steward task routing

Status: **Accepted**

## Context

A synchronized Steward grant was intentionally presentation-only until each
remote action had a concrete authorization, retry, conflict, and audit model.
The first useful action is delegating an outcome to a Hive the Keeper has
explicitly placed in the Steward's scope. Routing directly to a remote worker
would violate Hive privacy and couple Apiary coordination to private repository
and provider state.

## Decision

A Member with the **Assign** capability may create one bounded Steward task
command for one explicitly managed Hive. The command contains only Apiary and
target-Hive identity, title, description, priority, command identity, and time.
It contains no worker, repository, terminal, provider session, Jira credential,
or private task data.

The Member validates its synchronized projection and writes the command to a
bounded outbox before network I/O. Delivery is always Member initiated. Keeper
authenticates the node credential, binds it to the Member Hive and operator,
then rechecks the current unrevoked Stewardship, exact target-Hive scope,
**Assign** capability, and active target membership inside the task-creation
transaction. Success creates one Keeper-canonical Apiary task already homed to
the target Hive. Denial creates no task but stores a durable rejected receipt
and audit record. Exact command retries return the original receipt and cannot
duplicate work.

The target Hive receives the task through the existing ordered feed and chooses
its own private worker and repository. A revoked or changed grant therefore
fails closed at Keeper even if a Member has a stale local projection.

## Consequences

- Keeper remains canonical for Swarm-generated Apiary work and Steward authority.
- A Steward can coordinate a managed Hive without seeing or selecting its workers.
- Network loss leaves a visible queued command that safely retries.
- Revocation stops new commands; an exact retry of an already-applied command
  remains idempotently successful.
- Observe, assist, takeover, project, and membership actions remain unavailable
  until separately designed and audited.

## Validation

Persistence tests prove cross-Hive routing, exact-retry idempotence, durable
receipts, revoked-scope rejection, and audit evidence. API tests prove the
credential-bound endpoint and response privacy. Member UI tests prove scoped
Hive selection and an outbound payload that contains no target worker data.
Desktop and Android browser acceptance must prove layout, keyboard use, and
queued/rejected delivery presentation before deployment.
