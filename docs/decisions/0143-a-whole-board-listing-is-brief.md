# 0143 — A whole-board listing is brief

- Status: Accepted
- Date: 2026-11-02
- Decided by: the operator, on the optimization pass before the next release
  (2026-11-02). Asked how a whole-board listing should shrink, they chose
  "Brief board": each open task as what it is, who holds it, where it stands
  and why it waits; the reader's own assignment in full; the closed list cut to
  the most recent 50.
- Amends: the whole-board listing described in the `swarm_list_tasks` tool
  contract. A worker without board access is unchanged.

## Context

`swarm_list_tasks` gives a whole-board reader every open task in full: its
description, every amendment, any Jira text that changed under it, and the
full id of each record its text names by a fragment. Below that is a one-line
index of closed work, as many entries as fit in 64 KB.

Measured on the operator's Hive from one listing saved on 2026-11-02:

| Part | Size |
| --- | --- |
| 44 open tasks, in full | 229 KB |
| closed index, 295 entries | 68 KB |
| whole reply | 291 KB, about 75,000 tokens |

Descriptions were 44% of the open part. The reply overflowed an agent's tool
output twice that day.

Queen reads the whole board on each automation review, about 100 a day on
that Hive, and two workers there have board access. Queen routes from what a
task is and where it stands. She reads in full only the one she acts on, and
`swarm_read_task_history` already returns that task in full.

## Decision

1. **A whole-board listing carries other work in brief.** For Queen and for a
   worker with board access, each open task not assigned to the reader is
   listed with named fields only:
   - always: `brief: true`, id, title, state, priority, workspace, assigned
     worker, next-move owner and when it last changed;
   - when present: the blocker note cut to 240 characters and marked, the
     blocked-until time, the structured wait, prerequisites by id, title and
     state (every one not removed: an unfinished one is why the task waits,
     and a completed or abandoned one is why it is Queen's to check), dispatch
     and outcome-delivery state, and the park;
   - as flags or counts: a returned review, clarification waits, amendments,
     and whether Jira's text changed under it.

   The entry is built from named fields rather than the task with fields
   removed, so a field added to a task later does not grow every listing.
2. **What an outside tool said when it filed a task stays in the brief entry**
   (ADR 0126). Queen routes a filed draft from it, and it is a few fields.
3. **The reader's own open assignments stay in full**, with amendments, Jira
   text and resolved id fragments, because that is the brief the reader works
   from.
4. **The closed index names the 50 most recently closed tasks**, newest first,
   with the total. It named about 300.
5. A note on the listing says what is left out and that
   `swarm_read_task_history` reads any task in full. A worker without board
   access still gets its own assignment in full, as before, and so does the
   listing of tasks a worker filed.

## Consequences

- **Reading a task to act on it costs one more call.** Queen reads a draft's
  description with `swarm_read_task_history` before routing it. The cost moves
  from every listing to the tasks acted on.
- **Id fragments are resolved where the text is read.** A brief entry has no
  description, so it names no fragments. The full read resolves them as
  before, and the tests that pin this now read there.
- **Amendments are counted, not carried.** A reader learns a task has
  corrections and reads them with the task.
- Queen's triage note asked for a way to ask for a summary or one state at a
  time. The brief default answers the size. A filter is left out until a
  reader needs one.

## Not proven

- **The size on the live board.** Simulated from the same saved listing: the
  open part falls from 229 KB to about 34 KB and the closed index from 68 KB
  to about 12 KB. It is measured on the Keeper after the reload.
- **Whether Queen's reviews change.** On 2026-11-02 her automation runs ended
  in 4 to 52 seconds without acting on two Review tasks named in their focus.
  A smaller listing may help; nothing here shows it will.

## Evidence

Tests:

- `a_board_reader_reads_other_work_in_brief_and_its_own_in_full`: another
  worker's task is brief, carries no description or amendment text, counts
  its amendment, and cuts a 400-character blocker note to 240 plus a marker;
  the reader's own task keeps its description.
- `queen_reads_the_board_in_brief_with_the_last_fifty_closed`: Queen's listing
  is brief and names 50 of 55 closed tasks, with the total.
- `queen_reads_what_a_tool_said_when_it_filed` (ADR 0126) is unchanged and
  passes through the brief entry.
- The three id-fragment tests read the task with `swarm_read_task_history`.
  One of them asserts an absence, and a brief entry would have passed it
  without testing anything.

Ablations, each failing at least one of those tests, with the file restored
byte for byte:

- board readers given the full listing again;
- the closed index not cut to 50;
- the reader's own work listed in brief too;
- the blocker note not cut;
- a brief entry without what the filing tool said.
