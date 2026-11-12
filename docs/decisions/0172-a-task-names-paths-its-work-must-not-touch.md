# 0172 — A task names paths its work must not touch

- Status: Accepted
- Date: 2026-11-11
- Decided by: the operator, in the same interview as ADR 0171, about ideas
  from another multi-agent tool whose task briefs say what a worker owns and
  what it must leave alone. Asked about a must-not-touch field, they chose
  "Path patterns, set by filer or Queen". Asked what happens when work
  crosses one, they chose "Flag it to Queen at Review".
- Extends: ADR 0115 (the check gate binds CI to the task's recorded commits)
  and ADR 0171 (an independent check shown beside the work, never gating it).

## Context

About 14% of the last 1,378 tasks state a scope limit in prose: "do not touch
the terminal host", "leave the lockfile alone". Nothing checks those
sentences. A worker can miss one, and Queen finds out only if she reads the
diff closely enough to notice.

Swarm already records which paths each of a task's commits changed, checked
once when the worker reports them. A limit written as path patterns can be
compared with those paths. A limit written as prose cannot, so free text is
not part of this.

## Decision

1. **Patterns, relative to the task's repository.** For example
   `crates/swarm-terminal/**` or `web/node_modules/**`.
   - `*` matches within one part of a path and `?` matches one character.
   - `**` as a whole part matches any number of parts, including none.
   - A pattern with no wildcard, or one ending in `/`, also covers everything
     under it, so `web/node_modules` covers `web/node_modules/react/index.js`
     and not `web/node_modules_extra/a.js`.
   - The match is computed over a table of pattern and path parts, so a
     pattern full of `**` cannot make it slow.
2. **What is refused when patterns are set.** A task carries at most 32, each
   at most 256 characters. A pattern is refused when it is empty, starts with
   `/`, contains `\`, contains a `..` part, or contains a control character.
   Patterns are trimmed, a leading `./` is dropped, and duplicates are
   removed. Refusing a pattern at filing files nothing.
3. **Who sets them.**
   - Whoever files the task, with `must_not_touch` on `swarm_create_task`. The
     task and its patterns are written in one transaction.
   - After that, only Queen, with `swarm_set_task_must_not_touch`. She gives a
     reason, which goes on the task's record. An empty list removes them, and
     a finished task is not changed.
   - The worker holding the task has no tool that changes them, and the
     application refuses the call if it makes it anyway. It cannot loosen its
     own limits, or tighten them. It asks Queen, and the note beside its
     patterns says so.
4. **Recorded.** Every change writes an activity row naming the actor, the
   patterns before and after, and the reason, in the same transaction as the
   change. Each list is spelled out up to 1,000 bytes and the rest counted,
   and the note is held to the 4,000 bytes other activity notes are. A table holds the current patterns, one row per task that has any
   (schema 227). A row that no longer reads as valid patterns is an integrity
   failure, never an empty list, because an empty list would hide a crossing.
5. **The comparison.** At read time, every pattern against every changed path
   of every recorded commit. Each crossing names the commit, the path and the
   first pattern it fell under. A commit lists at most 20 crossed paths, and
   the report lists at most 50 commits; the rest are counted. Like CI and the
   independent check, it follows the commits the worker records, so a commit
   never recorded is never compared.
6. **Where it is shown.**
   - The worker's own Active and Ready tasks in `swarm_list_tasks`, and the
     task in `swarm_read_task_history`, carry the patterns and who changes
     them.
   - The response to a worker's move to Review carries the report. When a
     commit crossed, one line asks the worker to explain each crossing in its
     handoff or undo it.
   - Queen's board listing, on each task in Review: how many commits were
     compared and crossed, and the first three crossed paths.
   - `swarm_read_task_history` carries the full report.
   - The task card, as a "Scope" row beside CI for work in Review or Awaiting
     Release, read from `GET /api/v1/tasks/{id}/ci`: "Within" or "Crossed",
     with the commits and paths in the tooltip and for screen readers. A
     crossing has the amber tone of something to look at, not the red of a
     failure.
7. **Shown, never refused.** Moving to Review, the coordinator's settlement,
   completion and the check gate do not read the patterns. Some crossings are
   needed, and Queen decides.
8. **A task without patterns behaves as it did before.** It has no row and
   no activity is written. Its listing, its Review response and Queen's board
   say nothing about patterns; the history's report and the card's read carry
   `null`, and the card shows no Scope row.

## Consequences

- A filer who knows where the work must not go can say so in a form Swarm can
  check. A limit still written only in the description is still unchecked.
- A worker that needs a path it must not touch has to ask Queen before
  touching it, or explain the crossing at Review. Either way she sees it.
- The patterns are the filer's judgement at the time of filing. A wrong one
  produces crossings Queen dismisses, and she can correct it.
- Not built:
  - a web editor for the patterns, so the operator sets them through Queen;
  - patterns on drafts filed by the intake agent, which files from email and
    other sources that rarely name paths;
  - any comparison with commits the worker did not record.
- Not proven by the tests: whether filers use the field, or whether workers
  ask Queen before crossing rather than explaining afterwards.
