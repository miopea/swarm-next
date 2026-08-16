# 0041: Deterministic exited-worker owned-work attention

## Status

Accepted for dogfooding.

## Context

The stale-owned-work rule observes only a loaded, resting provider. If a worker
process exits while it owns Active work, that rule correctly stops matching,
but the work can then disappear from Queen's routine review until an operator
notices the sleeping worker. A generic idle nudge or periodic Queen poll would
spend model calls without adding evidence and could race normal worker
recovery.

## Decision

The deterministic coordinator records one attention action when all of these
facts remain true after the existing five-minute worker-recovery window:

- the task is Active and has one durable worker owner;
- that worker's newest process session ended;
- no replacement process session is live;
- no operator engagement lease remains;
- the task revision and ended session still match the observation; and
- the exact task, owner, revision, and process incarnation have not already
  produced an attention action.

The action is evidence only. It does not restart a worker, transition a task,
inject terminal input, or write to Jira. Current evidence enters the bounded
Queen review fingerprint and the Queen-only coordination tool. A replacement
session, task revision, ownership change, or lifecycle transition immediately
makes the observation non-current without deleting its audit record.

## Consequences

Active work cannot quietly disappear merely because its worker process exited.
Normal autostart recovery gets a bounded chance to succeed first, while manual
stops with unfinished work become visible after the same grace period. Queen
still decides whether restarting, reassigning, waiting, or involving the
operator is appropriate; that ambiguity remains outside deterministic
authority.
