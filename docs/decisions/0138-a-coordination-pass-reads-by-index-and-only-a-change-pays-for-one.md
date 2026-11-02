# 0138 — A coordination pass reads by index, and only a change pays for one

- Status: Accepted
- Date: 2026-11-01
- Decided by: the operator, who asked for the server follow-ups to ADR 0137 on
  2026-11-01 ("keep going with the server follow-ups").
- Amends: ADR 0016, which has the MCP request wrapper attempt delivery after
  every request. Now it does so after a call that may have changed something.
  Does not change what is delivered, to whom, or when the supervisor runs.

## Context

After ADR 0137, an agent call on the Keeper still cost 210–250 ms of CPU while
holding the store's only connection, even for a tool that does nothing. A POST
to `/mcp` that the handler refused cost 130–140 ms of CPU and about 200 ms
of wall time, while a plain GET cost nothing measurable. The difference is
the coordination pass: `mcp()` ran `deliver_coordination` after every request,
whatever its outcome.

Timed one by one on a copy of the Keeper's database, the pass's store calls
took about 120 ms. Four queries made up 106 ms of that, and each returned no
more than two rows:

| Query | Cost |
| --- | --- |
| `unattended_block_candidates` | 41 ms |
| `stale_owned_work_candidates` | 35 ms |
| `observe_queen_automation` | 19 ms |
| `sweep_attention_notifications` | 10 ms |

There were two causes.

1. **The detectors grouped the whole activity log.** `blocked_transition_source`
   and `last_task_action_source` filter `task_activity` on columns no index
   leads with. So each read all 28,000 rows, with their notes, to group the
   few a pass wants. That costs about 25 ms each time.
2. **Each detector scanned the attentions it had already raised.** Before
   raising an attention, a detector asks whether it already did, by task and
   kind. `coordinator_actions` (about 5,900 rows) had no index on either, so
   the check read the whole table once per candidate.

## Decision

1. **Schema 219 adds three indexes. No query changes.**
   - `task_activity_blocked_transitions` and `task_activity_owner_actions` are
     partial indexes whose `WHERE` matches each macro's filter exactly.
   - `coordinator_actions_by_task` is on `(task_id, kind)`.
   - The SQL text is unchanged, so results cannot differ. Only the plans
     change.
2. **Only a call that may have changed something runs the pass before it is
   answered.**
   - The test is the one that already decides whether the control room is
     told: a tool call that succeeded and is not one of the nine reads.
   - A read, a refused call, and a request that never authenticated no longer
     run a pass.
   - The decision is made in `agent::handle`, which knows the outcome. `mcp()`
     no longer calls the pass itself.

## Consequences

Measured on a copy of the Keeper's data:
- `unattended_block_candidates` takes 1.6 ms instead of 41 ms.
- `stale_owned_work_candidates` takes 4.1 ms instead of 35 ms.
- The pass's store work takes about 45 ms instead of 120 ms.

That saving reaches every pass, including the supervisor's every thirty
seconds and the inline passes after operator actions. Agent reads, the
commonest calls, pay for no pass at all.

A delivery that a read used to set off incidentally now waits for the next
mutating call, operator action or supervisor pass, which comes at most thirty
seconds later. Nothing a read does creates work to deliver. ADR 0016 already
holds that correctness does not depend on the supervisor's timer, and that is
still true. ADR 0017's immediate delivery of a worker's outcome to Queen is
unaffected, because a transition is a mutating call.

Tests:
- A plan test asserts that each macro and the already-raised check read
  through their index. Editing a macro's filter away from its index, or
  rebuilding `coordinator_actions` without recreating the index, fails it.
- An MCP test goes through the router. A message claim that only a pass
  recovers stays untouched after an unauthenticated request, a read and a
  refused call, and is recovered after a call that filed a draft.

## Rejected

- **Rewriting the five queries to read activity per task.** The indexes fix
  all five, and any later use of the two macros, without touching SQL whose
  results callers depend on.
- **Waking the supervisor instead of running the pass inline.** Its pass also
  reconciles terminals, recovers workers and continues Queen's review, which
  is more work than the delivery a call needs. A mutating call also keeps the
  immediate attempt ADR 0016 gives it.
- **Coalescing concurrent inline passes.** With reads no longer paying and the
  pass about a third of its old cost, the queue it would bound is short. It
  can be added if measurement shows otherwise.

## Not proven

- Not measured on the running Keeper yet: that needs a merge and a reload.
- Queen's own detectors are untouched. `observe_queen_automation` (18 ms) and
  `sweep_attention_notifications` (10 ms) are now most of what a pass costs,
  and they read every completed attention row to find the few still live.
- About 90 ms of an agent call is the tool itself and varies by tool. It is
  not covered here.
