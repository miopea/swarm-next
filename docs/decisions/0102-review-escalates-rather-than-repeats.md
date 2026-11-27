# ADR 0102: A review escalates rather than repeats

Status: Accepted implementation direction for the operator-requested stall control.

Work must not sit for days while nobody is asked about it. Reporting the
condition was tried and did not stop it, so the review path now REFUSES a fresh
assessment of work that has stood still past a bound with no question raised.
This is the first control in that path that refuses rather than reports.

## Why reporting was not enough

The repetition was already measured, already stored and already delivered.
`times_seen` has counted unchanged re-assessments since schema 179, swarm-api
serves them as `reviews_repeating` at a threshold of three, and the comment
beside it describes this exact failure in Queen's own words. On 2026-09-18 that
surface held 31 receipts at or above the threshold, 15 of them at twenty or more,
and one at fifty-eight. The count kept climbing while being displayed.

Measured the same day: 741 `corrected` events against 23 `state_changed` in
twenty-four hours — thirty-two re-assertions for every piece of work moved. And
of 33 live non-terminal tasks with no pending decision, nineteen had not moved in
over two days and six had sat more than a week. Three were waiting on the
operator: a capability nobody had been granted, a design answer nobody had been
asked for, and a config value nobody had requested.

## Why the board could not show it

`NextMoveOwner::derive` returns `Operator` only when a decision is already
pending. Work genuinely waiting on a person, with none filed, reads as Queen,
Blocked or Release. Asking is the act that makes work visible as operator-gated,
so the one state the board structurally cannot display is nobody having asked.
One of the three carried a block note reasoning the answer was "not a crisply
fileable operator decision" — the choice not to ask was deliberate, and then
invisible for sixty-seven hours.

## The control

A stall is: no `state_changed` activity for `MAX_UNASKED_STILL_SECONDS`, and no
pending decision either owned by the task (`decision_requests.task_id`, primary
membership) or linked to it. Both forms count; reading only the link table would
refuse work whose operator question was already pending, which is the worst
version of this control.

Recording a Queen review disposition on stalled, unasked work is refused with
`ReviewNeedsEscalationNotRepetition`. The refusal names every exit — raise the
question, move the task, or abandon it — and each is already in the caller's
hands, so it cannot strand what it refuses. It sits after the replay check, so an
idempotent replay still returns early and only a NEW assessment is refused.

Three days, because that is the failure being corrected: the complaint named 67,
72 and 78 hours. Work that moves at all moves inside a day. The bound is declared
once in the persistence boundary and re-exported by swarm-api, so the surface
that reports a stall and the guard that refuses one cannot disagree.

## What escalation actually does

Raising the question does not merely permit review again. A pending decision
makes the task the operator's, and the existing rule that a disposition needs
Queen-owned waiting work then bars her from reviewing it at all. The task leaves
her loop rather than returning to it, which is a stronger outcome than the
control was designed for and is asserted as such.

## Blast radius, stated rather than discovered

Roughly nineteen tasks qualify on the day this ships, so the next review pass on
any of them is refused until somebody escalates, moves or abandons it. That is
the correction, not a defect, and the number should sit near zero afterwards. A
count that climbs again is the signal that something upstream is still parking
work without asking.

Acceptance: a disposition one second under the bound is allowed and one at the
bound is refused; raising a decision from the task clears this refusal; the guard
and the reporting surface use one constant; an exact replay is unaffected. Both
tests ablate — removing the guard fails the boundary case, and ignoring primary
membership fails the exit path.

## Amendment: an answered question still counts as asked

Only a pending decision counted as asked. When the operator answered, nothing
was pending any more, so the task went back to Queen. Her next assessment was
then refused as repetition, even the deferral citing that answer, which records
the result of the escalation. An answer like "keep it parked to <date>" leaves
the task where it is on purpose, so nothing could clear the refusal. On one Hive
two tasks the operator had parked this way were re-reviewed 150 and 230 times,
and every review run finished incomplete.

A decision now also counts as asked when the operator resolved it after the task
last changed state. It may be the task's own decision or a linked one.

- **Until the task moves.** It counts until the task moves again, because a new
  stillness needs its own question.
- **Not in the same second as the move.** That answer is the one that moved
  the task. Work it released and that then sits still has been asked about
  nothing.
- **Both places.** The guard and the stalled-work surface read the same
  condition, and the refusal's text now says that an answer counts.

Acceptance: the cases below are each pinned by a test.

- An operator answer after the last move lets an external condition and a
  deferral citing it be recorded past the bound.
- An answer from before or in the same second as the last move is still refused.
- A task answered about through a link leaves the stalled-work list, and returns
  to it once it moves.
