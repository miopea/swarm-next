# ADR 0015: Guarded durable decision outcome delivery

Status: **Accepted**

## Context

ADR-0014 records operator judgment without interrupting a worker. A resolved
request still needs to reach the authenticated worker that asked for it. Direct
PTY writes would bypass ADR-0012 whenever the operator is actively steering,
and an API crash between writing bytes and recording success could duplicate a
context-changing message after restart.

The independent terminal host must remain protocol-compatible so ordinary API
and web releases continue to preserve running worker sessions.

## Decision

Resolving a decision atomically creates one bounded delivery-outbox row for the
requesting worker.

- Delivery targets that worker's current active session; worker identity remains
durable across provider-process replacement.
- A live operator engagement lease leaves the outcome queued. Viewing or
resizing a terminal does not block it.
- The API claims at most 16 queued outcomes per pass and serializes claims inside
one API instance. A claim records its target session and attempt before any PTY
write.
- The terminal payload is bounded to the decision identity, selected action,
operator note, and an MCP retrieval hint. Full request context stays in the
durable inbox.
- A terminal-host acknowledgement marks the outcome Delivered. A definitive
host rejection can retry up to three times. An unexpected response or transport
failure is crash-ambiguous and becomes Uncertain immediately.
- On API startup, every interrupted Dispatching row becomes Uncertain and is
never retried automatically. The worker can retrieve the durable resolution
through `swarm_list_decisions`, and the operator can see the uncertainty.
- Delivery uses the existing protocol-7 `Write` request. No terminal-host
protocol migration or sidecar restart is required.
- The 30-second supervisor is a liveness optimization for newly clear leases or
new sessions. Correctness rests on the durable queue, engagement predicate, and
MCP-readable resolution, not the timer.

## Consequences

Resolved outcomes reach workers automatically without breaking active operator
focus. API replacement preserves both the queue and the running sidecar.
At-most-once safety wins over silent duplicate injection when an acknowledgement
is ambiguous; uncertainty is explicit rather than guessed away.

The first slice does not offer manual retry for Uncertain outcomes. That action
needs a separate authenticated command with clear duplicate-risk language.

## Validation

- Persistence tests prove atomic queue creation, engagement deferral, active
session targeting, acknowledged completion, and crash recovery to Uncertain.
- API integration tests resolve through HTTP and observe the bounded message in
a real host-owned PTY.
- Browser tests render Queued, Dispatching, Delivered, and Uncertain states.
- Rolling deployment validation confirms protocol 7 and the existing terminal
host PID and Queen session remain unchanged.