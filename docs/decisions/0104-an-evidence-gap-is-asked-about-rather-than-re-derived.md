# ADR 0104: An evidence gap is asked about, not re-derived

Status: Accepted implementation direction for the operator's retarget of
2026-09-18, filed as task `01a0b780-08cd`.

Recording a further `insufficient_evidence` disposition is REFUSED once the same
missing fact has been re-derived `MAX_UNCHANGED_REVIEW_PASSES` times with nobody
asked for it.

## The loop cannot be exited by reviewing, which is why it never ended

This is not a habit, and it is not Queen being slow.

1. `review_coverage` counts every live task whose `next_move_owner` is Queen as
   an obligation.
2. Only `operator_deferral` and `external_condition` receipts can cover one. The
   query never selects `insufficient_evidence`, matching its design contract that
   every recurrence needs a genuinely fresh check.
3. `queen_conductor.rs:722` forces a run's outcome to `Incomplete` while any
   obligation is uncovered.
4. So the work is re-reviewed, judged identically, and is still uncovered.

**An insufficient-evidence finding cannot discharge the obligation that produced
it.** There is no exit through the review path at all. The only two exits leave
her queue rather than satisfy it: the task moves, or a decision is raised — and
`NextMoveOwner::derive` returns `Operator` whenever one is pending. That is why
"ask sooner" is not advice here; it is the sole terminating condition that
exists.

## Measured before building, on 2026-09-19

| kind | receipts | accumulated passes | worst |
| --- | --- | --- | --- |
| `insufficient_evidence` | 81 | 1149 | 90 |
| `external_condition` | 8 | 177 | 86 |
| `operator_deferral` | 28 | 115 | 29 |

⚠️ THE HEADLINE OVERSTATES THE ONGOING COST BY ABOUT 2x, and the ticket's
acceptance should be read against the live figure or it will appear to improve
merely as tasks close. Of those 1149 passes, **616 across 50 receipts sit on
tasks already completed or abandoned** — frozen history that cannot recur. The
live forward-looking cost is **31 receipts and 533 passes**, and 20 receipts at
20+ repeats carry 946 of the total. This is not 81 tasks each costing a little.

Confirmed still live rather than inferred: over four minutes one receipt rose
41 → 42 and the live total 533 → 535.

## Why neither existing terminator reaches it

**ADR 0102's guard does not**, and this was checked first precisely because the
author of that guard also wrote this one. It refuses work that has not MOVED in
three days. Of the twenty worst offenders here, **exactly one** was stalled three
days; the rest had moved within two. The bound is wrong for this population by
roughly an order of magnitude. Repetition here is a COUNT, not a duration — which
is why the two refusals are separate error variants with separate messages rather
than one message covering both causes.

**The rotation's cap does not either.** `MAX_UNCHANGED_REVIEW_PASSES = 6` gates
`queen_review_queue_snapshot`, but the queue is what is OFFERED while coverage is
what makes a run Incomplete. Different paths, so capping the rotation never
touched the obligation forcing the re-review.

## The control

`EvidenceNeedsAskingNotRechecking { times }`, raised when the disposition is
`InsufficientEvidence`, the task's existing receipt has reached the bound, and no
decision is pending — primary (`decision_requests.task_id`) or linked. Both forms
count; reading only the link table would refuse work whose question was already
pending, the worst version of this control.

It sits beside its sibling, after the replay check, so an idempotent replay still
returns early and only a NEW assessment is refused. The bound is the constant
that already governs the rotation rather than a second number, so the surface
that skips repetition and the guard that refuses it cannot disagree.

## Blast radius, stated rather than discovered

**15 live receipts carrying 425 of the 533 live passes — 80%** — are refused the
moment this ships, and each needs somebody to raise the question, move the task
or abandon it. Two live receipts already have a decision pending and are
unaffected; they have already taken the exit. The number should sit near zero
afterwards, and a count that climbs again means something upstream is still
parking work without asking.

## Acceptance, and one weakness stated plainly

A disposition one pass under the bound is allowed and one at the bound is
refused; raising a decision clears this refusal.

⚠️ The two tests are NOT fully complementary and the commit says so. Removing the
pending-decision check isolates cleanly — only the asking test fails. Removing
the threshold fails the boundary test at its assertion AND fails the asking test
in its SETUP, because both share a helper that assumes ordinary passes succeed.
That is weaker than isolation in one direction, and it is recorded rather than
dressed up.
