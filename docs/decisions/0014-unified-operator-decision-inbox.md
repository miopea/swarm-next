# ADR-0014: Unified operator decision inbox

Status: Accepted

## Context

Workers and Queen often need operator judgment without needing permission to
run a provider tool. Legacy coordination mixed proposals, messages, direct
terminal injection, and provider approvals. That fragmented attention, made
urgent and routine activity difficult to distinguish, and could interrupt an
operator who was actively steering another worker.

ADR-0012 protects active operator engagement, and ADR-0013 establishes
authenticated role-scoped agent identities. The first attention surface must
use those boundaries instead of reintroducing arbitrary messaging.

## Decision

Swarm Next owns one durable, typed operator decision inbox.

- Requests are one of Input, Approval, Credentials, Conflict, or Help, with
  Normal or Time-sensitive urgency.
- Every request records an authenticated requesting worker, optional task,
  reason, risk, evidence, suggested action, one to six explicit allowed
  actions, and an optional deadline.
- A worker may correlate only the task currently assigned to its active
  session. It sees only requests it originated. Queen sees the full Hive inbox.
- Agents may create and list requests through the scoped MCP bridge, but cannot
  resolve them. Resolution is an authenticated operator action.
- Resolution records the selected allowed action, optional note, operator
  identity, and timestamp atomically with a content-free control-room event.
- Pending requests are bounded at 256 and reads at 200. Resolved history stays
  durable but visually quiet until requested.
- A decision is a recorded judgment, not arbitrary agent-to-agent messaging.
  ADR-0015 delivers resolved outcomes to the requesting worker through the
  ADR-0012 engagement boundary.
- Provider-native permission prompts remain provider-owned and are not copied
  into this inbox.

## Consequences

The operator gets one calm, mobile-first “Needs you” queue instead of terminal
interruptions or fleet broadcasts. Durable context survives browser and API
restarts, while role visibility prevents workers from reading peer requests or
claiming unrelated tasks.

The initial inbox does not send push notifications or model long-running
discussions. Guarded terminal delivery is specified by ADR-0015. Those features
must extend the typed record and engagement guard rather than bypass them.

## Validation

- Domain and migration tests cover typed values, schema upgrade, bounds,
  same-Hive references, ordering, atomic resolution, and audit events.
- Application tests prove worker visibility and task-correlation authority.
- MCP tests prove role-scoped discovery, authenticated creation, structured
  responses, and Queen visibility.
- HTTP tests prove operator-only listing and resolution.
- Browser tests cover the pending-first queue, optional note, explicit actions,
  hidden resolved history, task/requester context, and the responsive App
  integration.