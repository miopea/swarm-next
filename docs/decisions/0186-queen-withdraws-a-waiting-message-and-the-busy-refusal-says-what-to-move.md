# 0186 — Queen withdraws a waiting message, and the busy refusal says what to move

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), answering "Revised scopes": when the
  one-Active rule refuses, tell the worker which task to move out of Active,
  and let Queen withdraw stale queued messages. No urgent flag and no
  automatic reply.

## Context

A worker held one task Active for 63 hours and worked another from Ready.
Queen's messages about the task it was really doing were held behind the
Active one, as ADR 0067 intends, so 17 of them waited, the oldest for 25 hours.

- The worker had tried to make the second task Active. The refusal named the
  Active task, as it has since 2026-09-02, and ended "finish or move that one,
  or leave additional assigned work Ready". It took the second option and
  worked the task from Ready, so the board showed the wrong work.
- Queen saw the held messages on her attention list, with their age and the
  task they waited behind. She had no way to take one back: reconciliation
  read only uncertain and rejected deliveries.

## Decision

1. **The refusal says what to move.** It names the Active task and says: if
   you are working on this task now, first move that one out of Active, to
   Review, Blocked or Ready, whichever is true of it, then make this one
   Active, so the board shows what you are doing and messages about this task
   reach you. Leave this one Ready only if you are not working on it yet. The
   rule itself is unchanged.
2. **Queen withdraws her own waiting message.**
   `swarm_reconcile_task_message` with `retry_may_duplicate` false resolves
   her own queued message that no delivery pass has claimed, whatever claim is
   named. Its reason is recorded as `withdrawn by Queen before delivery: ...`,
   it is never typed in, and the settlement it held (ADR 0150) goes ahead.
   - A message already being written is not withdrawn: the call changes
     nothing and says so.
   - The operator's message and Swarm's reminder are not hers to withdraw.
   - A retry of a queued message changes nothing; it is already queued.
3. **The history note says what queued means.** A queued or held message has
   not been typed into the recipient's terminal yet.

## What this does not do

- No urgent flag: the operator ruled it out, and it would have released none
  of the held messages, which were about a normal-priority task.
- No automatic reply from a held worker.
- It does not change which messages are held, or for how long.

## Consequences

- No schema change and no change to any tool's arguments, so the agent tool
  surface does not move. A session that connected earlier can already call
  this; it reads the new wording after it restarts.

## Evidence

- A persistence test: the refusal names the Active task to move first, and no
  longer offers leaving this one Ready while it is worked.
- A persistence test:
  - Queen's waiting message holds settlement until she withdraws it;
  - the withdrawal is recorded with her reason, and it is never claimed;
  - the operator's message is not withdrawn, a retry changes nothing, and a
    message being written is not withdrawn;
  - the task then settles.
- An MCP test: a worker cannot withdraw it, and Queen withdraws it through the
  tool with no claim.

## Not proven

- No Hive has run it.
