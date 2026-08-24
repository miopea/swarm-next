# ADR 0052: A held Jira write does not freeze the local lifecycle

## Status

Accepted. Ruled by the operator on 2026-08-22 (missing mapping) and 2026-08-23
(write in flight); implemented in `queue_jira_transition`. Recorded here on
2026-08-24 because the reasoning was only in a scoping document and in code
comments, and the task that asked the question said plainly what closes it: the
next person to hit a stalled queue should find the ruling rather than file it
again.

## The question

When a Jira-backed task is moved to a state its binding has no mapping for,
should the local lifecycle advance?

It was found the hard way. Queen was asked to move a Ready task out of the
actionable queue on an operator ruling, and the Hive's own record could not
change because a Jira mapping was absent. One misconfigured Jira project stopped
**thirteen tasks moving inside Swarm** — all of them, because the lookup is
keyed on `(binding_id, task_state)` and nothing about the individual task
participates.

## Decision

**The local move always succeeds. The Jira write is queued and resolved at
delivery.** No external configuration error may stop internal work.

Rejected: **atomic**, keeping the two stores honest with each other by refusing
the local transition. It is a coherent position, and it is wrong here. Swarm's
task board is the Hive's own record of what its workers are doing; Jira is a
system it reports to. Letting the second one hold the first hostage inverts
that, and the failure is not even a Jira outage — it is a status somebody did
not map in a settings screen.

The third option the question priced — advance locally, mark the Jira side
pending, retry or surface it — was costed as "a queue and a reconciliation path
that does not exist today". It did exist. `jira_transition_deliveries` already
queued transitions and delivered them asynchronously with retry. The only thing
wrong was *where the mapping was resolved*: at queue time, inside the same
transaction as the local move. Moving that lookup to delivery time was most of
the work.

## What follows from it

The same principle decided three further cases in the same code path, each one
the same shape arriving a step later:

- **A write already in flight** does not block the local move (2026-08-23). It
  used to refuse while any delivery was pending, so one Jira write stuck
  retrying froze that task.
- **A queued write is superseded, not joined**, so Jira ends at the task's
  newest state rather than walking every intermediate one.
- **A full delivery queue** does not refuse the local move either. The queue
  being full is a Swarm problem, not a statement about this task, and refusing
  would hand an internal backlog the power to freeze internal work.

An unmapped delivery becomes a **retryable conflict** (`state = 'conflict'`,
`last_error = 'workflow_state_not_mapped'`) rather than an error that aborts the
claim — returning an error there took every other queued transition down with
it, the same freeze one layer further down. Fixing the mapping in Settings and
retrying is the whole recovery; the task never has to be re-transitioned by
hand.

## The cost, stated plainly

There is a window where Swarm and Jira disagree, and somebody has to reconcile
it. That is the price, and it is the right one to pay: the disagreement is
visible in the delivery queue with the reason attached, and it stops as soon as
the mapping is fixed. A frozen board is neither visible nor self-correcting —
it presents as work that has simply stopped moving, which is exactly how it was
found.

One narrow gap is accepted rather than closed: if a transition lands while
another is mid-flight, that state is not queued, so Jira is behind by one until
the task moves again. Joining it is not possible — one active write per task is
a unique index, and inserting beside it would fail the constraint and take the
local transition down with it, reintroducing the freeze as a database error.

## Where the behaviour lives

`queue_jira_transition` in `crates/swarm-persistence/src/jira.rs`, with the
delivery-side resolution in `claim_jira_transitions`. Pinned by
`local_jira_transition_is_durable_bounded_and_acknowledged`, which asserts the
local move succeeds with no mapping, the write is queued rather than dropped,
the unmapped delivery does not take the batch with it, and it stays retryable.

Three error variants were removed when this ADR was written:
`JiraStateNotMapped`, `JiraTransitionPending` and `JiraTransitionQueueFull`.
Each existed only to refuse a local transition on the Jira side, each was still
declared and still mapped to an HTTP status, and none had been constructed
anywhere since the rulings above. They are the three refusals this decision
removed. Keeping named errors for a behaviour that was deliberately taken out is
how somebody talks themselves into putting it back.
