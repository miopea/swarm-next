# 0184 — Queen is told when her message cannot reach a closed task

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), answering "Revised scopes": refuse or
  flag Queen's message to a closed task, and do not re-route it.

## Context

A message to a worker is about one task. When that task is completed or
abandoned before the message arrives, Swarm cancels the message unread
(`its task closed before it arrived`). Nothing told the sender.

Measured on one Hive from 2026-11-05 to 2026-11-22, 249 of Queen's messages
were cancelled that way:

- 88 were sent to a task that was already closed. Swarm accepted each one as
  queued.
- 161 were queued on an open task that closed before they arrived: 115 by
  Swarm's own settlement or release sweep, 42 by Queen's completion, 2 by
  Jira, 2 by Queen's abandon.

Some of them were a GO, and one told a worker not to merge until she sent a
new GO.

The ticket asked for the message to be re-routed to a parent task. Tasks have
no parent, so there is nowhere fixed to route it, and guessing a task would
put Queen's words on work she did not name.

## Decision

1. **Queen's send to a closed task is refused.** `swarm_message_worker` on a
   completed or abandoned task records nothing and says the task's state and
   what to do: send it on an open task of that worker, or tell the operator if
   none is open. A removed task is refused as before.
2. **Her message whose task closed first is handed back to her.** When a
   delivery pass finds Queen's queued message on a completed or abandoned task,
   its delivery becomes `rejected` with the reason
   `QUEEN_MESSAGE_TASK_CLOSED_REASON`, instead of `cancelled`.
   - That is the state her wake-up already counts and her attention list
     already lists, so she is woken and sees it, with the reason.
   - Queues shows it under Delivery waits as hers, saying the task closed.
   - She clears it with `swarm_reconcile_task_message`, `retry_may_duplicate`
     false and no `claim_id`, since no pass ever claimed it. If it still
     matters she first sends it again on an open task.
   - A retry is refused while the task is closed: queued there again, it
     would only be handed back again.
3. **Nothing is re-routed.** Swarm never moves her words to another task.
4. **Only Queen's message.** The operator's message and Swarm's own reminder
   are cancelled as before, and a message to a removed worker is cancelled
   whoever sent it. A message to Queen was never cancelled for this.

## What this does not do

- It does not raise the messages cancelled before this release. They keep
  their old reason and stay cancelled.
- It does not hold a task open, or hold its settlement, for a queued message.
- It does not change which task a message is about.

## Consequences

- No schema change. `rejected` was already a delivery state, and a handed-back
  delivery has no claim.
- `swarm_reconcile_task_message` no longer requires `claim_id`, so the agent
  tool surface moves to revision 47. A Queen session that connected earlier
  still holds the schema that requires it, until it restarts.
- A handed-back message counts toward the 4,096 pending deliveries, as every
  delivery waiting for Queen does.

## Evidence

- Persistence tests on a real SQLite store:
  - Queen's send on a completed task is refused and records nothing; on an
    open task it is admitted;
  - her queued message on a completed task and on an abandoned task is handed
    back with the reason and no claim, and the operator's is cancelled;
  - it is listed for her, a retry is refused, a wrong claim does not match,
    and a clear with no claim resolves it for good;
  - it wakes her conductor, whose reading of a delivery with no claim does
    not fail, and clearing it goes quiet.
- An MCP test: her send on the closed task is refused with its state, the
  earlier message is listed with the reason, and she clears it without a claim.
- A Queues test: the row says the task closed and does not tell her to retry.

## Not proven

- No Hive has run it.
