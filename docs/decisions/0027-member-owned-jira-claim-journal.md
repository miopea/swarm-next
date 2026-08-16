# ADR 0027: Member-owned Jira claim journal

Status: **Accepted**

## Context

ADR 0023 keeps Jira authoritative for issue identity, workflow, and human
assignment. ADR 0025 requires member Hives to initiate every connection to the
Keeper and to contact Jira with their own operator identity. A shared issue
claim therefore crosses three independently failing systems: the member Hive,
Keeper, and Jira.

Assigning Jira before reserving Keeper can let two Hives race. Reserving Keeper
inside an HTTP request without durable member state loses recovery after a
crash. Treating an uncertain response as success can create local ownership
that neither Keeper nor Jira confirms.

## Decision

A member Hive journals one bounded claim intent before any remote side effect.
It advances through these durable phases:

1. `queued`: request an atomic Keeper reservation;
2. `reserved`: verify the exact Jira issue and assign it to the local operator;
3. `jira_assigned`: confirm the reservation with Keeper;
4. `confirmed`: import or reconcile the linked local task;
5. `complete`: no further action is required.

Exact retries resume from the recorded phase. Keeper reservation conflicts,
unexpected Jira assignees, missing issues, authentication failures, and
protocol failures move the intent to `attention`. Temporary network loss keeps
the current phase, increments a bounded retry counter, and applies durable
backoff. Expired unconfirmed reservations return to `queued`. A concurrent
local phase change fails closed.

Keeper stores coordination identity and durable home-Hive ownership, not Jira
issue content or credentials. Jira assignment is performed directly by the
member Hive using its connected operator. The local Jira task link is created
only after both Jira assignment and Keeper confirmation succeed.

The journal is bounded to 100 active intents and reconciles at most 16 in one
pass. Complete and attention records remain inspectable evidence rather than
being silently retried or discarded.

## Consequences

Claiming shared work survives API restarts and temporary Keeper or Jira
outages without duplicate Jira assignment. A member cannot import an Apiary
issue as owned work merely because one HTTP response was lost. Operators may
occasionally need to resolve an explicit attention state when external truth
changed during a claim; Swarm does not guess through that conflict.

## Alternatives considered

- **Assign Jira first:** rejected because Keeper cannot serialize competing
  Hives before the external mutation.
- **Reserve and assign in one request:** rejected because Keeper cannot and
  must not use a member's Jira identity, and request lifetimes are not durable.
- **Keep the workflow only in memory:** rejected because crashes between Jira
  assignment and Keeper confirmation would make ownership uncertain.

## Validation

- Persistence tests cover schema migration, phase transitions, bounded queue
  behavior, and retry-stable intent identity.
- A mocked end-to-end API test joins a real member fixture to a Keeper, reserves
  one promoted issue, assigns it through mock Jira, confirms it, imports it,
  and proves an exact retry performs no second Jira write.
- The same test stops Keeper before a second claim and proves the intent remains
  queued with backoff while Jira receives no write.
- Keeper persistence tests independently prove competing Hives cannot hold the
  same active issue claim.
