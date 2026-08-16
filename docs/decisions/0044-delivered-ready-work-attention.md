# ADR 0044: Surface delivered Ready work that never starts

## Status

Accepted for dogfooding.

## Context

A task can be durably assigned, its sleeping worker can be loaded, and the
guarded briefing can reach that exact worker session while the task remains
Ready. If the provider ignores the briefing, loses the instruction in context,
or simply returns to rest without calling the lifecycle tool, neither the wake
rule nor stale Active-work detection sees a problem. The task looks assigned
but unattended work can stop before execution begins.

Polling Queen to rediscover this condition would spend model calls on a fact
already present in durable dispatch and runtime evidence. Automatically
re-sending the briefing would be unsafe because the first delivery was
acknowledged and its effect beyond the terminal boundary is not knowable.

## Decision

The deterministic coordinator observes a Ready task only when all of these are
true:

- it still has the same durable worker owner and exact live session;
- the current assignment's briefing is acknowledged as delivered;
- at least five minutes have elapsed since that delivery;
- the provider is resting rather than active or awaiting the operator; and
- no operator engagement lease owns that worker.

Swarm then records one revision-, worker-, session-, and assignment-bound
attention item. It does not re-send the task, transition it, restart the worker,
or mutate Jira. The attention item enters Queen's bounded actionable
fingerprint and the read-only coordination tool. Queen rechecks current task
and worker state before choosing whether to steer, wait, or ask the operator.

The item stops being current as soon as the task revision, lifecycle, owner,
session, or active assignment changes. Exact observations are idempotent.

## Consequences

- Overnight work cannot silently remain Ready after an acknowledged briefing.
- The common case spends no Queen call; only the durable exception escalates.
- A delivered instruction is never replayed merely because its downstream
  effect is unclear.
- Operator focus continues to suppress automatic coordination.
- Jira remains authoritative for externally synchronized lifecycle changes.

## Verification

- Persistence tests prove the grace period, exact live assignment, delivered
  dispatch, engagement suppression, revision recheck, idempotence, visibility,
  and lifecycle clearing.
- Queen conductor tests prove current attention enters and leaves the bounded
  actionable fingerprint.
- An API integration test supplies resting provider evidence and observes the
  resulting deterministic attention record.
- Settings reports not-started, stale, and exited evidence separately while
  keeping one compact surfaced-work total.
