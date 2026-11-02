# 0137 — A question about one task reads one task

- Status: Accepted
- Date: 2026-11-01
- Decided by: the operator, on task 01a199b6 ("Fix it"), after ADR 0136 left
  swarm-api still holding a core for seconds at a time under real use.
- Amends: ADR 0136, which made the fleet reads cheap and did not reach these.
  Does not change who may see or act on what.

## Context

After ADR 0136, swarm-api on a Keeper with about 2,000 tasks and 1,500 decisions
still ran at 1,000 ms of CPU per second for 4 to 6 seconds at a stretch. A
member-list request that normally takes 2 ms took up to 4.6 s. There were two
causes, and they multiplied.

1. **The decision membership view rebuilt itself for every row that asked.**
   `task_decision_membership` was a `UNION` of a task's own decisions and its
   linked ones. SQLite cannot push a correlated `task_id = t.id` into a
   de-duplicating compound view. So every row that asked rebuilt and sorted the
   whole set, at about 0.7 ms each time. The task projection asks three times
   per task. Projecting every task cost 4.4 s of CPU, and
   `GET /api/v1/tasks/settled` cost 3.65 s.
2. **Every task-scoped agent call read the whole board to check permission.**
   Each of these asked `list_visible_tasks` or `list_tasks_readable_by` and
   looked for one id in the answer:
   - transitions;
   - notes, evidence and messages, through `visible_task_id`;
   - the reload guard;
   - completion exemptions;
   - the read gate.

   Each check projected every task on the Hive while holding the store's only
   connection. `swarm_read_task_history` runs the read gate four times, which
   is about 15 s.

## Decision

1. **The membership view is the same set, written without `UNION`** (schema
   218). It is a `UNION ALL` of the decision's own task and its links, with an
   anti-join dropping a link that restates the decision's own task.
   - Neither half can repeat itself: a decision's id is unique, and a link's key
     is `(task_id, decision_id)`. So the result is exactly the old set, and
     callers keep treating it as one.
   - SQLite flattens `UNION ALL` into the query, so a question about one task
     searches that task's rows by index.
2. **A permission check reads only the task it asks about.**
   - `visible_task` and `task_readable_by` answer for one id.
   - The act rule is stated once, in `acts_on`, and both the list and the
     single-task check apply it.
   - A worker's own list reads only its unfinished assignment, by SQL, and then
     applies the same rule. Queen's list and a board reader's list are still
     the whole board, because that is what they ask for.

## Consequences

Measured on a copy of the Keeper's data:
- Projecting all 1,955 tasks takes 98 ms instead of 4.4 s. Its output is the
  same for every task.
- The three membership columns cost 5 ms between them instead of 4.2 s.
- A gate check is a single-row read.

The roughly twenty other queries that use the view stop rebuilding it per row
as well. These include the coordinator, dispatch, waits and Queen's review.

Tests:
- A test holds the view to the old `UNION` as an oracle, including a restated
  link.
- The same test asserts the query plan searches rather than rebuilds. The old
  view was correct, so only the plan shows the defect.
- A test checks every gate against the whole-board answer, across six kinds of
  principal and ten kinds of task.

## Rejected

- **Rewriting each of the twenty callers instead of the view.** One definition
  is easier to keep right than twenty, and a new caller would have picked up
  the slow form by default.
- **Caching the board between changes.** The gates must see a revoked
  assignment immediately, and a one-row read needs no cache.
- **Pushing the act rule into SQL alone.** The rule stays in Rust, in one place.
  SQL only skips rows the rule could never accept, so narrowing the read cannot
  widen what a worker may do.

## Not proven

That nothing else holds the store for seconds. 4 of 42 ticks under test load
stalled with low CPU before this change, and none of the above explains a stall
without CPU.
