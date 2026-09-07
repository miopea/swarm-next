# ADR 0084: One operator decision can block several tasks

Status: implementation direction under the approved grouped-attention and
structural-blocker maturity scope. Not deployed or accepted end to end.

## Evidence and outcome

September 7 live recovery assessments cite pending decision 01a07c72 for several
Member Services, Platform and Admin tasks. Its durable task_id names only one
task. Other tasks remain worker-owned with the same blocker recorded in prose.
Current recovery and task projections query only that original task_id.

Queen or the operator may explicitly link additional local, unfinished tasks to
a pending decision with a concise reason. Do not infer links from prose, titles,
matching email addresses, or a historical Queen assessment. Keep the original
decision task and requesting worker unchanged. The relation means "this task
depends on this answer", not "this task receives the permission requested".

## Invariants

- One decision remains one Needs You question. Show linked tasks and reasons in
  its detail and expose the same authoritative relation in Queues/task evidence.
- Linking never modifies the question, allowed command, response, execution
  assignee, task lifecycle or provider conversation. Command permission delivery
  remains scoped to the original request. Resolution is not blanket approval.
- Reject new links to resolved/withdrawn decisions or completed/abandoned tasks.
  Exact accepted retries remain recoverable without creating another audit event.
  Removing a relation is explicit and audited; it does not withdraw the decision.
  Mutations require the current task review evidence revision. Exact retries are
  no-ops; an old add cannot recreate a removed link, and an old remove cannot
  delete a subsequently re-added link.
- At most 32 additional task links per decision, 32 per task and 4096 per Hive.
  Refuse capacity rather than silently removing current blockers. Historical
  decision responses and task audit remain intact when obsolete links are removed.
- Persistence owns same-Hive existence checks, Queen/operator authorization,
  atomic link plus activity/event writes, and concurrent capacity enforcement.
  Domain owns bounds, state and reason validation. No SQL in API/UI adapters.
- One reusable decision/task membership projection must cover both original and
  explicit links. Apply it consistently to next-move ownership, decision-aware
  dispatch/attention guards and review/recovery evidence. A read never creates a
  link. Avoid duplicate rows when the original task is also queried.
- Pending links use existing decision precedence: Ready/Review/Blocked work can
  wait on the operator; Active execution is not interrupted or relabeled merely
  by linking. Queen still decides whether a genuinely stalled Active task must
  transition to Blocked. No second task starts past the single-active-task guard.
- Resolve/withdraw invalidates linked evidence and exposes the next legitimate
  move; it does not resume Blocked work, erase review requests, satisfy task
  prerequisites, or automatically send answers into every worker terminal.

## Acceptance and rollout

Verify duplicate/concurrent links, changed reasons, original-task duplication,
foreign/missing/settled targets, unauthorized workers, pending-decision races,
capacity, removal and restart. Verify one decision gates several tasks, a second
pending decision remains effective when the first resolves, and resolution or
withdrawal invalidates all affected review evidence without duplicating command
grants or terminal delivery. Exercise real adapters and rendered Queues/Needs You
with fictional tasks before promotion. Existing genuine waits must remain safe.

Keep this migration independent of the inactive support outbox. Allocate its
schema version during integration and reconcile support activation ordering;
never activate support or skip a required table because another branch advanced
the database version. This ADR does not claim the persistence/API/UI is wired.

## Implementation checkpoint, September 7

Root allocation is membership schema 148, followed by the inactive support
outbox at 149. Main integration must keep support inactive and reconcile its
separate migration; do not promote support just to obtain this capability.

The shared application command is exposed as Queen-only
`swarm_set_task_decision_link`. Decision reads include bounded linked tasks and
reasons; Needs You keeps one answer surface with those tasks in its details.
Pending original and shared gates both hold queued briefing delivery. Clearing
the gate restores ordinary delivery eligibility without changing task state.

Evidence so far: the full persistence suite passed 659 tests after the historic
withdrawal-fixture correction and shared read projection. The subsequent ten
focused tests passed, including concurrent last-slot admission and real dispatch
hold/recovery. The Queen MCP authorization/stale-revision test passed before the
last projection change; its final rerun is required. The 91 focused Needs You,
Queues and briefing UI tests passed. These are not live acceptance evidence.

The final twelve focused persistence checks passed, including explicit
foreign/missing/removed/finished targets. The final Queen command and strict
API checks also passed. Broader MCP discovery caught the required tool-surface
revision bump; revision 22 and its served-schema fingerprint are now recorded
and need their final broader rerun before integration.

Still required: final broader Rust checks, deployed-schema integration,
current tool discovery in Queen,
fictional end-to-end orchestration and rendered live browser verification.

The main-only integration (8c009bce, schema 148 with support excluded) passed
647 persistence tests, 71 MCP tests, formatting and strict Rust checks. A final
read-path audit found that compact MCP decision summaries and worker-scoped
listing also needed the relation. The follow-up adds a count to the compact
index, full links plus an explicit permission-scope warning to the exact decision
read, and assigned shared-task discovery. Its 71 MCP tests passed; the refactored
focused test and strict checks passed afterward. No live acceptance is claimed.
