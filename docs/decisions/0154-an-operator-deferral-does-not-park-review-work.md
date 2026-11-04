# 0154 — An operator deferral does not park Review work

- Status: Accepted
- Date: 2026-11-04
- Decided by: the operator's report and Queen's routing. The operator asked why
  a task had sat in Review for hours: "the queen should have picked it up, so
  there is some issue that we would address with swarm letting this sit". Queen
  routed the fix as proposed, for Review only.
- Amends: ADR 0135, which kept "An operator deferral still covers Review".

## Context

Measured on the Keeper on 2026-11-04. A worker moved a task to Review although
its done-when was not met. The last step could only happen after an update that
the operator starts. The rest of the day went like this:

1. Queen asked the operator. They answered "Wait for a quiet point; I will say
   when".
2. Queen recorded that answer as an operator deferral four times in 47 minutes.
   After that, the task was never offered to her again.
3. It sat in Review, Queen's move, for six hours, until the operator asked why.
   It was the only open Review task in that state when read.

Three things combined:

- **An operator deferral covered Review.** `covers` accepted it on Draft,
  Blocked and Review. It also counts in every run, not only the run that
  recorded it. So once recorded, the task stayed covered until its evidence
  changed.
- **The board shows a wait only for Blocked work.** A task's park and wait are
  read only while it is Blocked. So the board read "Review, Queen's move" with
  no reason, and the task was not on the operator's list. Nothing would bring it
  back when the update landed.
- **The worker's own handoff.** It sent unfinished work to Review instead of
  blocking it on a named wait. ADR 0135 already says Review is not a place to
  wait.

Between runs the queue went further, as read from code rather than watched. It
reads `no_active_review` on an operator deferral as still applying. So the
Queues view files such a task as "scheduled", and the review queue orders it
behind work that needs a move.

## Decision

1. **An operator deferral covers Draft and Blocked, not Review.** A deferral
   saved on Review before this stops covering it. The task returns to Queen's
   review focus and counts as uncovered.
2. **Recording one on Review is refused** with the same exits as an external
   condition. The refusal now names the operator's word among the waits:
   - approved work that only waits to ship goes to awaiting release;
   - work that waits on an outside party, a date or the operator's word goes
     back to its worker with `swarm_return_reviewed_work`, and the worker
     blocks it on a named wait the board shows;
   - work that waits on another task gets it as a prerequisite;
   - a judgment goes to the operator as a decision.
3. **Between runs, a saved receipt whose kind cannot cover a Queen-owned task's
   state reads `fresh_external_check_required`**, as it already did inside a
   run. The queue therefore stops filing such a task as scheduled. For an
   external condition, the queue's behaviour is unchanged: both statuses
   already put it among the rechecks.

`swarm_record_review_disposition` says the same in its description.

## What this does not change

- An operator deferral still covers Draft and Blocked, in and out of a run.
- An operator's answer that parks its task (ADR 0106) moves the task to Blocked
  before it writes its receipt, so it is unaffected.
- Whether a Draft has the same blind spot is recorded as an open question in
  `docs/09-open-questions.md`, not decided here.

## Consequences

- Work the operator has deferred can no longer rest in Review. Queen hands it
  back, and its worker blocks it on a wait the board shows. A "Waiting on:
  operator" wait puts it in the operator's Needs you, and Done returns it.
- A Review task holding an old deferral reads as uncovered until Queen moves
  it. That is the intended correction.

## Validation

- `an_operator_deferral_does_not_park_review_work` (domain):
  - a valid deferral cannot cover Review;
  - its refusal names every exit, including the operator's word;
  - it still covers Draft and Blocked;
  - a deferral saved on Review reads `fresh_external_check_required` in a run,
    in a later run and between runs;
  - one on Blocked is still covered in a run and `no_active_review` between
    them.
- `review_is_not_a_place_to_wait_on_an_outside_condition` (domain): its
  deferral assertion now says a deferral does not cover Review.
- `an_operator_deferral_does_not_park_review_work` (persistence):
  - recording a deferral on a Review task is refused, naming
    `swarm_return_reviewed_work`;
  - a deferral saved before the rule, past the repetition bound, leaves the task
    uncovered;
  - the snapshot reads `fresh_external_check_required` both during the run and
    after it finishes.
- Not yet observed on a live Queen run.
