# 0133 — Jira descriptions follow Jira until a worker holds the task

- Status: Accepted
- Date: 2026-10-30
- Decided by: Queen, routing task 01a18eb6 (found by Scout). Her design rule:
  "a routed task's description governs what the work is for, so do not
  overwrite it under a worker; refresh only while a task is unrouted (Ready, no
  worker), and for a routed task record the changed Jira text as an amendment or
  activity row the worker will see."
- Builds on: ADR 0023 (Jira canonical synchronization boundary), which already
  says adding an issue "establishes an ongoing synchronization relationship,
  not a snapshot import".

## Context

The Jira sync updated an existing task's title and state, and never its
description. The description was written once, at import. Scout found 18 AI
search tasks from WWD whose Jira issues had been rewritten three times in a day
while the board kept the import-time text. In one of them, WWD-7053, the board
copy still said merging was the operator's decision. Jira by then said Queen
may release once checks pass, and added a timeout, an off switch and a safety
message. Workers read the board copy through `swarm_list_tasks`. Only the
dashboard's detail view fetched the live Jira text, and workers don't use it.

Writing Jira's text straight over every description would be wrong in two ways:

- A task a worker already holds would get a different job under it, and
  nothing would tell the worker.
- A description someone edited on this board would be overwritten on the next
  sync, every few minutes, with text nobody had just changed.

## Decision

1. **Schema 217 keeps the description each linked issue last had in Jira**, in
   `jira_issue_links.jira_description`. Existing links are backfilled with
   their task's own description. That is the import-time copy, unless someone
   edited it here since. So an issue edited in Jira before the upgrade counts
   as edited at the first sync after it, and the backlog of stale briefs clears
   itself.
2. **A sync acts only when Jira's text differs from what it last read.** An
   unchanged issue re-sent every few minutes never touches the description, so
   an edit made on this board survives until Jira itself is edited again. Jira
   priority already works this way.
3. **A task nobody holds takes Jira's text.** That means Draft or Ready, with
   no worker. The sync writes the new description and records "Description
   refreshed from Jira KEY" as a Jira-authored `details_updated` activity row.
4. **Any other task keeps its description.** That covers a task a worker holds,
   one in progress without a worker, or a closed one. The link records when
   Jira's text was kept back (`jira_description_unapplied_at`), and the task's
   history says so in one Jira-authored row.
   - `swarm_list_tasks` shows the kept-back text beside the task, as
     `jira_description_change` (issue key, Jira's text, when it was read).
   - A note with it says the description still governs, and the worker should
     ask Queen where Jira's text changes the scope or acceptance.
   - Queen's whole-board listing carries the same field.
5. **A kept-back change is applied once the task is unrouted**, at the next
   sync, without waiting for another Jira edit.
   - It stops being listed when the task's description is made to match Jira's.
   - It also stops being listed when a later sync applies it.

Jira-authored activity rows do not count as acting on a task. The staleness
clocks read only worker and operator actions, so a Jira edit cannot hide a
stuck worker.

## Why not an amendment

Amendments are corrections of fact by a named worker. The table requires a
worker author, and their note tells the reader to believe them over the
description. A Jira rewrite has no worker author, and it may change scope,
which an amendment must never do. Giving it amendment standing would let
upstream text silently redirect work. Listing it beside the description, under
a note saying the description governs, tells the worker without changing what
the task is for.

## Consequences

- Unrouted Jira-backed tasks carry Jira's current brief. Until this ships,
  whoever routes a Jira-backed task should hand the worker the live Jira text.
- A worker holding a Jira-backed task sees the issue was rewritten the next
  time it reads its assignment. Nothing interrupts it mid-turn. This is the
  same reach an amendment has.
- A description edited on this board is replaced the next time Jira's own
  text changes, if the task is still unrouted. Jira stays canonical for
  unrouted work, as ADR 0023 has it for state.
- The upgrade cannot tell an import copy from a local edit. An unrouted task
  whose description was edited here before schema 217, and now differs from
  Jira's, therefore takes Jira's text once at the first sync after the
  upgrade. A held task in the same position lists Jira's text once instead.
- Draft counts as unrouted alongside Ready. Queen's guard named Ready, but
  Jira's backlog, icebox and triage statuses map to Draft, and a Draft task
  with no worker is not held by anyone.
- Refreshing a description changes the task's evidence revision. Any Queen
  assessment of that task reads as evidence changed, which is correct: its
  brief changed.
- The conversion from Jira's rich text still drops issue links, mentions and
  dates. That is a separate defect, filed as 01a18ee3.

## Validation

- `an_unrouted_task_takes_a_description_edited_in_jira`: the refresh happens,
  its activity row is written, and an identical re-sync records nothing and
  wakes no client.
- `a_task_somebody_holds_keeps_its_description_and_lists_jiras_beside_it`:
  - A held task's description is kept and Jira's text is listed.
  - Re-reading the same edit writes no second row.
  - Unassigning applies the kept-back text.
- `a_description_edited_here_survives_until_jira_is_edited_again`.
- `a_draft_nobody_holds_takes_a_description_edited_in_jira`.
- `a_closed_task_keeps_its_description_until_jira_reopens_it`: closed work
  keeps the text it was closed against, and takes Jira's when Jira reopens it
  to nobody.
- `matching_the_description_to_jira_settles_a_kept_change`.
- `an_upgraded_hive_refreshes_a_description_jira_edited_before_the_upgrade`:
  the migration from 216 backfills the column from the task, and the first
  sync after it refreshes the stale copy.
- `a_worker_reads_jiras_edit_beside_the_brief_it_kept`: a worker's own
  `swarm_list_tasks` carries Jira's text and the note, with its description
  unchanged.
- Each test was broken on purpose to check it catches the failure:
  - Removing the description step fails all four persistence tests.
  - Overwriting under a worker fails the held-task test and the existing
    `issue_sync_is_idempotent_and_preserves_swarm_owned_work`.
  - Comparing against the task instead of Jira's last text fails the
    edited-here test.
  - Leaving Draft out, treating closed work as unrouted, and dropping the
    match filter each fail their own test.
  - Removing the listing decoration fails the API test.
