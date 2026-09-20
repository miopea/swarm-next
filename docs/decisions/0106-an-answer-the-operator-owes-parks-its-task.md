# ADR 0106: An answer the operator owes parks its task

Status: Accepted from the operator interview of 2026-09-20, recorded in
`docs/specs/operator-owed-work-parks-itself.md`.

When a decision resolves with an action the operator marked as theirs to carry
out, the linked task PARKS instead of reverting to its worker.

## Asking made the work visible. Answering made it invisible again.

`NextMoveOwner::derive` reads a task as the operator's only while a decision is
PENDING:

```sql
EXISTS(SELECT 1 FROM decision_requests dr ... AND dr.state = 'pending')
```

So the moment they answer "I'll file it", ownership snaps back to a worker that
cannot act. ADR 0104's own comment named the larger version of this — "the
system cannot see its own operator-gated work... asking is what makes it
visible" — without noticing that answering un-sees it.

Measured 2026-09-20 on task `01a0bd40`. Decision `01a0bd43` resolved 12:39:21Z
with "File it now in Play Console". Every remaining step is operator-only and
verified API-unreachable against the live Play Developer API v3 discovery
document. Six hours later the task was still `active` and firing
`stale_owned_work_attention`. That attention and one other were the Hive's ONLY
two live recovery obligations, so `recovery_coverage` returned `Missing` and the
conductor forced EVERY Queen run to Incomplete — 3 of 3 after ADR 0105 shipped,
against a 73.4% baseline. ADR 0105's fix was correct and completely inert,
because the thing it could have covered was never recorded.

`DecisionDischarge` already models whether an authorised act happened. It is
computed for display, never reaches ownership, and reads `Unknown` whenever no
task names the decision in its own text — which is the common case, and exactly
the cases that go quiet.

## The requester marks; only the operator fires

`swarm_request_decision` gains `operator_actions`, a subset of
`allowed_actions`. The worker authoring the options already knows which ones are
the operator's to perform.

⚠️ THE MARKING IS A HINT AND NEVER AN AUTHORITY. The park fires on the
operator's CHOSEN action, by equality, exactly as the command grant beside it
does. A worker can offer an option; it cannot park its own task and so cannot
silence its own stale-work attention. This needs no extra guard, because it is
the shape `QueenReviewDispositionKind::OperatorDeferral` already demands: one
authenticated task-linked operator source, which the resolved decision IS.

A marked label that matches no offered action is refused at authoring time — a
park that can never fire is worse than none, and creation is the only moment
anyone is looking.

## A receipt, not a flag

The park is a `queen_task_review_receipts` row of kind `operator_deferral`, and
`park` is derived live from it WHILE THE TASK IS BLOCKED. Three things then fall
out without being written:

- `ParkedWork.tsx` already lists `park === "operator_deferral"`, so it appears
  under Needs You / Parked with no new listing code.
- `review_coverage` already covers that receipt kind, so Queen's runs stop being
  forced Incomplete by it.
- Parking updates the task, so `task.updated_at` no longer equals the
  attention's `evidence_revision` and the stale attention drops out of
  `LIVE_ATTENTION_SOURCE` by itself. Nothing has to delete it.

The assignment is KEPT. The task is resting, not reassigned.

## Only the operator lifts it, and lifting DELETES the receipt

Leaving `blocked` already hides the park by derivation — but the row would
survive, and the next unrelated block on that task would read as an operator
park and stay covered in review. A stale deferral quietly covering a real
blocker is worse than the stall this removes, so discharging deletes the
evidence. The task returns to `ready` with its worker.

Rejected: auto-lifting on `DecisionDischarge::Discharged`, because discharge
reads `Unknown` without a text-naming link and would silently never lift in the
cases that go quiet. Rejected: a time-boxed re-raise, because it re-raises
whether or not anything changed, which is the cost ADR 0104 spent two fixes
removing.

## ⚠️ FREE TEXT PARKS NOTHING, and that gap is named rather than papered over

When the operator answers in their own words, `answered_how` is
`in_their_own_words` and `resolution_action` is a placeholder, so no marked
option matches. Swarm genuinely cannot tell whether they took the action, and
parking on a guess would hide work. That path behaves exactly as before: no
regression, and no improvement. It is a real hole and it is recorded as one.

**The cost, named rather than discovered.** A worker that forgets to mark an
option gets today's silent revert. The schema default is `[]`, so every existing
record and every older client parks nothing — which is the behaviour they
already had, and means this cannot regress anything, but also means adoption is
per-request rather than automatic.

## Not solved here

Work that is operator-gated with NO decision ever filed stays invisible. That is
the older and larger problem ADR 0104's comment names, and nothing here touches
it: this only parks work the operator explicitly took on.
