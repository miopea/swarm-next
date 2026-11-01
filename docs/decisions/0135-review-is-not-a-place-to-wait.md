# 0135 — Review is not a place to wait

- Status: Accepted
- Date: 2026-11-01
- Decided by: the operator, answering two questions recorded on task 01a1978e
  ("Both, strict" and "Yes, same PR"). Their report: they kept having to tell
  Queen to deal with tasks sitting in Review and in Draft.
- Amends: ADR 0082 (evidence-backed review coverage), on which states an
  external condition covers. Amends ADR 0076 (explicit task prerequisites), on
  which states a prerequisite takes off Queen's list.

## Context

Measured on the Keeper on 2026-11-01:

1. **`swarm_promote_task` looked like it moved drafts on.** It only moves a
   task to the front of the delivery queue. It accepted a Draft and answered
   `{"promoted": true}`. Queen promoted two drafts three times, believed they
   had moved, and found them still in Draft eleven minutes later.
2. **Approved work waited in Review, and Swarm counted it as handled.** Each
   run, Queen must account for every task she owns. An external condition
   covered a Review task, so she recorded "approved, waiting on merge and
   deploy" on each approved task instead of moving it. One worker had 18
   approved tasks waiting this way, some re-noted 19 times. Awaiting release
   was never refused: once the operator asked, Queen moved ten in a minute.
3. **The alarm had been on for two weeks.** A run that leaves any Queen-owned
   task uncovered is recorded as incomplete. In 14 days about 1,600 runs
   finished, and all but 5 were incomplete, so the flag said nothing. One
   reason: a draft filed behind unfinished work stays Queen's, since a draft
   cannot move to Blocked, and nothing she records can cover it. The finish
   reply never named what was left.

## Decision

1. **Promoting a Draft is refused.** The refusal names
   `swarm_transition_task` (ready or abandoned). A promotion's reply states the
   task's state and `state_changed: false`.
2. **Review refuses an external condition.** Saved receipts recorded before
   this stop covering Review: coverage now asks whether a receipt's kind still
   covers the task's current state. The refusal names the exits:
   - approved work that only waits to merge or ship goes to awaiting release,
     which completes itself when the deployment is recorded;
   - work that is missing something, or waits on an outside party or a date,
     goes back to its worker with `swarm_return_reviewed_work`, so the worker
     can block it with a named wait (Review cannot move to Blocked);
   - work that waits on another task gets that task as a prerequisite;
   - a judgment goes to the operator as a decision.
   An operator deferral still covers Review. An insufficient-evidence note is
   still recordable there and still covers nothing. Queen's review snapshot
   agrees with coverage: a saved receipt whose kind no longer covers the task's
   state reads `fresh_external_check_required`, never `covered_for_current_run`,
   so the task stays in her review focus.
3. **A Draft behind unfinished work waits, like Review does.** While any of
   its prerequisites can still finish, its next move is `blocked`, not
   Queen's, and it does not wake her. It returns to Queen to triage when they
   finish. A prerequisite that was abandoned or removed never finishes, so it
   holds nothing, for a draft or a review, and the task is Queen's to re-route.
   The wake count already treated blocked work this way. The queue labels a
   waiting draft "Draft waiting on N prerequisites". Queen's brief no longer
   tells her to sort drafts into "blocked", a move a draft cannot make: it
   says ready or abandoned, and a prerequisite for work that waits on a task.
4. **An incomplete finish names what is left.** `swarm_finish_automation_run`
   returns `uncovered`: up to 16 tasks with state, title and which duty was
   missed (review or worker recovery), the total, and whether coverage could be
   read in full. The run is already recorded when the list is built, so a
   failed read marks it `unavailable` and never fails the finish.
5. **Every refusal reaches Queen as a refusal.** The Review check also runs in
   the application layer. A refusal from the store reads "database integrity
   check failed", which points at the database instead of the move to make.

## What this does not change

- An external condition still covers Blocked work.
- A draft that waits only on the operator's word, with no prerequisite, is
  still Queen's. Once that word is on the task, as a resolved decision or a
  task-linked statement, she records an operator deferral and it is covered. No
  special case was built for it.
- Moves between states are unchanged.

## Consequences

- After deployment, Review tasks still holding an old "waiting" note show as
  uncovered until Queen moves them. That is the intended correction.
- A worker can get a Review task handed back to block on an outside wait. Before
  this, Queen held it in Review.
- An incomplete run now says which tasks made it incomplete, so the flag can be
  acted on.

## Validation

- `review_is_not_a_place_to_wait_on_an_outside_condition` (domain): the
  refusal names all four exits; Blocked still covers; operator deferral still
  covers Review; insufficient evidence is recordable and covers nothing.
- `a_draft_behind_unfinished_work_waits_and_returns_when_it_finishes` (domain):
  also an abandoned or removed prerequisite holds neither a draft nor a review.
- `a_draft_behind_unfinished_work_wakes_her_only_when_she_can_triage_it`
  (conductor): the wake count and the board agree.
- `an_incomplete_finish_names_what_is_left_without_failing` (domain): the
  named list is bounded, and a failed read or a removed task says
  `unavailable`.
- `the_run_brief_names_finished_work_and_the_move_for_each_state` (brief): no
  "blocked" for drafts.
- `approved_work_cannot_wait_in_review_on_an_outside_condition` (persistence):
  - recording is refused;
  - a receipt written before the rule, in the same run, at the current
    revision and past the repetition bound, leaves the task uncovered, and the
    snapshot does not call it covered;
  - moving the task to awaiting release covers the run.
- `a_draft_behind_unfinished_work_is_not_queens_until_that_work_finishes`
  (persistence).
- `a_review_wait_and_a_draft_promotion_are_refused_with_the_move_to_make`
  (application): both come back as `Refused`, not a store failure.
- `promoting_a_draft_is_refused_and_a_promotion_states_what_the_task_is` and
  the extended `unattended_queen_run_blocks_external_effects_and_closes_only_its_exact_marker`
  (MCP): `uncovered` names the untriaged draft.
- `a draft behind unfinished work says it is waiting, not awaiting triage` (web).
- Not yet observed on a live Queen run.
