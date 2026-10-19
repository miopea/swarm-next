# 0119 — A wait says who must act and until when

- Status: Accepted
- Date: 2026-10-18
- Decided by: the operator, on 2026-10-18. Asked "Accept ADR 0119 (typed waits)
  with those three behaviours?", they chose "Accept as drafted". The three
  behaviours are in Decisions 4 and 5.
- Asked for by: the operator, on 2026-10-18, after the stuck-work analysis:
  "I show 15 tasks that are open and it is hard to get them worked on, unclear
  what needs to happen to unblock them, etc. This is a weakness of the system and
  a limitation when we move to apiary level tasking". They chose all three of
  typed waits, freeing the Active slot, and Apiary tasks carrying waits.
- Task 01a15192-a9fd-7a92-9745-8028952cf2b5.
- Builds on: ADR 0047 (one Active task per worker), 0067 (task-aware message
  holds), 0076 (prerequisites), 0081 (block reassessment), 0084 (decision
  blockers), 0106 (operator-owed parks), 0111 (federated task sync).

## Context

Blocked work on the Keeper used to clear in hours, and it now takes days:

- The typical blocked spell went from 0.2 hours in late August to 4.3 days in
  the week of 2026-10-12. The slowest tenth went from 26 hours to 28 days.
- Of the 15 open tasks, four waited on the operator, two on an outside party,
  four on another task and three on a date. Needs you was empty.
- Queen re-read the outside waits every 15 minutes. She wrote the same note on
  the Spectrum outage task 213 times in one day.

Three gaps in the model cause this.

1. **A worker cannot block honestly.** `refuse_a_block_nobody_owns` accepts a
   block only with a prerequisite or a decision. "Waiting on the operator to sign
   in", "waiting on Spectrum" and "come back on November 16" are all refused. So
   workers leave waiting work Active.
2. **Active work that is waiting holds everything behind it.** One Active task
   per worker (ADR 0047) holds that worker's Ready briefings and its messages
   about other tasks (ADR 0067). RCG Networks and BFG Watchfaces were both
   stuck this way. Blocked releases all of it, but see gap 1.
3. **Nothing comes back by itself.** When a prerequisite finishes, a decision
   is answered or a date passes, the task's next move goes to Queen. She judges
   it and moves it to Ready. Lifting an operator-owed park writes Ready without
   re-arming the briefing, so the worker may never be told.

Apiary tasks make all three worse. `apiary_tasks` carries a bare state, so a
member sees "blocked" with no reason, no owner and no date.

## Decision

### 1. Every Blocked task names one wait

A wait is one of five kinds. Two already exist and stay where they are. Three
are new.

| Kind | Who must act | Recorded as | Clears when |
|---|---|---|---|
| `task` | another task | prerequisite (ADR 0076) | the prerequisite completes |
| `decision` | the operator answers | decision link (ADR 0084) | the decision is resolved |
| `operator_action` | the operator does something | new wait row | the operator marks it done |
| `outside` | a named outside party | new wait row, with a recheck time | the recheck time arrives |
| `date` | nobody until then | new wait row, with a resume time | the time arrives |

- A wait row is anchored to the activity entry that moved the task into
  Blocked, as `task_block_reassessments` is (ADR 0081). Leaving Blocked ends it
  without a delete.
- It holds the kind, what must happen in words (at most 500 bytes), the outside
  party's name, and the recheck or resume time. That time must fall between one
  hour and 30 days ahead.
- `swarm_transition_task` to Blocked takes a `wait` argument. The assigned
  worker, Queen and the operator can all name any kind. The block guard accepts
  the three new kinds.
- A block with no wait, which only the operator's own path allows, is
  **untyped**. It keeps today's behaviour, and Queen owns it.

### 2. Who owns the next move

`next_move_owner` is still derived, not stored:
- `operator_action` and `decision` waits derive **Operator**.
- `outside` and `date` waits derive **Blocked** until their time, then
  **Worker**.
- `task` waits derive **Blocked** until the prerequisite completes, then
  **Worker**.

Queen is the owner only for untyped blocks and unassigned work.

Consequences for the attention machinery:
- Queen's wake count (ADR 0037) leaves out every typed wait that has not
  cleared, as it already does for date holds and open prerequisites.
- The four-hour unattended-block attention and the twelve-hour escalation skip
  typed waits that have not cleared. They exist to catch work nobody owns, and a
  typed wait names its owner.

### 3. An operator action is a Needs you item

An `operator_action` wait shows in Needs you, and counts in its badge, as:
"*RCG Networks is waiting on you:* Sign in to the Entra portal and approve M2."
- It has two buttons, **Done** and **Open the task**.
- Done clears the wait and records the operator as the actor.
- ADR 0106's operator-owed park becomes this wait kind, and its "Parked" list
  folds into Needs you.

### 4. A cleared wait comes back by itself

When a wait clears, Swarm moves the task from Blocked to Ready for its
assigned worker. Swarm acts as the system actor, records a note saying what
cleared it, and re-arms the briefing.
- The briefing waits behind the worker's current Active task under the existing
  rules, so nothing interrupts current work.
- An unassigned task goes to Ready unassigned, and Queen routes it.
- For an `outside` wait, the briefing says: "Recheck: is <what> done yet?"
  - The worker checks with its own tools. If the party has not acted, it blocks
    again with a new recheck time, and each recheck is a line of history.
  - The one-hour minimum bounds the loop. Nothing re-reads it in between.
- Clearing is evaluated in the same transaction as the event that clears it:
  - the prerequisite completing;
  - the decision resolving;
  - Done being pressed.
- Time-based clearing is evaluated by the coordinator pass that already runs. It
  is idempotent: a wait clears once, and a task that has left Blocked is not
  moved.
- The ADR 0106 park-lift gap closes, because lifting a park is a wait clearing.

### 5. The Active slot is freed by Blocked, not by a new state

No "Active but waiting" state is added. Blocked already frees the slot, the
briefings and the held messages, so the fix is to make Blocked reachable
(Decision 1) and reversible (Decision 4).
- Asking the operator a question on your own Active task
  (`swarm_request_decision`) blocks that task on the decision.
- To carry on working, the worker passes `keep_working: true`, and the task
  stays Active as today.
- An Active task that gains an open prerequisite is blocked on it. ADR 0076 says
  this "does not rewind Active". This ADR changes that, because an Active task
  that cannot finish only holds the slot.
- Reassigning an Active task to a worker that already holds another Active task
  is refused, closing the gap where `assign_task_to_worker_as` made two.

### 6. Apiary tasks carry the wait

- `ApiaryTask` gains `wait: Option<ApiaryTaskWait>`, with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`, as
  `closure_evidence` does. It holds:
  - the kind, as a string;
  - what must happen;
  - the party;
  - the recheck time;
  - for an operator wait, the Hive whose operator must act.

  The kind is a string, not a closed enum, so an unknown kind can never fail a
  page.
- The Keeper stores it in a new `wait_json` column, added with
  `add_column_once`. Members store snapshots and need no change.
- It travels on the existing Transition command, as an optional field sent with
  a Blocked target. A changed wait on a task that is already Blocked is a
  Blocked-to-Blocked transition, which an older Keeper refuses with a clean
  receipt.
- The home member's local task holds the typed wait. When it clears, the local
  resume mirrors to the Apiary task through the existing lifecycle intents.
- What does not change:
  - the feed schema version, the federation protocol version and `TaskState`;
  - the terminal-host protocol.
- A member on 1.17.4 or older sees Blocked as it does today, without the
  detail. The Keeper infers which members show waits from the release they
  report, as it does for `local_choice_since`.
- `insert_apiary_task_for_hive` writes `'ready'` whatever the task's state.
  That is fixed here, because a relocated Blocked task would otherwise lose its
  wait.

### 7. The work already on the board

A one-time migration types every open block it can:
- an open prerequisite becomes `task`;
- a pending linked decision becomes `decision`;
- a future `blocked_until` becomes `date`, resuming at that time;
- an `external_condition` park becomes `outside`, with a recheck time one day
  out;
- an `operator_deferral` park becomes `operator_action`.

Anything left is untyped, and listed for Queen to type with the worker.

## Implementation notes, part 1 (2026-10-19)

The behaviour is as decided. Three details of how it is recorded differ from
the wording above, each for a reason found while building it.

- **A wait is named in the Blocked note, not by a `wait` argument.** An MCP
  client fetches its tool list once when it connects. A new argument would be
  invisible to every worker already running, and those are the workers holding
  today's waiting work. The `Blocked until:` date hold was built the same way
  for the same reason. The markers are `Waiting on:`, `Needs:` and `Recheck:`.
  The tool description teaches them to new sessions. The refusal of a block
  that names nothing teaches them to every session, because a refusal is a tool
  result rather than a schema.
- **A block waiting on a task or a decision records a row too.** Links outlive
  the block they were made for. A decision answered in September is still linked
  when the task is blocked again in October for a reason nobody recorded, and
  bringing that task back because the old decision is answered would be wrong.
  So the block records a `task` or `decision` wait when it is made with a live
  gate, or when a gate is linked to a block that has no wait. Only a wait
  recorded with the current block can bring a task back.
- **Clearing is evaluated by the coordination pass**, not inside the
  transaction of the event that clears it. The pass runs every 30 seconds, and
  straight after a decision is answered or Done is pressed. Done also brings
  the task back at once. Persistence does no logging, and the pass reports a
  task that cleared but could not move.

ADR 0106's park lift is removed. A park now records an operator-action wait,
and Done replaces "I've done this".

## Alternatives considered

- **An "Active but waiting" state, or a slot rule that ignores waiting Active
  work.** It frees the slot without touching Blocked. But it puts two tasks'
  conversations in one terminal at the same moment, adds a state every surface
  must learn, and still leaves nothing to bring the work back. Rejected for
  Decision 5.
- **Queen re-reads outside waits, but less often.** It reduces cost, but it
  keeps the judgement with the party who has no tools to check the condition.
  Rejected: the worker rechecks, on a date the worker chose.
- **A new Apiary command kind for waits.** It is cleaner on the wire, but an
  older Keeper refuses an unknown kind with an error that stalls the member's
  whole task step. Rejected for Decision 6.
- **Leave clearing to Queen, as today.** This is the gap the analysis measured.

## Consequences

- Every open task can say who must act and when it comes back, on the board,
  in Needs you and across Hives.
- Queen stops spending runs on waits she cannot move. Her remaining work is
  routing, review and untyped blocks.
- Workers block more, and come back without being chased. A worker may now
  hold several Blocked tasks, which is correct.
- Automatic moves from Blocked to Ready are a new system action. Each one is
  recorded with what cleared it, and each is tested.
- Not covered: a wait that clears on something Swarm cannot observe. Those stay
  `outside` and rely on the worker's recheck.

## Related decisions

0037, 0047, 0067, 0076, 0081, 0084, 0106, 0111, 0113.
