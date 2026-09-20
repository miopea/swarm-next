# ADR 0105: A recovery verdict outlives the run that reached it

Status: Accepted implementation direction for the operator's instruction of
2026-09-20 — improve Swarm so tasks stop getting stuck — taken with full board
access granted for that purpose.

A `queen_recovery_receipts` row covers its obligation for as long as the task
evidence and the terminal output are unchanged, rather than only inside the run
that wrote it. The repetition count added alongside it is REPORTED and never
certifies coverage.

## The review gate was fixed and the runs stayed Incomplete

ADR 0104 and `07c24d3d` fixed review coverage. They worked, and the measurement
is unambiguous:

| | runs | Incomplete | |
| --- | --- | --- | --- |
| before `07c24d3d` | 2109 | 2108 | Queen's review had never completed |
| after, to 2026-09-20 17:19Z | 75 | 54 | 21 of the 22 completions this Hive has EVER recorded |

`finish_queen_automation_run_with_recovery` downgrades on either of two gates:

```rust
if !matches!(review_coverage(...), Covered { .. })
   || !matches!(recovery_coverage(...), Covered { .. })
```

Queen requested `completed` on all 75 runs. The review half was healthy — her
three obligations all carried `operator_deferral` receipts at or past the bound,
and no `external_condition` or `operator_deferral` receipt has incremented since
the fix landed. The `||` beside it was doing the downgrading.

## The same defect, unfixed, on the sibling path

`recovery_coverage` looked its receipt up with `AND run_id=?2`. That is verbatim
the clause removed from `review_coverage` on 2026-09-18, whose comment describes
the consequence exactly: a receipt counted only inside the run that recorded it,
so the next run found the obligation uncovered.

Measured 2026-09-20: across **2184 runs the Hive had recorded four recovery
receipts, each under a different `run_id`**. Not one could ever cover anything.
Three live recovery obligations stood against a run firing roughly every 17
minutes, so Queen re-assessed the same three workers every run or finished
Incomplete.

## Why dropping the run is safe here

The lookup still matches `accepted_revision` and `terminal_revision`. The verdict
holds only while the task evidence AND the terminal output are byte-identical; a
worker that writes one line, or a task that gains any activity, uncovers
immediately and returns to Queen. The run was never what made the judgment
current — those two revisions are.

## ⚠️ THE BOUND IS NOT MIRRORED, AND THAT IS DELIBERATE

The operator's first instruction was to mirror ADR 0104 in full, including
letting `times_seen >= MAX_UNCHANGED_REVIEW_PASSES` accept the current revision.
That was retracted once the inversion was shown, and the reason is recorded here
because the symmetry is inviting and wrong.

`times_seen` rises precisely while a task does NOT move. On the review path the
obligations are parks — a deliberate wait, which is SUPPOSED to sit still, so
covering it at the bound costs only a re-check. On this path the obligations are
`stale_owned_work_attention` and `assigned_ready_work_not_started_attention` — a
worker that is not moving. There, a count rising while nothing changes IS the
failure. Accepting the current revision at the bound would have made a genuinely
stuck worker go quiet after six passes: the exact outcome this work exists to
prevent, shipped under the name of a fix for it.

So the count is recorded and served as `recoveries_repeating`, beside
`reviews_repeating`, and it never becomes coverage.

**The cost, named rather than discovered.** A worker whose task evidence and
terminal output are both frozen now produces one assessment instead of one per
run. If Queen's verdict was wrong, that wrong verdict stands until something
moves, where previously the next run re-derived it. The repetition report is what
makes that case findable; nothing else re-raises it.

⚠️ NOT PROVEN HERE. `observations_complete` is the other input to
`recovery_coverage`, and it comes from an async terminal read this change does
not touch. If it returns false the run still finishes Incomplete, for a reason
that has nothing to do with receipts. Whether that happens, and how often, is
unmeasured — the 21 completed runs since `07c24d3d` say it is not constant, and
that is all they say. A residual Incomplete rate after this ships is the evidence
to chase there, not here.
