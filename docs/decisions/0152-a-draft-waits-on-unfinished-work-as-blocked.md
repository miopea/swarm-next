# 0152 — A draft waits on unfinished work as Blocked

- Status: Accepted
- Date: 2026-11-04
- Decided by: the operator (2026-11-04). Their words: you should be able to set
  a task to blocked if it has a prerequisite, since that is the whole purpose
  of one. Asked what should happen when the work a blocked draft waits on
  finishes, they chose "Back to Queen": it stays Blocked and Queen moves it.
  They were first asked with that option described as what every Blocked task
  does. That was wrong: a Blocked task that names what it waits on comes back
  to Ready by itself (ADR 0119). Asked again with the difference stated, and
  with "Ready by itself" offered, they chose "Back to Queen" again.
- Amends: ADR 0135, which said a draft cannot move to Blocked and told Queen
  to leave a waiting draft in Draft. ADR 0076 (explicit task prerequisites),
  on which moves a draft with a prerequisite may make. ADR 0119 (typed waits),
  on what clears a wait that was entered from Draft.

## Context

On 2026-11-04 the operator's Hive had three open drafts, all routed to a
worker. Two of them waited behind unfinished work: one behind a blocked task,
one behind work that was finished and waiting on a release. Queen tried to
move them out of Draft and both moves were refused:

- Blocked: "task cannot move from draft to blocked". The lifecycle had no
  such move.
- Ready: "This task still has unresolved prerequisites; Queen must reconcile
  them before resuming work". The store refuses Ready while any prerequisite
  is unsatisfied.

ADR 0135 already kept such a draft out of Queen's review coverage and
labelled it as waiting. It still sat in Draft, the column for work nobody has
decided about, and the refusals told Queen neither what she could do nor why.

## Decision

1. **Draft may move to Blocked, behind work that can still finish.** The
   store admits the move only when, after any link named on the same call is
   recorded, the draft has a prerequisite that is not completed, abandoned or
   removed, or a pending operator decision linked to it. Otherwise it refuses,
   naming `prerequisite_id` and `decision_id`. The check is in the store's
   transition, so every move made through it is held to the rule: Queen, a
   worker, the operator's board and the coordinator.
2. **Ready on a waiting draft names Blocked.** While a prerequisite can still
   finish, Ready on a draft is refused with a message saying to move it to
   blocked. A draft whose prerequisites are all abandoned or removed is
   refused as before, and the Blocked refusal now says to remove those first.
3. **When the work finishes, the task goes back to Queen.** It stays Blocked,
   its next move becomes Queen's even with a worker assigned, and it is listed
   among the blocked tasks whose prerequisites finished. She moves it to Ready,
   and its worker is briefed, or she abandons it. This differs from other
   Blocked work on purpose: a draft was never made ready to start, so it does
   not start without another look. The same holds when it waited on a decision
   and the operator answers: no "Work left parked by an answer" card asks them
   to release it. The block keeps the wait it names, so the board still says
   what it waits on. Whether the block was entered from Draft is read from the
   activity row that entered it; nothing new is stored, and the wait carries
   the flag only when it is true, so no other task serializes differently.
4. **Triaging a draft stays Queen's.** A worker cannot move a draft routed to
   it to Blocked. It is told to name the wait to Queen.
5. **A draft's block cannot leave it stuck behind finished work.** A link
   named on the call is written before the store decides the move. On a
   draft, a link to a completed or abandoned task, or to a decision that is
   not pending, is refused before anything is written.
6. **Naming a link the task already has is not a change.** Queen blocks a
   waiting draft by naming the prerequisite she already gave it. The store
   refuses a second add of the same link with a different reason as a
   conflict. Naming an existing prerequisite or decision link now records
   nothing and keeps the original reason. This applies to every task.
7. **Queen's guidance offers the move.** Her review brief, her list of the
   ways a draft leaves her list, the promote tool's description and its
   refusal say a draft that waits on unfinished work moves to blocked, naming
   that work.

## What this does not change

- A draft left in Draft behind unfinished work still waits, out of Queen's
  review coverage, and still reads as waiting on its card (ADR 0135).
- Other Blocked work comes back exactly as before (ADR 0119). That includes
  a draft the operator parks through their own action answer, and a draft a
  Jira status moves straight to Blocked. Neither is a lifecycle move from
  Draft: the park records no previous state, and a Jira move is recorded as
  Jira's. The parked draft comes back to Ready when they press Done, and
  answering a decision the Jira-blocked one waits on still asks the operator
  to release it.
- On Queen's and a worker's path, Blocked still needs an assigned worker, so
  an unrouted draft is refused with "Assign a worker first". The operator's
  routes never asked for one, and still do not.
- The Apiary's shared ledger has no prerequisites, so a shared draft cannot
  be blocked there, and the mirror never proposes it.
- The board's drag targets for a draft are unchanged: Ready only. The API
  admits Blocked for a draft that waits on something, but the board would
  have to offer that drop only for those drafts.

## Consequences

- Queen's two refused moves become one that succeeds. The draft leaves
  Draft, the board reads Blocked, and the prerequisite stays on the card.
- A link named on a refused block can still stay recorded, as on any refused
  block. On a draft it is a link to work that could still finish when it was
  checked, left when the block is refused for another reason: no assigned
  worker, a wait in the note named wrongly, or a decision named as an operator
  action. A prerequisite left this way holds the draft in Draft as ADR 0135
  describes, and Ready on it names Blocked. A decision left this way makes the
  draft wait on the operator's answer.
- The check and the move are separate steps, so work abandoned between them
  can still leave a draft linked to it. The Blocked refusal then says to
  remove that prerequisite.

## Validation

- `task_transitions_are_explicit` (domain): Draft → Blocked is a move; Draft →
  Active, Draft → Review and Blocked → Draft are not.
- `a_typed_wait_names_its_owner_and_never_leaves_it_to_queen` (domain): a
  cleared task or decision wait entered from Draft is Queen's with a worker
  assigned, and waits like any other until it clears.
- `only_a_former_draft_s_wait_carries_the_flag` (domain): any other wait
  serializes without it, and a snapshot without it reads as not.
- `a_draft_behind_unfinished_work_blocks_keeps_its_link_and_returns_to_queen`
  (persistence): the draft has a worker and its prerequisite is itself
  blocked. Ready is refused naming Blocked. Blocked keeps the link and its
  reason, and the wait is a task wait marked as entered from Draft. When the
  prerequisite completes, the resume pass brings back an ordinary task
  blocked behind the same work and leaves the former draft. That draft stays
  Blocked and assigned, its next move is Queen's, it is listed among blocked
  tasks whose prerequisites finished, and Ready then succeeds.
- `a_draft_blocks_only_behind_work_that_can_still_finish` (persistence):
  refused with no prerequisite, with only a completed one, and with only an
  abandoned one, naming `prerequisite_id` and `decision_id`.
- `a_draft_waiting_on_a_pending_decision_may_block` (persistence): when the
  operator answers, the resume pass leaves it Blocked and it is Queen's.
- `the_shared_ledger_does_not_block_a_draft` (persistence).
- `a_draft_jira_blocks_is_still_released_by_the_operator` (persistence): a
  draft a Jira status moved to Blocked is still offered back to the operator
  when the decision it waits on is answered.
- `queen_blocks_a_draft_behind_its_prerequisite_and_its_worker_cannot`,
  `queen_blocks_a_draft_behind_a_pending_decision_it_already_names` and
  `a_draft_block_naming_finished_work_writes_nothing` (application). Queen
  naming a link the draft already has keeps it, and the routed worker is
  refused. Naming an abandoned task is refused, nothing is written, and the
  draft can still be made ready.
- `answering_a_former_draft_s_decision_hands_it_to_queen` (application): the
  operator answers through their own route; no release card is raised, the
  resume pass leaves the task, and its next move is Queen's.
- `queen_blocks_a_routed_draft_behind_its_blocked_prerequisite` (MCP): Queen's
  two calls through `swarm_transition_task` on a routed draft whose
  prerequisite is blocked. Ready is refused naming Blocked. Blocked with
  `prerequisite_id` succeeds, keeping the assignee, the link and its reason.
- The brief, uncovered-draft hint and promote refusal tests now require the
  blocked move.
- On a disposable test Hive, through operator routes:
  - Ready on a routed draft behind a blocked task was refused naming Blocked, and Blocked kept its worker and link.
  - The prerequisite was completed with a recorded deployment, and then one coordination pass ran.
  - An ordinary task blocked behind the same work came back to Ready.
  - The former draft stayed Blocked, its next move Queen's, and Ready on it then succeeded.
- Not yet observed on a live Queen run.
