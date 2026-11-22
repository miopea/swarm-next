# 0182 — Queen or the operator lets a worker read another task

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), answering "Revised scopes": a read
  link that only Queen or the operator sets, the way prerequisites work,
  instead of letting a worker read any task its own task names.

## Context

A worker reads only its own work: the tasks it holds or finished, and a task
the board names as its own task's prerequisite. Work often depends on rules or
findings recorded on another task, in another repository. Twice in one week a
worker was refused that task and rebuilt the operator's rules from Queen's
summary of them, and the summary was wrong. Its task text named the other task,
and the listing resolved the id to a title, but not to its content.

The ticket asked that a worker may read any task its own task names. That
would open any task:

- A worker edits its own task's text: it amends facts and appends corrections.
- Email and Jira text become task text when work is filed from them.

So anyone who can put an id into a task's text could open that task to its
worker. Prerequisites already show the safer shape: Queen or the operator
records the relation, and the board reads the record, never the prose.

## Decision

1. **A read link names another task this task's worker may read.** It is
   stored on its own, apart from prerequisites: it does not block, wait or
   order anything.
2. **Only Queen or the operator sets one.** Queen uses
   `swarm_set_task_read_link`; the operator uses
   `POST /api/v1/tasks/{task_id}/read-links`. Both add or remove one link with
   a reason. A worker may not, not even on its own task, unlike a
   prerequisite, which a worker may name on the task it blocks.
3. **It grants a read and nothing else.** The worker holding the task, or the
   one who finished it, reads the linked task with `swarm_read_task_history`:
   its description, amendments, history, evidence and messages. It cannot
   change, move or message the task, and the task does not join its board.
   - The linked task may be in any repository and in any state, finished
     included, and the worker keeps the read after its own task finishes.
   - The worker sees what that task's own worker sees. That includes the
     operator's instruction line on it, every note on its history, and any
     message the operator sent on it. A task has no field kept from its own
     worker, so none is kept from this one. Whoever sets the link decides
     that this is fit to share.
4. **The worker is told.** Its own task lists `read_links`, with each linked
   task's title, state, repository and reason, and a note saying how to read
   them and that what a linked task asks of its own worker is not asked of
   this one.
5. **It is recorded and bounded.** Each add and remove goes on the task's
   history with its reason. Adding the same link with the same reason again
   changes nothing; a different reason is refused until the link is removed.
   A task holds at most 32 links and a Hive 4,096. A task cannot link to
   itself, and a removed task is neither linked nor read.

## What this does not do

- It does not read a task's text for ids. A link exists only when Queen or
  the operator records one.
- It adds no button to the operator's task page. The operator set 1 of the
  533 prerequisites on this Hive by hand, and Queen set the rest, so the
  operator asks Queen or uses the route.
- It does not reach another Hive's task.

## Consequences

- Schema 233 adds the `task_read_links` table. Removing a task removes its
  links.
- Queen's tool list gains one tool. A worker is never offered it. The agent
  tool surface moves to revision 46, so a Queen session that connected
  earlier calls it after it restarts.
- `swarm_read_task_history` says a worker also reads the tasks in its own
  task's `read_links`.

## Evidence

- Domain tests: the reason bound, a task linked to itself, and an exact
  change shape.
- Persistence tests on a real SQLite store:
  - Queen and the operator may link a task, and the worker may then read it;
  - a worker may not link one, not even on its own task;
  - a link opens only the linked task, and only to that task's holder;
  - removing it closes the read, and both changes are on the history;
  - the same link again changes nothing, and a new reason is refused;
  - a task holds at most 32 links;
  - a removed task is neither linked nor read;
  - a finished task is read, and a worker whose own task finished keeps the
    read;
  - links are listed per task.
- An application test: before the link, the worker is refused and cannot set
  one itself; after Queen sets it, every read `swarm_read_task_history` makes
  succeeds; after the operator removes it, the read is refused again.
- An MCP test: Queen's tool sets the link, the worker's own task lists it,
  and the worker reads the linked task in full. A worker's call to set one
  changes nothing.
- An HTTP test of the operator's route: authentication, a repeated add, a
  task linked to itself, and a remove.

## Not proven

- No Hive has run it.
