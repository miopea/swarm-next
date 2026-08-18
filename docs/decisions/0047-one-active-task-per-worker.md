# ADR 0047: One active task per worker

## Context

A repository worker may have an unlimited ordered queue, but only one item can
be her current work. Allowing two assigned tasks to enter `Active` makes worker
state ambiguous, weakens Queen coordination, and turns the task board into a
poor indicator of what the provider is actually doing.

## Decision

The task persistence transaction refuses a transition to `Active` when the
task's assigned worker already owns another non-removed `Active` task. The
check and lifecycle write share one SQLite transaction, so concurrent Queen,
worker, Jira-reconciliation, and operator paths cannot race around it.

Additional assigned work stays `Ready`; the queue remains unbounded within the
existing Hive task limits. An unassigned task has no worker concurrency claim,
and Jira work remains externally canonical until it is assigned to a Swarm
worker.

## Consequences

- The roster and task board have one truthful current-work item per worker.
- Queen can advance the next Ready item after the current item leaves Active.
- Assignment does not imply execution and does not wake or start extra work by
  itself.
- Existing databases containing conflicting Active rows remain readable. The
  rule prevents new conflicts without silently rewriting operator history.
