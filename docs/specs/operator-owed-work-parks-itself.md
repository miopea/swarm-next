# Operator-owed work parks itself

Status: Accepted from the operator interview of 2026-09-20. Implements the
visibility half ADR 0104 named and did not build.

When a decision resolves with an action the operator has taken on themselves,
the linked task PARKS instead of reverting to its worker.

## The hole, measured

`NextMoveOwner::derive` reads a task as the operator's only while a decision is
PENDING:

```sql
EXISTS(SELECT 1 FROM decision_requests dr ... AND dr.state = 'pending')
```

So asking makes work visible as theirs and ANSWERING MAKES IT INVISIBLE AGAIN.
The moment the operator says "I'll file it", ownership snaps back to a worker
that cannot act.

Observed 2026-09-20 on task `01a0bd40`: decision `01a0bd43` resolved 12:39:21Z
with "File it now in Play Console". Every remaining step is operator-only and
verified API-unreachable. Six hours later the task was still `active`, firing
`stale_owned_work_attention` continuously. That attention plus one other were the
Hive's ONLY two live recovery obligations, so `recovery_coverage` returned
`Missing` and the conductor forced EVERY Queen run to Incomplete — 3 of 3 after
the recovery fix shipped, against a 73.4% baseline.

`DecisionDischarge` already models whether an authorised act happened
(`Discharged` / `Outstanding` / `Unknown`, built after three acts silently never
happened on 2026-09-03). It is computed for display and never reaches ownership,
and it reads `Unknown` whenever no task names the decision in its own text —
which is the common case, and precisely the cases that go quiet.

## What is built

### 1. The requester marks which options mean "the operator acts"

`swarm_request_decision` gains `operator_actions`: a subset of `allowed_actions`.
The worker authoring the options already knows which ones are theirs to carry
out — it wrote "File it now in Play Console" — so the knowledge is recorded
where it exists rather than asked for later.

### 2. The park fires only on the operator's own choice

⚠️ THE MARKING IS A HINT, NEVER AN AUTHORITY. A worker can offer an option; only
the operator picking it parks anything. A worker cannot park its own task and so
cannot silence its own stale-work attention. This needs no new guard: it is the
same shape `QueenReviewDispositionKind::OperatorDeferral` already requires, which
is exactly one authenticated task-linked operator source, and the resolved
decision id IS that source.

### 3. Free text does not park

If the operator answers in their own words, `answered_how` is
`in_their_own_words` and `resolution_action` is a placeholder, so no marked option
matches. Swarm then genuinely cannot tell whether the operator took the action,
and parking on a guess would hide work. That path behaves exactly as today: no
regression, and no improvement either. Named so the gap is known rather than
discovered.

### 4. Parking keeps the worker

The task moves to `blocked` and KEEPS its assignment. It is resting, not
reassigned; it returns to the same worker when the park lifts. Active work is
parked too — exempting it would exempt `01a0bd40`, the case that caused this.

### 5. The park is a receipt, not a flag

`park` is derived live in the task projection from a
`queen_task_review_receipts` row of kind `operator_deferral` WHILE THE TASK IS
BLOCKED. Writing a receipt rather than setting a column is what makes a park
leave the list the moment its task moves, and reuses the machinery already
keeping four parks quiet and covered.

Consequences that fall out for free:
- `ParkedWork.tsx` already filters `task.park === "operator_deferral"`, so the
  task appears in Needs You / Parked with no UI change to list it.
- `review_coverage` already covers an `operator_deferral` receipt, so Queen's
  runs stop being forced Incomplete by it.
- Parking updates the task, so `task.updated_at` no longer equals the
  attention's `evidence_revision` and the stale attention drops out of
  `LIVE_ATTENTION_SOURCE` on its own. Nothing has to delete it.

### 6. Only the operator lifts it

A control on the Parked card returns the task to `ready`, keeping its assignment.
The operator is the only one who knows they filed it in Console, so nobody else
can honestly clear it. Leaving `blocked` clears `park` by derivation.

Rejected: auto-lifting on `DecisionDischarge::Discharged`, because discharge
reads `Unknown` without a text-naming link and would silently never lift in the
cases that go quiet today. Rejected: a time-boxed re-raise, because it re-raises
whether or not anything changed, which is the re-derivation cost ADR 0104 spent
two fixes removing.

## Acceptance

- A resolved decision whose chosen action is marked parks its linked task, which
  then appears under Needs You / Parked and is covered in Queen's next run.
- A worker cannot park a task by marking options; only the operator's choice does.
- A free-text answer parks nothing.
- The parked task keeps its assigned worker and returns to `ready` when lifted.
- A decision with no linked task parks nothing and does not fail.

## Not solved here

Free-text answers, per section 3. And this parks work the operator TOOK ON; work
that is operator-gated with no decision ever filed stays invisible, which is the
older and larger problem ADR 0104's comment names.
