# ADR 0043: Loaded worker before Active work

Status: **Accepted**

## Context

Queen may assign a Ready task to a sleeping durable worker. That assignment
queues the deterministic wake from ADR 0038, but the MCP lifecycle tool could
previously move the task to Active in the same model turn before the
coordinator started the worker. The wake then ceased to be eligible because it
was revision-bound to Ready work. A newly configured worker could therefore
own Active work without any provider process to execute it.

Checking the worker before the transition in API code alone is insufficient: a
process can exit between the check and the state write. Applying the invariant
to every task transition is also incorrect because Jira reconciliation may
mirror remotely canonical In Progress work before Swarm has selected a local
worker.

## Decision

The local Queen agent boundary requires a current live assignment for every
transition to Active, including Ready to Active and Blocked to Active.

The persistence transition validates the exact assignment and live worker
session in the same transaction that changes lifecycle state. If the worker is
sleeping, exits concurrently, or is rebound to another process, the operation
returns `WorkerNotRunning` and leaves the task unchanged. Queen must allow the
guarded wake to finish, observe the live session, and retry.

Worker-originated transitions retain their existing exact-session guard.
Operator and Jira reconciliation paths remain separate: this rule narrows the
Queen MCP authority and does not reinterpret externally canonical Jira state.

## Consequences

- Queen cannot strand locally coordinated work as Active on an unloaded
  worker.
- Resuming Blocked work also requires a loaded owner.
- A concurrent worker exit fails closed rather than relying on later stale-work
  detection.
- Sleeping assignment remains valid and continues to queue one serialized,
  resource-aware wake.
- Jira may still mirror remote status without manufacturing a local worker
  process.

## Proof

- Application tests cover Ready assignment, rejected premature start,
  successful start after binding, and rejected resume after the process exits.
- The Queen MCP regression calls the real assign and transition tools and
  verifies the task remains Ready until the worker has a live session.
- Tool discovery tells Queen explicitly to wake and observe the worker before
  starting or resuming work.
