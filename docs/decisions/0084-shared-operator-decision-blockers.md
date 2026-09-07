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

### Live checkpoint, September 7, 17:40 UTC

App/API build `e2a896f4b96c-20260907172200-133536` is serving the development
Hive with membership schema 148. Its complete CI run `34147294805` passed.
Queen alone was refreshed through retained-session stop/start at an idle,
unengaged prompt to discover revision 22; the other fifteen session identities
were preserved. The older running worker engine has not been replaced.

In the fictional `5663a1f7` scenario, Queen herself used the new link command
on consumer `01a07cea-8992-7aa3-9a67-e87d5374d09c`, moved it Ready and assigned
it to the demo worker. The source remained Blocked on the single input decision
`01a07ceb-2e1c-7921-a1b8-2e9135736558`. The task audit identifies Queen as the
actor; the controller did not perform these routing transitions.

The separate authenticated Edge tab at `swarm.bfgsolutions.net` rendered one
Needs You card, its source and linked consumer in expanded details, and the
existing "Say something else" option. A screenshot confirmed readable desktop
layout. Clicking "Use fictional sample A" resolved the fictional decision and
the page cleared to zero without reload. The consumer became worker-owned and
eligible for normal delivery; the source became Queen-owned for reassessment.
This proves shared gate creation and clearing, not final task completion.

The fixture's optional API resolution action initially used the wrong route
and received HTTP 405 without changing the decision. It now uses the actual
`PATCH /decisions/{id}/resolution` contract. The successful resolution above
was through the browser, not a claimed successful script execution.

Still open: observe both tasks completing through normal recovery, execution
and settlement; inspect any failure instead of forcing task state. Native
mobile acceptance and engine convergence remain separate unverified gates.

### Completed fictional workflow, September 7, 17:42 UTC

Both tasks subsequently reached Completed without controller task transitions
or terminal input. The browser answer was recorded at Unix time 1788802790.
The source worker verified the resolved decision, resumed Blocked to Active
18 seconds later, recorded nine passing Node tests, and submitted Review.
System settlement completed it at 1788802843 (53 seconds after the answer).
The consumer was then delivered normally, verified its own explicit shared
link, became Active at 1788802878 and completed at 1788802912 (122 seconds
after the answer). Each completion event identifies the system actor, not
Queen or the operator. No duplicate question was raised.

Task activity sequences 7850-7856 preserve the recovery, evidence and automatic
settlement. The controller separately confirmed both final states, one resolved
decision retaining its link, the fixture's unchanged HEAD `27fbcc152289d33e`
and clean working tree, and independently ran all nine tests successfully.
This independent rerun corroborates the fixture, not the provider's original
execution timing. The worker corrected an invented message identifier in its
own task note; use the actual recorded message `01a07ceb-9412-7eb1-a772-d6e55a8bba03`
rather than the superseded prose identifier when tracing delivery.

The shared-decision fictional workflow is accepted for this deployment. This
does not establish that Queen has reconciled existing real-world prose-only
blockers, nor close the wider orchestration, mobile or engine-update scope.
The script correction passes `bash -n`; its successful resolution branch still
needs a separate fixture because this decision was answered through the UI.

## September 7 operator-approved resolved-decision references

The operator explicitly approved Queen linking an existing resolved decision
from its originating task to another unfinished task within the original scope.
This supersedes the prohibition on new resolved-decision links above, not the
prohibition on withdrawn decisions or expanded permission. Queen must explain
applicability in the audited link reason, preserve the original wording and
response, and ask the operator if scope is uncertain. Persistence requires an
authenticated operator resolution and an originating task in the same Hive.

Reuse the bounded explicit membership relation. A resolved reference is evidence,
not a pending gate, command grant, answer delivery, task transition or assignment.
Review deferrals may cite a resolved decision only through original or explicit
membership; removal or changed evidence invalidates coverage. Test missing
operator provenance, withdrawn/foreign/finished targets, replay, atomic failure,
unchanged approval/delivery scope, and linked deferral validation. Live Queen
reconciliation remains required; do not bulk-link real tasks from prose.

Implementation checkpoint: the existing link command now accepts authenticated
resolved references, and deferral validation uses the shared membership view.
The pending-gate queries still filter pending decisions; no new answer delivery
is created. Tests verify unchanged original decision/answer fields, rejected
missing provenance, explicit membership before deferral, and invalidation after
unlinking. Twelve shared-link tests, two resolved-reference persistence tests,
four domain link tests, the delivered-focus test and all 71 agent adapter tests
passed on the main-compatible Linux tree. The tool schema is unchanged; updated
focus instructions teach an existing Queen session the approved scope boundary.
Deployment and live use are not yet established by this checkpoint.

### Resolved-reference live checkpoint, September 7, 20:00 UTC

API `5ace7c313649-20260907194623-257889` is healthy with no degraded
subsystems or database recovery required. All sixteen exact session identities
were preserved during deployment. The health response's engine build ID is the
packaged fingerprint, not the running engine identity: terminal-host status
reported retained engine `e1d98e0a4b94-20260907183121-205234`, sixteen sessions,
no unreadable sessions and no draining. No engine replacement or release occurred.

Fictional task `01a07d6d-4277-7410-8030-6be9eb814426` exercised the new
resolved-reference path. Activity 7986 identifies Queen as the actor adding the
original-scope reference to decision `01a07ceb-2e1c-7921-a1b8-2e9135736558`.
The original source task, resolved answer and null requested command remained
unchanged. The demo worker's terminal records its own scoped source read,
exact reply to request `01a07d74-a591-7142-9370-5bb3b9856e41`, nine passing Node
tests, unchanged clean HEAD `27fbcc152289d33ef5d7e8a9e9af77a6624dc33b`, and a
truthful empty commit report. Activity 7987 independently confirms ordinary
system settlement to Completed. Source-read details, test execution and exact
reply are worker-observed evidence, not independently extracted message rows.
The controller did not link, answer, or manually complete this task.

Queen also linked real child `01a066f3-b078-73b3-a0f6-d8aff7b9a43a` to original
resolved decision `01a06624-be2c-71a3-9d97-d1fcedc86a43`, explaining why the
operator's narrow choice excludes the wider work. The child remains Blocked;
this is evidence of authentic scope reconciliation, not permission to execute
the excluded work or proof that the fleet backlog is reconciled.

CI `34156818818` caught a single-element loop in the new domain regression test.
Web, packaging and security audit passed, but Rust tests were skipped after
workspace lint failed. The correction passes full-workspace/all-targets/
all-features strict Clippy, formatting and all four domain link tests on Linux.
It is main commit `9227d2c4`; its CI result must be checked separately. The
earlier API-only Clippy check was insufficient to cover dependency test lints.
