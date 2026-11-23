# 0185 — Queen's coordination attention answers in short rows

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), answering "Revised scopes": compact
  rows by default, filterable by state, owner, age and workspace, with full
  detail on request, and each filtered answer counting what the filter left
  out.

## Context

`swarm_list_coordination_attention` is how Queen finds what waits on whom. It
took no arguments and returned sixteen sections of detail, each row-capped,
with no bound on the whole. Queen saved the answer to a file and ran jq over it
to read it.

A filtered answer has a failure of its own: an empty one reads as "nothing
waits" when the filter only hid what does.

## Decision

1. **Compact by default.** Each item of every section becomes one row:
   - its section, task and title;
   - the task's state and whose move it is (its next-move owner, the same one
     Queues shows);
   - its age in seconds;
   - why it waits, in at most 160 characters;
   - `ref`, the item's own ids where a follow-up tool needs them: an attention
     action, a message and its claim, a review run, a decision.
   The board's counts by state and by owner come with it.
2. **Filters.** `state`, `owner`, `workspace` (an exact path) and
   `min_age_seconds`. A row with no task, such as an unfinished review run, is
   left out by any task filter.
3. **What was left out is counted.** Per section the answer says what it
   showed, what the filters left out, what the row cap left out
   (`limit_per_section`, 8 by default, at most 64), and whether the section
   itself stopped short of every item. Sections with nothing in them are named.
4. **Full detail by section.** `detail: "full"` returns the answer as it was,
   and `sections` narrows it, or the compact answer, to the sections named. A
   name that is not a section is refused, so a typo cannot read as an empty
   section. Each section's guidance is in the full answer, and the compact
   answer says to read a row's section in full before acting on it.
5. **Every section has rows.** A test holds the compact mapping to the keys
   the full answer actually returns, so a section added later cannot drop out
   of the compact answer unnoticed.

## What this does not do

- It changes nothing in what each section finds, and no section's guidance.
- It does not change `swarm_list_tasks`. A row's "why" is what triage needed
  without a history read per task.
- It adds no byte budget. The row cap, with its count, bounds the answer.
- It adds nothing for the operator.

## Consequences

- No schema change. The tool gains arguments, so the agent tool surface moves
  to revision 48.
- From the reload on, a call with no arguments gets the compact answer, in a
  session that connected earlier too. That session's cached schema lists no
  arguments, but the client passes one it does not list: a call on this Hive
  with an unlisted field reached the server, which refused it by name. So
  such a session asks for `detail: "full"` as the compact answer's note says,
  and needs no compatibility path.
- Her automation prompt names sections, and the rows carry their section
  names. A message row carries its `message_id` and `claim_id` in `ref`, which
  is what reconciling it needs.

## Evidence

- Unit tests:
  - each item becomes one row, with its age taken from the item or its task;
  - a long reason is cut to 160 characters;
  - owner and workspace filters, and an age and state filter, each with what
    they left out, including a row with no task;
  - the row cap and a section truncated at its source are both said;
  - `sections` narrows both answers, an unknown name is refused, and a section
    not asked for is not called empty.
  - a message row carries its message and claim ids, and leaves out a claim it
    never had.
- An MCP test:
  - the compact mapping covers exactly the full answer's sections;
  - a draft is a row of Queen's with its state, owner and reason;
  - the compact answer is smaller than the full one;
  - a workspace filter shows one task and counts the other;
  - the full answer narrows to one section;
  - an unknown section and an unknown owner are refused.

## Not proven

- No Hive has run it, and the full answer's size on this Hive was estimated,
  not measured.
