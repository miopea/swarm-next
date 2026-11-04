# 0150 — Settlement waits for a message to the worker

- Status: Accepted
- Date: 2026-11-03
- Decided by: Queen, triaging the defect on 2026-11-03: build it with tests,
  and record the handoff question rather than guess it.
- Amends: ADR 0066, its automatic no-deployment settlement guard.

## Context

The coordinator settles Review work that built nothing, or built only
documentation, without asking anyone. It does this in the same orchestration
pass that delivers messages, and before delivery. Closing a task makes its
queued messages to the worker undeliverable, and the next delivery attempt
cancels them unread.

From 2026-10-20 to 2026-11-04 00:00Z:
- The no-deployment sweep closed 117 tasks. 98 of them closed within a minute
  of entering Review.
- 5 of those closes cancelled a message Queen had queued to the task's worker.
- Each was sent before the task reached Review: two while it was Active, two
  while it was Ready, and one while it was Blocked. Each had been tried 5 to 31
  times without delivery. The worker then moved the task to Review, and the
  sweep closed it in the same second.
- The messages were 60 s, 72 s, 6 min, 42 min and 56 min old when they were
  cancelled. One was Queen's go on a pull request.

ADR 0066 already holds this sweep for one thing the worker owes: an unanswered
review request. Nothing held it for something the worker had not yet been told.

## Decision

1. **A message on its way to the task's worker holds automatic settlement.**
   This covers any message to the task's assigned worker, about that task,
   whose delivery is still pending: queued, dispatching, uncertain or
   rejected. That is the same pending set the message queue limit counts.
   Delivered, cancelled and resolved messages do not hold it.
2. **It is checked twice, from one rule.** The sweep does not select such a
   task. The completion transaction checks again, so a message queued after
   selection still holds the task. It refuses with its own error, and the
   sweep skips that task, as it does a review answer.
3. **Nothing new waits unowned.**
   - A queued message is delivered at the worker's next resting prompt, or
     cancelled if the worker is removed.
   - An uncertain or rejected message is in Queen's attention list until she
     reconciles it.
   - Delivery records a Tasks Changed event, and the next pass settles the
     task.
   - No timer, queue or schema change is added.
4. **Only the coordinator waits.** Queen or the operator completing a task on
   purpose is not held, and the message is cancelled as before. Deployment
   closes are unchanged too. ADR 0066 keeps them closing over reviewer holds,
   as an earlier operator decision, and this ADR does not reopen that.

## Consequences

- Work that built nothing stays in Review until Queen's message to its worker
  has arrived. Usually that is the end of the worker's current turn.
- A message to a worker that never comes back to a resting prompt keeps its
  task in Review. Once it has waited too long it shows in Queen's attention
  list, and she can close the task herself.
- The change takes effect when the API reloads.

## Not proven

- **On the Keeper.** Nothing here has run there yet. The prediction: no
  settlement cancels a queued message to its worker.
- **Delivery is not action.** The task can settle once the message lands,
  before the worker has acted on it.
- **Answers sent after the close.** 7 messages from Queen reached 7 tasks
  within an hour after the sweep had closed them, and all 7 were cancelled.
  This hold cannot help those, because nothing was pending at the close.
  Whether a handoff that asks Queen to act should hold settlement until she
  answers is an open question in `docs/09-open-questions.md`.
- **The sweep's skip arm.** No single-threaded test can make the completion
  transaction refuse inside the sweep, because selection and recheck read the
  same rule. ADR 0066's skip arm has the same gap.

## Evidence

- `settlement_waits_for_a_message_to_the_worker_then_settles`: a queued
  message holds the task, and nothing is claimed for it. A write in flight
  still holds it. Delivery releases it to the next pass.
- `an_unconfirmed_message_holds_settlement_until_queen_reconciles_it`: covers
  the crash recovery path. Uncertain holds, a retry holds, and a rejected write
  holds. Resolving it releases the task.
- `only_a_pending_message_to_this_tasks_worker_holds_settlement`: none of
  these hold the task:
  - a cancelled review request;
  - a message about the worker's other task;
  - a message to a worker who does not hold this task.
- `a_message_queued_after_selection_still_holds_the_completion`: the
  completion transaction catches a message queued after selection, then
  settles once it lands.
- `an_explicit_completion_is_not_held_by_a_pending_message`.
- ADR 0066's two settlement tests still pass unchanged.
- **Ablations: 10 changes, 9 each failing a test, and each file restored byte
  for byte.**
  - Two take out one of the checks: selection, and the completion recheck.
  - Four change the pending set: without dispatching, without uncertain,
    without rejected, and with cancelled added.
  - Two widen the rule: a message to any worker, and a message about any
    task.
  - One holds explicit completions too.
  - The tenth, removing the sweep's skip arm, fails no test, as described
    above.
