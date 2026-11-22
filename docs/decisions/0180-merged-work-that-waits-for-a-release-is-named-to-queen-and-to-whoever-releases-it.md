# 0180 — Merged work that waits for a release is named to Queen and to whoever releases it

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), in three answers:
  - "Revised scopes" over the scope first written on the ticket;
  - "From merge, measured": count from the merge, not from Awaiting Release,
    set the threshold from measured release times, one row per release line,
    nudge whoever holds the release, and Queen decides when to tell the
    operator;
  - "Both": raise a row when another task is recorded as waiting on the merged
    work, 30 minutes after the merge, and also when a repository's slow tail
    is passed.
- Amends:
  - ADR 0039: a coordinator row may now cause one message to a worker.
  - ADR 0149: a message about a task the worker is named to release is not
    held behind its Active task.

## Context

A task in Awaiting Release has merged, or is about to, and waits for a
release. Nothing said when that wait had gone wrong.

- `next_move_owner` is `release` for every task in that state: an event, not a
  person. So no worker or Queen row named it.
- Unasked stalled work listed such a task only after 72 hours.
- Swarm stored no merge time and no merge commit.
- Swarm did not record who would release a task. Queen parked 929 of the 930
  tasks that have entered Awaiting Release, and often named the releasing
  worker in her note, in prose.

The incident: a pull request merged and its work waited about 80 minutes for a
release. Other work waited behind it, but the board recorded that only in
prose. The release was held by a worker who was not the task's assignee.

A threshold counted from entering Awaiting Release is the wrong clock. 55% of
the measured stays (344 of 623) were parked before they merged. Counted from
entry, the incident's task would have alarmed five hours before it merged.

### Measured, 2026-11-22, over the 623 stays that ended in the last 30 days

The merge time is read from git: for each recorded commit, the committer time
of the first-parent commit on the remote default branch that brought it in.
The wait runs from the later of the merge and the park, to the task leaving
Awaiting Release.

- Across the Hive, half of the waits ended within 1.4 hours, 90% within
  18.8 hours and 95% within 31.7 hours.
- Repositories differ widely:
  - one repository, half within 0.8 hours and 90% within 7.7 hours;
  - another, half within 7.7 hours and 90% within 33.8 hours;
  - this one, half within 9.0 hours, because its releases are batched on
    purpose.
- A single threshold is therefore wrong in every repository at once. 30
  minutes would flag 66% of all waits. Past each repository's own 90th
  percentile flags 54 in 30 days.
- The incident's 80 minutes was ordinary for its repository, so no
  repository threshold would have flagged it. What made it costly was the
  work waiting behind it.
- 90 stays had another task recorded as waiting on them (`task_prerequisites`).
  Of those, 57 waited more than 30 minutes after the merge, and 34 more than 2
  hours.
- 162 further stays could not be measured, because their commits never
  reached the remote default branch (squash merges, for example).
- Local refs learn of a merge quickly. A workspace's remote-tracking ref
  carried the merge within the same minute in half of the stays, within 15
  minutes in 90%, and within 46 minutes in 95%. In 574 of 623 stays it carried
  the merge before the task left Awaiting Release. So the detector reads local
  refs and never fetches.

## Decision

1. **Merged is a git fact, read from the workspace's own refs.** A task in
   Awaiting Release has merged when every commit it recorded is an ancestor
   of its workspace's remote default branch: `refs/remotes/origin/HEAD`, or
   `refs/remotes/origin/main` when that is not set. Its merge time is the
   latest of its commits' merge times, each the committer time of the
   first-parent commit that brought it in. Its merge commit is the one with
   that time. Swarm never fetches: an unread or stale ref only delays a row,
   and the measurement above bounds how much. The release sweep reads this
   every five minutes, on the thread that already runs git for releases.
2. **The clock starts at the later of the merge and the park.**
3. **Two triggers. Either raises the row:**
   - **Holding others.** Another task that is not finished lists this one as
     a prerequisite, and 30 minutes have passed.
   - **The slow tail.** The wait has passed the 90th percentile of this
     repository's measured waits over the last 30 days. A repository with
     fewer than 20 measured waits uses 24 hours, which is above the Hive-wide
     90th percentile of 18.8 hours.
4. **One row per release line.** A release line is a workspace, because one
   release carries every merged task in it. Its row hangs off the task that
   has waited longest. It names:
   - the worker who releases it, and whether Queen named them;
   - the merge commit;
   - how long it has waited and which trigger fired;
   - the unfinished tasks waiting on it, and the other merged tasks waiting
     in that workspace;
   - whether a pending card is linked to it, and whether it is Queen's or the
     operator's.

   The row clears itself when the task leaves Awaiting Release. Any other
   change to the task clears it too, like every coordinator row, and the next
   pass raises it again while the wait lasts.
5. **Who releases it is a fact Queen records.** When Queen parks a task in
   Awaiting Release she may name the releasing worker with
   `release_worker_id`, in the same transaction as the move. Without one, it
   is the task's assignee. The name lasts that stay: entering or leaving
   Awaiting Release clears it. The named worker may message Queen on that
   task, and Queen's messages to them about it are not held behind their own
   Active task. It grants nothing else.
6. **Swarm reminds that worker once per stay.** The message is from Swarm, a
   new sender, and never from Queen. A worker's auto mode takes Queen's
   message as her release instruction (ADR 0131), so a reminder in her name
   would be a timer authorising a release. The reminder says it authorises
   nothing, names the task, the merge commit and the wait, and asks the
   worker to release it if Queen's go covers that, or tell Queen what it
   waits on. Delivery is the ordinary task-message path. A reminder:
   - goes only to a worker, which the schema enforces;
   - is not a message owed to the assignee, so it never holds a task's
     automatic settlement;
   - is cancelled unsent when its task leaves Awaiting Release;
   - waits for the next pass when the message queue is full, while the row
     still reaches Queen.
7. **The operator is never told by this row.** Queen reads it and decides
   (ADR 0061, ADR 0082: no age alone makes an operator decision).

## What this does not do

- It does not see CI on the merge commit or Queen's go. Neither is a stored
  fact. The row names only what Swarm stores.
- It does not see squash or rebase merges, whose recorded commits never reach
  the default branch.
- It does not move or close any task, and it does not release anything.
- It does not hide the row while a card waits on the operator. That is a
  separate decision.

## Consequences

- Schema 232:
  - the new attention kind in `coordinator_actions`;
  - `swarm` as a task-message sender, which rebuilds `task_messages`;
  - `release_waits`, one row per stay whose merge git was asked about,
    dropped 30 days after the stay ends;
  - `task_releasers`, the worker Queen named.
- A Hive taking this release has none of its history measured. The sweep
  fills it in batches of 32 ended stays, so a busy Hive has its own
  thresholds within about two hours, and until then a repository uses the
  24-hour default.
- Each sweep runs git in the workspaces of tasks in Awaiting Release: one
  ancestry check and one first-parent walk per recorded commit, at most 64
  live stays and 32 ended ones a sweep.
- Queen's brief gains a paragraph: name the releasing worker when you park a
  task whose release is not its assignee's, and what the row and the reminder
  mean.

## Evidence

- Domain tests: the clock starts at the later of merge and park; the 90th
  percentile is nearest-rank; a repository with fewer than 20 waits uses the
  default; holding others trips at 30 minutes and not before.
- Storage tests:
  - a stay is read while it lasts and once more after it ends;
  - a stay released between the read and the write is still closed;
  - stays older than the window are dropped;
  - the 30-minute trigger runs from the merge, not the park;
  - an unmeasured repository waits a day, and a measured one uses its own
    percentile;
  - one row per workspace, naming the longest wait and the others;
  - the release clears the row and cancels an unsent reminder;
  - a reminder does not hold settlement;
  - a full queue defers the reminder, and the next pass sends it once;
  - a reopened store raises nothing twice;
  - a named releaser gets the reminder past their own Active work, may answer
    Queen, and loses the name when the stay ends;
  - a releaser is refused on any other move, and Queen is refused as one;
  - Swarm writes only to a worker, in code and in the schema.
- Git tests against real repositories: a merged pull request is dated by its
  merge commit; a commit pushed onto the line is dated by itself; work the
  remote branch lacks is not merged; what git cannot read records nothing.
- **Not proven:** no live Hive has run the sweep, raised a row or delivered a
  reminder.
