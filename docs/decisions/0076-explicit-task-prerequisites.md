# ADR 0076: Explicit task prerequisites, separate from queue order

Status: Accepted under QUEUE-01 and QUEEN-02 of the daily-driver maturity plan.

## Decision

A local task may name another local task as a prerequisite. This is an explicit,
durable directed edge with a bounded reason and author, not an inferred relation
from descriptions, dispatch order, repository names or elapsed time. Queen and
the operator manage edges; ordinary workers request cross-worker coordination
through Queen and cannot change a peer's dependencies.

The initial command attaches prerequisites to work already in Blocked. It does
not rewind Active or Review work, stop a terminal, start another worker or create
an operator decision. Existing lifecycle commands remain the owner of transitions.
Adding an edge requires both tasks to exist in the same Hive and rejects self
links, cycles and limit exhaustion atomically. Each task has at most 32 edges;
cycle traversal has a fixed node budget and fails closed if exceeded. A repeated
identical edge is idempotent; conflicting changes require explicit removal.

Only a non-removed Completed prerequisite satisfies an edge. Abandonment, removal,
missing ownership, Ready, Review and Awaiting Release are not completion. Removing
an edge requires a reason and retains an audit event. Satisfying every edge does
not silently resume the blocked task: Queen owns the next move and must recheck
any other recorded reason before applying an ordinary guarded transition.

Local transitions from Blocked to Ready or Active refuse unresolved prerequisites.
Jira's externally canonical state remains distinct, but local automatic briefing
and startup cannot bypass an unresolved local prerequisite. Existing active
providers are never interrupted if an upstream task is reopened. Such drift is
visible coordination work, not permission to discard the current conversation.

The shared task projection supplies bounded prerequisite facts to Queues and
agent readers: identity, title, state, assignee, recorded reason and satisfaction.
Unavailable/removed prerequisites remain explicit, not silently absent. Queues
links to the prerequisite's existing task detail; no duplicate task board or
parallel workflow engine is introduced. Current task updates invalidate the
existing projection; no per-edge polling or background loop is added.

## Delivery and acceptance

Implement through domain rules, the persistence transaction boundary and shared
application commands before exposing HTTP/MCP and UI adapters. Preserve backup,
migration and rolling-client compatibility. This ADR records the agreed behavior;
it does not claim that the implementation or live acceptance is complete.

Required tests cover self/cyclic/missing/cross-Hive edges, bounded traversal and
per-task capacity, idempotency, authorization and concurrent mutation, rollback
and reopen, removal/abandonment, completion and reopening, transition/dispatch
guards, truthful task/owner projection, and browser navigation to the prerequisite.
Finish with isolated demo Queen cross-worker coordination. Never synthesize
dependencies in the operator's existing backlog from prose.
