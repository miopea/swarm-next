# ADR 0032: Confirmed Jira claim handoff

Status: **Accepted**

## Context

A confirmed Apiary Jira claim has one durable home Hive. Reassigning the Jira
issue alone would make Jira and Swarm disagree; changing the Swarm home first
would let the destination Hive appear responsible before its operator identity
can access and accept the issue. Keeper must coordinate the ownership change
without receiving Jira credentials or issue content, and every Member must
continue making Jira calls with her own identity.

## Decision

Confirmed ownership moves through one Keeper-authoritative handoff record:

1. The current home Hive offers its confirmed claim to one active destination
   Hive. Keeper validates both memberships, the promoted project, and the exact
   current claim revision, then stores one idempotent `offered` handoff.
2. The destination Hive may accept or decline. Acceptance is durable and does
   not change the claim home. Once accepted, the offer cannot expire or be
   cancelled automatically; this prevents a successful Jira assignment from
   being followed by a vanished Keeper reservation.
3. The destination Hive journals the accepted handoff before asking its local
   Jira adapter to assign the issue to its own operator. It never receives the
   source Hive's Jira identity or credentials.
4. After Jira confirms assignment, the destination confirms the handoff at
   Keeper. In one transaction Keeper verifies the same active claim and both
   memberships, changes its home node/Hive/operator, and completes the handoff.
   An exact retry returns the same completed record.
5. Only an unaccepted offer may be cancelled by its source or declined by its
   destination. An accepted handoff that cannot finish requires operator
   attention; it never silently returns to the queue or changes ownership.

The source remains the authoritative Swarm home until step 4. During the short
post-Jira/pre-Keeper interval, the destination's durable journal makes the
pending confirmation visible and retryable. Neither Hive may start another
handoff for the same claim while one is offered or accepted.

Keeper receives only bounded Jira project/issue identities, public Hive
identity, timestamps, state, and an optional bounded operator reason. Jira
content, comments, attachments, credentials, workers, repositories, terminals,
and provider sessions remain inside each Hive.

## Consequences

- Cross-Hive ownership is explicit, audited, and cannot become two homes.
- A lost Keeper response after Jira assignment is safe to retry.
- Temporary Keeper or Jira outages pause the durable local operation instead of
  guessing which system won.
- The destination Queen chooses a private worker only after Keeper confirms the
  destination Hive as home.
- Keeper or Steward-directed reassignment can reuse this state machine later,
  but the first dogfood slice is initiated by the current home Hive.

## Rejected alternatives

- **Change Jira and infer the Swarm home from assignee.** Jira users are not
  durable Hive identities and polling races can overwrite intent.
- **Move the Swarm home before Jira assignment.** The destination could appear
  responsible without access to the issue.
- **Expire accepted handoffs.** Jira may already have accepted assignment,
  creating an invisible split-brain.
- **Let Keeper call Jira for Members.** This centralizes credentials, weakens
  per-operator audit, and creates the bottleneck the Apiary model avoids.
