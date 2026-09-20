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

## Amended, 2026-09-19: the bound now also reaches the coverage path

⚠️ THIS ADR'S SCOPE DECISION IS REVERSED ON ONE POINT, and the reversal is the
operator's, not a later reading of the original. Decision
`01a0bc10-821a-73a3-b9f7-5fbc219d24d6`, answered `chose_an_offered_action`:
**"Apply the existing bound to the coverage path."**

What this ADR got right and still stands: the refusal it built genuinely works.
Since `3fb6084e` deployed at 02:32 on 2026-09-19, **zero `insufficient_evidence`
receipts have incremented**, and the worst is frozen at 90.

What it did not cover, deliberately, was `operator_deferral` and
`external_condition` — the two kinds `review_coverage` actually selects. The
reasoning above is still the right reasoning: those two are legitimate coverage,
and refusing them would stop Queen looking at waits that might have moved.

**The measurement that changed the answer.** Receipt `01a0588f-5a6a` was created
2026-09-19 12:11 — after the 09-18 rotation cap AND after this ADR's guard went
live — and reached `times_seen` 12, twice the bound, by 15:52. Twelve passes in
3.7 hours, identical condition text each pass, zero state changes. Live cost at
that moment: 216 passes beyond the bound, 103 of them on these two kinds.

**Why neither existing terminator reached it**, which is the same shape this ADR
already documented once. `times_seen` gates what is OFFERED
(`queen_review_queue_snapshot`) and what is REFUSED (this ADR). `review_coverage`
does neither: it matches `accepted_revision` against `task_review_evidence`, a
hash that includes `task_messages` and `task_message_deliveries`. So a message,
or a delivery-state update, uncovers the receipt and forces a re-review that can
only reach the same conclusion. This ADR's own line — "capping the rotation never
touched the obligation forcing the re-review" — turned out to be true of its own
guard too.

**No third number and no third anchor.** `times_seen` resets to 1 whenever the
task moves state and otherwise increments, so reaching the bound ALREADY means
"re-derived this many times while standing still". The change accepts the current
evidence revision at the bound, which says the wait is covered until the task
moves.

**The cost, named rather than discovered.** Queen stops re-checking an external
wait whose evidence genuinely moved, once it has been re-derived six times
without the task moving. A wait that becomes actionable on its seventh pass now
waits for a state change. That is a real loss and it is what the operator chose
with it stated; the 2026-09-18 answer about showing a hold with Last checked was
NOT read as authorising it, which is why a separate decision was raised.

⚠️ NOTHING BECOMES INVISIBLE, and the test that matters most asserts it. Skipped
work stays in `reviews_repeating`, and a real state change resets the count and
returns the work. Ablation is clean: removing the clause fails exactly one test,
`a_wait_at_the_bound_stops_returning_even_when_its_evidence_moves`, while the
under-bound and returns-on-movement tests still pass — they assert behaviour that
exists without it.

⚠️ THE VISIBILITY HALF IS NOT BUILT HERE. Stopping the re-raise removes the only
thing currently keeping a park in front of anyone. The operator raised this in the
same breath: parked work should have its own surface beside Needs You and
Activity. Measured the same day: of 15 blocked tasks, 4 are operator parks, 3 are
external waits, 6 wait on another task. Until that surface exists, this change
makes parks quieter without making them findable.

⚠️ THE SIBLING GATE CARRIED THE SAME DEFECT AND WAS NOT FIXED HERE. Review
coverage stopped depending on the run on 2026-09-18; `recovery_coverage` kept its
`run_id` clause until 2026-09-20, so 54 of the 75 runs after this ADR's change
were still forced Incomplete by the `||` beside it. ADR 0105 removes it — and
records why the bound above must NOT be mirrored onto that path, where a count
rising while nothing moves is the stall itself rather than a settled wait.
