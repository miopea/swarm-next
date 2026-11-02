# 0145 — Agent answers fit what agents read

- Status: Accepted
- Date: 2026-11-02
- Decided by: the operator, on the optimization pass's per-tool sweep
  (2026-11-02). They chose "Open + 20 recent" for the decision index and
  "Active+Ready full, rest brief" for a worker's own list.
- Amends: ADR 0143, which kept a reader's own open assignments in full. Only
  its Active and Ready work stays in full now.
- Amends: ADR 0144, which counted the whole serialized answer. It counts the
  answer's text now.

## Context

ADR 0144's tally read two hours of real agent work on the operator's Hive.
Tool calls were about 3% of the API's time, so time was not the problem.
Size was. Two answers were, by an estimate of about four bytes a token, at or
past Claude Code's default limit on MCP tool output, 25,000 tokens:

- **The decision index.** Queen's unfiltered `swarm_list_decisions` listed
  the newest 256 decisions. 235 were resolved, 19 withdrawn and 2 pending.
  It came to about 210 KB of text. Settled history was about 92% of it, and
  the two decisions waiting on the operator were a few hundred bytes of it.
- **A worker's own list.** A worker holding 19 open tasks read all 19 in
  full each time it listed its work, about 105 KB of text. One of them was
  Active. Five were awaiting release, six blocked, three in review.

An answer past the limit is cut or refused, and an agent reading a cut index
cannot tell what is missing.

The tally itself overstated sizes. `CallToolResult::structured` sends every
answer twice, once as text and once as `structuredContent`, and the model
reads one copy. Counting the serialized result reported about 2.15 times what
an agent reads.

## Decision

1. **The decision index lists every pending decision, and settled ones 20 a
   page.** Settled decisions come most recently settled first.
   `settled_page` reads older pages. Every page carries every pending
   decision, because an unanswered request is what the index is for, and
   admission already bounds them to 256.
2. **The index's `total` is what the reader's scope holds.** `pending` and
   `settled_total` say how it divides. The caption says rows were left off by
   paging and names the next page. Before, `total` counted the whole Hive
   even for a worker, whose scope is its own raises and the rulings on its
   work.
3. **A task listing carries the reader's own Active and Ready tasks in full.**
   Its other open tasks are in brief, as other workers' tasks already are on
   a whole-board listing (ADR 0143). This holds for a worker's own list and
   for a board reader's own work. `swarm_read_task_history` still reads any
   task in full, amendments included.
4. **The tally counts an answer's text.** The structured copy sent beside it
   is not counted.

The web inbox keeps its own list and is unchanged.

## Consequences

- An agent reading the index sees every request waiting on the operator, on
  any page, however much history the Hive holds.
- A worker reads the work it is doing or can pick up in full. When resting
  work moves to Ready or Active it is in full on the next listing. Until then
  the worker reads it by id.
- Agents connected before this change hold an older tool list, so the tool
  surface revision moves.

## Not proven

- The sizes after this change on the operator's Hive. They need a reload
  and a reading of the tally.
- Whether any agent relied on the resolved decisions the index used to
  carry. A settled ruling is still on a page, and still readable by id.

## Evidence

Measured on the Keeper from 17:32:51Z for 7,238 s: 91 tool calls, 2,763 ms
spent answering, and the API at 13.7 ms of CPU per second.

Tests:

- `past_the_cap_the_index_reports_the_store_total_and_not_the_page`: with
  two pending decisions behind 25 settled ones, the first page lists both
  pending and 20 settled, `total` is 27, and the caption names
  `settled_page 1`. The second page lists both pending again and the last
  five.
- `the_decision_index_pages_settled_history_by_when_it_settled`: a decision
  raised first and settled last leads a page, and pending is on every page.
- `unrelated_pending_decisions_cannot_hide_a_workers_resolved_ruling` and
  `decision_visibility_and_task_correlation_follow_agent_authority`: the
  index keeps each reader's scope.
- `a_worker_reads_its_work_in_hand_in_full_and_its_resting_work_in_brief`:
  Active and Ready in full, Review and Awaiting release in brief, without
  their descriptions.
- `a_board_reader_reads_other_work_in_brief_and_its_own_in_full`: a board
  reader's own Ready task is in full and its own Blocked task is in brief.
- `each_tool_call_is_counted_with_the_size_of_its_answer`: the counted size
  equals the answer's text, with structured content present beside it.
