# ADR 0016: Guarded durable task assignment dispatch

Status: **Accepted**

## Context

A Queen or operator can durably assign work, but a worker cannot act until it
is briefed. Ad hoc terminal injection would recreate the context-breaking
broadcast behavior Swarm Next is replacing. It would also lose work across API
replacement or duplicate a brief after a crash-ambiguous terminal write.

Task state, assignment, and briefing delivery are separate facts. The durable
task remains authoritative even when terminal delivery is delayed or uncertain.

## Decision

Creating an assignment atomically creates one bounded task-dispatch outbox row.

- The assignment targets an active immutable worker session and records the
  stable worker identity owning that session.
- A live operator engagement lease leaves the brief Queued. Merely viewing or
  resizing the terminal does not block delivery.
- Reassigning or stopping a session cancels its still-Queued brief in the same
  transaction that releases the assignment. A claimed brief is never silently
  retargeted.
- The API claims at most 16 briefs per pass, with no more than 256 active queue
  rows and 1,024 retained final rows. It serializes task and decision delivery
  within one API instance.
- The one-line terminal payload contains task identity, title, priority,
  workspace, bounded description, and an MCP retrieval hint. Control characters
  are replaced before the single terminal submission. The PTY transport writes
  the prompt, follows at most 64 actual host-output advances until its bounded
  task marker appears in canonical output, and only then sends Enter. A stalled
  or unverified render becomes Uncertain. Provider line editors therefore
  cannot render a brief without accepting it while the ledger reports
  Delivered.
- A terminal-host acknowledgement marks the brief Delivered. A definitive host
  rejection retries at most three times. Unexpected or transport outcomes are
  Uncertain immediately.
- API startup converts interrupted Dispatching rows to Uncertain and never
  automatically replays them. `swarm_list_tasks` remains the recovery source.
- HTTP assignment and the MCP request wrapper attempt delivery immediately.
  The existing supervisor later provides liveness when an engagement lease
  clears; correctness does not depend on its timer.
- Delivery uses the existing protocol-7 `Write` request, preserving the running
  terminal sidecar during API and web updates.

## Consequences

Queen assignment now wakes the correct quiet worker without introducing a
worker-to-worker message bus. Operators can keep steering a worker without an
automation injection breaking context. Queue pressure and ambiguous delivery
are explicit instead of dropping or duplicating work.

This slice does not make terminal output the task-completion authority. Workers
continue to report Blocked or Review through typed MCP transitions; Queen or the
operator approves completion.

## Validation

- Persistence tests cover engagement deferral, active-session targeting,
  acknowledged completion, queued cancellation on reassignment and stop, and
  crash recovery to Uncertain.
- A v11-to-v12 migration test proves existing databases gain the outbox.
- API integration assigns through HTTP and observes the brief in a real
  host-owned PTY.
- Browser component tests render every delivery state.
- Rolling deployment validation must preserve protocol 7, the terminal-host PID,
  the Queen worker identity, and her active session.
