# ADR 0039: Deterministic stale-owned-work attention

## Status

Accepted for dogfooding.

## Context

An operator needs to know when a worker still owns active work but has stopped
making observable progress. Asking Queen to rediscover this condition through
periodic model turns wastes calls, while injecting a prompt directly into the
worker can interrupt real work or duplicate an in-flight command.

Terminal silence alone is not proof of a stall. A loaded worker may be
thinking, waiting for the operator, or actively receiving input. Any signal
must therefore combine durable task ownership with provider-derived activity
and the operator-engagement lease.

## Decision

Every 30 seconds, the deterministic coordinator examines a bounded set of
durably owned **Active** tasks. It records attention only when all of these
conditions hold:

- the owning worker still has a loaded provider session;
- provider activity is exactly **Resting**;
- no operator engagement lease is active for that session;
- the task revision has not changed for at least 30 minutes; and
- the same task, worker, session, and revision has not already been recorded.

The record is revision-bound and idempotent. It contains task and worker
identity plus observed age, but it does not inject terminal input, change task
state, call Jira, or perform another external effect. A task revision, owner,
session, or state change makes the attention record no longer current.

Current records enter Queen's bounded actionable fingerprint. Queen receives a
read-only `swarm_list_coordination_attention` tool and must recheck current
tasks and workers before deciding whether to wait, steer a worker, or ask the
operator. Existing presence and external-effect limits remain authoritative.

## Consequences

- The control room can surface stalled-looking owned work without polling with
  an LLM or interrupting a provider turn.
- **Active**, **Awaiting you**, **With you**, unknown, and sleeping workers are
  excluded rather than guessed about.
- Queen spends a model turn only after the deterministic evidence changes.
- Thirty minutes is a dogfood threshold, not a universal definition of a
  stalled task; later tuning requires measured operator evidence.
- Automatic retry, reassignment, task mutation, and Jira transitions remain
  separate policies.
