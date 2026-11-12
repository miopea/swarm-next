# 0171 — A worker records an independent check of its commits

- Status: Accepted
- Date: 2026-11-11
- Decided by: the operator, in an interview on 2026-11-11 about ideas from
  another multi-agent tool, whose coordinator adds a verifier agent that reads
  each worker's diff and runs the tests. Asked who runs the check, they chose
  "the worker runs it". Asked what happens without one, they chose "Show it,
  don't refuse". Asked what the checker sees, they chose "Diff and task brief
  only".
- Extends: ADR 0115 (the check gate binds CI to the task's recorded commits)
  and ADR 0150 (the coordinator settles routine completion evidence).

## Context

Work reaches Review checked by the worker that wrote it, by CI where the
repository has it, and by Queen. Queen reads many tasks a day. Over the last
30 days she sent back 14 of about 1,020 code tasks, and roughly 90 of her
about 1,610 go-aheads say she did not read the diff.

The worker that wrote a change is the one least able to see what it missed,
and its own summary is what everyone downstream reads. A second agent that has
seen neither the reasoning nor the summary, only the task and the change, is a
cheap independent reading. Nothing in Swarm recorded one, or could say whether
a reading still covered the work after another commit.

## Decision

1. **Two worker tools.**
   - `swarm_independent_check_input` (read only) returns the checker's input
     for the caller's task, and a `basis` naming the commits it was built
     from.
   - `swarm_record_independent_check` records the answer: the basis, a
     verdict (`pass`, `partial` or `fail`), what the checker ran, and its
     findings.
2. **Swarm writes the checker's input, and the worker adds nothing.** It holds:
   - fixed rules: read only, judge what is there, do not ask the author;
   - the task's title, description and amendments;
   - the recorded commits, oldest first by commit time, each with its subject,
     repository and changed paths. Commit times are whole seconds, so within
     a tie a parent goes before its child. A commit git cannot read is listed
     and marked so. Each commit lists at most 30 paths, and the list stops at
     16 KiB; what is left out is counted;
   - the changes themselves, up to 48 KiB. Past that, or if git cannot read
     any one of them, no patches are sent, and the checker is told to read
     each commit with `git -C <repo> show`. Part of the change is never
     passed off as all of it;
   - what to check: that the change does what the task asks, that it changes
     nothing else, that no test was removed or weakened, and the repository's
     own checks with their exit codes;
   - the answer's shape: verdict, ran, findings.

   A merge commit is left out when ordinary commits are recorded beside it.
   Recorded alone, it contributes its first-parent diff. Every git read has a
   10-second timeout and a byte cap, and is killed if the call is dropped. All
   the reads for one call share 30 seconds. Past that, commits go unread and
   the checker is sent to git for the changes.
3. **The checker is the worker's to start, and Swarm is provider-neutral.**
   The input is plain text. A worker hands it to a fresh agent from its own
   provider, such as a new subagent. Swarm starts nothing and cannot see who
   answered, so a recorded check is the worker's report of an answer, not proof
   of one.
4. **A check is bound to exact commits.** The basis is the first 16 hex
   characters of a SHA-256 over the task's recorded commits, as the sorted,
   deduplicated list CI observations already bind to. Recording is refused:
   - when the task has no recorded commits;
   - when the basis is not the task's current commits. This is compared
     inside the store's transaction, so an answer built before more commits
     were recorded cannot be filed against them;
   - when `ran` is blank or over 1,000 characters, findings are over 4,000,
     or a `partial` or `fail` has no findings;
   - when the task's commits, listed, are not 1 to 20,000 characters long
     (20,000 is about 480 full commit names), or the head is not 4 to 64
     characters.
     Recording commits has neither limit, and the table does, so these are
     refused in words before the table's own constraint is reached.

   The head stored with it is the newest of those commits git can read, or
   the last recorded when git can read none.
5. **State.**
   - `not_checked`: the task built code and no check is recorded. A commit
     report git could not settle also counts, when it names commits.
   - `checked`: the latest check covers the current commits. It shows the
     head, verdict, what ran and the findings.
   - `stale`: commits were recorded after the latest check. `since` names
     them.
   - Nothing: the task recorded no commits, an empty list, or documentation
     only.
6. **Storage.** A table holds the checks (schema 226). Each task keeps its
   latest 10, and a task's checks go when the task does.
7. **Where it is shown.**
   - The response to a worker's move to Review, with one line naming the tool
     when the check is missing or stale.
   - `swarm_read_task_history`.
   - Queen's board listing, on each task in Review.
   - The task card, as a "Check" row beside CI for work in Review or Awaiting
     Release, read from `GET /api/v1/tasks/{id}/ci`: the verdict and the commit
     it covers ("Partial at 8b7d6e5f"), "Stale" or "Not checked". It shows
     whether or not the check gate is on. A check the Hive cannot read shows
     as "Unreadable", with the reason in the tooltip, and the CI row beside it
     still shows.
8. **Shown, never refused.** Moving to Review, the coordinator's settlement,
   completion and the check gate do not read it. A missing, failed or stale
   check is information for Queen.

## Consequences

- Each check costs a fresh agent's tokens, so it runs when the worker chooses
  to. The Review response is the only prompt to run one.
- Queen sees a verdict that a worker could have written without running
  anything. "Ran" and the findings are what she weighs it on. Swarm controls
  what the checker is given, not whether a checker answered.
- Re-reporting the same commits keeps a check current. Adding one makes it
  stale, and the worker builds the input again.
- Staleness follows the commits the worker records, as CI's binding does. A
  commit made and never recorded leaves the check reading as current.
- A large change, or one with a commit git cannot read, sends the checker to
  git instead of carrying the diff, so the checker must be able to reach the
  task's repository.
- A task with more recorded commits than fit 20,000 characters cannot have a
  check recorded. That is refused in words, and Queen sees the task as not
  checked, or its earlier check as stale.
- Must-not-touch paths, the other idea kept in the same interview, are a
  separate decision.
- Not proven by the tests: a check run by a real fresh agent on live work, or
  whether workers run one without being asked.
