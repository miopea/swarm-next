# ADR 0082: Evidence-backed Queen review coverage

Status: Accepted direction under the approved maturity scope; implementation and
live acceptance remain in progress.

## Outcome

### September 8: disappearing focus cursor and unchanged order

A checked operator deferral may leave the focus candidate set. The persisted
cursor remains an identity boundary: continue with the first greater candidate,
wrapping only at the end, even if the cursor task itself is now absent. Resetting
to the beginning on absence lets recurring external checks starve later work.
This changes attention traversal only, never task authority or coverage.

Reordering must update only tasks whose position actually changes. Unchanged
positions retain their timestamps and review evidence; exact order replay emits
no TasksChanged event or row write. Changed positions still invalidate their
own complete task projection. This does not erase a real dependency or decision
change, nor treat order changes as task execution progress.

A Queen turn ending is not proof that her outstanding work was examined. A
review may be settled only when its bounded obligations have current, durable
dispositions, or the obligations have actually moved out of Queen ownership.
Neither a summary sentence nor the outcome `no_action` supplies that evidence.

## Boundaries

For Ready and returned Review work, an open task-linked operator decision owns the next
move before the worker's outstanding review request. This does not erase the
request, change the assignee, move Review backward, or approve work. Resolving
the decision re-derives worker ownership if the request remains outstanding.
Likewise, a pending decision suppresses an older idle-worker attention record;
the same task must not simultaneously be described as an unexplained worker
stall while the system is waiting for a recorded human ruling. This does not
infer an operator decision from prose or alter ordinary Active-task ownership.
For Ready work, resolving or withdrawing that ruling restores worker ownership
when assigned, otherwise Queen ownership. Neither the task lifecycle nor its
assignment or dispatch record changes as a side effect of this read projection.

The application derives obligations from the same domain task projection as
Queues. The persistence boundary supplies an opaque evidence revision covering
the task, relevant decisions, dependency states and current delivery evidence.
A task timestamp alone is insufficient: completion of a prerequisite changes
the obligation even when the consumer task itself has not been edited.

Queen records a disposition through an explicit command, not a side effect of
a read-only tool. The command requires the observed evidence revision and checked
source references. Dependency, operator-decision and scheduled-window claims
must match their existing authoritative records. A prior operator deferral must
retain its actual operator provenance; Queen cannot manufacture a new approval.
External-condition judgments require a concise condition and verification source;
they are judgments, not machine-proven completion or permission to resume work.

Coverage is evaluated separately from queue clearance. A verified wait can be
covered while work remains waiting. An unchanged receipt is reusable only while
its evidence revision still matches; external conditions also require a fresh
check each run because external state is absent from the local revision.
Authenticated operator deferrals remain reusable while local evidence matches.
Changed dependencies, decisions or delivery
invalidate it. No timer, repeated reminder or new human approval establishes
coverage. A capacity-exceeded or partial snapshot cannot certify a complete review.

The task evidence read also returns its single saved assessment, condition/source,
and an explicit reuse status from that same transaction. A matching authenticated
operator deferral can cover the current run; an external condition from another
run requires a fresh check. Changed evidence and absence of an active review are
explicit, never reported as covered. Reads create no receipts, activity events or
terminal observations. This avoids reconstructing already-verified judgments from
history without turning historical assertions into fresh proof or clearing queues.

## Bounded execution and recovery

### Prompt-ended work becomes visible without an age gate

An unfinished Active task or unanswered returned Review may enter Queen's
recovery attention immediately when a fresh, complete canonical snapshot shows
a resting prompt, known-empty input and no visible background work. Cached
activity alone cannot establish this early path. The existing domain terminal
safety gate is reused; this observation is not a delivery or permission to act.
Persistence rechecks assignment, task/review revision, session, engagement and
pending decisions, and deduplicates the same attention identity.

Use the existing bounded coordinator owner: at most 32 candidates, four parallel
reads, each with the existing two-second deadline. No extra timer or retained
terminal transcript. The existing 30-minute path remains for reporting older
unknown/background evidence honestly; it is not the admission gate for a freshly
verified empty idle prompt. Queen must still obtain current evidence before any
guarded continuation, distinguish intentional waits and assess the result.
An early attention record never creates a Needs You request or changes task state.

### Recovery visibility shares the coordinator snapshot

Queues displays recovery responsibility separately from task execution ownership.
The persistence boundary reads at most 32 live attention identities plus one
overflow sentinel in a single statement, with exact-session message delivery
evidence and any historical assessment. The existing supervisor activity cache
supplies observations; opening or refreshing Queues never reads terminal output.
Engaged and observed working/background sessions are not recovery rows. Missing
observations remain explicitly unavailable. Delivery is not execution, and a
previous assessment is not a current revalidation.

The browser fences independently refreshed task and recovery data by task,
worker, session and task revision, using the same projection for rows and badge.
Partial or missing recovery data cannot support an all-clear claim. The optional
field is an API rolling-update compatibility path owned by the control-room UI;
remove that fallback when all supported API versions expose recovery snapshots.
This projection never dispatches, changes task state, or creates a human decision.

### Queue age is not retry activity

Persist the entry time of each briefing generation separately from retry
`updated_at`. Persistence owns this bounded metadata on the existing dispatch
row: insert establishes it; a new generation restarts it; claim, refusal,
retryable failure and crash reconciliation preserve it. It does not authorize
dispatch or escalation. Presentation age and retry scheduling are distinct.

Migration 146 freezes existing `updated_at` as a lower bound because the exact
original entry time was not retained. The API adds `queued_at_is_lower_bound`;
the UI says "at least" and rounds down for migrated rows. Mixed groups retain
that qualification. Missing evidence from an older API is also qualified.
The UI adapter owns this older-API fallback until supported mixed-version
deployments all supply the field. No age alone makes an operator decision.

Inactive support outbox activation must remain later than this independently
deployed migration (147 on the integration branch). This migration must not
create the support outbox or activate its sender on the development Hive.

Provider conversation compaction and an intervening worker notification do not
finish a review. Queen can recover the unfinished delivered run identity through
a read-only coordination-attention observation. Unlike lifecycle reconciliation,
this read cannot expire or requeue a run. A notification prepared for the exact
delivery session carries a bounded context reminder, preserving its own delivery
identity and requiring a fresh read before acting; a concurrent finish must not
be undone. This is context restoration, not a new review or permission to replay
prior side effects. Notification context alone does not recover an idle Queen
when no new notification arrives; that same-session recovery path remains part
of acceptance and must respect terminal/input safety and bounded delivery.

Same-session continuation uses a fresh, complete canonical
snapshot showing Resting, no background work and known-empty input. The domain
gate refuses unknown observations; persistence rechecks the exact Running run,
delivery session, live Queen session, enabled automation, operator engagement
and Steward takeover. It queues the same run without resetting its existing
three-attempt delivery budget. Normal delivery guards recheck terminal safety.
A concurrent explicit finish is accepted for an already-delivered queued or
delivering continuation, with the same coverage checks; an initial undelivered
run still cannot finish. Unattended authority restrictions remain active during
that continuation. This is bounded recovery, not a claim that exhausting the
budget resolves the underlying failure. Exhaustion visibility and full adapter
failure/restart acceptance must be verified before this slice is deployed.

Task-review and worker-recovery dispositions use that same unfinished-run rule: a delivered queued
or delivering continuation can record an assessment against its exact run and
current task evidence. Requeueing does not invalidate ongoing Queen judgment.
An initial undelivered continuation, missing delivery session or closed run
cannot accept a new assessment. This corrects the September 8 live mismatch
where the read advertised a current run but the write rejected unchanged
evidence. It does not enqueue delivery, reset attempts or waive coverage checks.

Recovery receipts have their own bounded persistence record, separate from task
review dispositions. One record per task retains the exact attention, worker,
session, task-evidence revision and optional canonical-terminal revision, plus
the review run and concise checked source. Retired-task receipts may be pruned;
their task activity remains the audit history. Replays must revalidate current
evidence and cannot duplicate that history. External-wait claims require an
explicit authenticated assessment with a concise condition, checked evidence
and source. They remain Queen judgments, not machine proof or operator approval.
The receipt must match the current run as well as task, session and terminal
evidence; another run requires another actual check.

The local recovery migration uses version 144. Version 143 belongs to an inactive,
not-deployed support-outbox foundation; it must not be activated as a side effect
of recovery delivery. When that integration is delivered to a Hive already at
144, its migration must use a later version rather than relying on the old 143
comparison. Swarm's integration owner is responsible for that renumbering before
support-outbox activation; the recovery feature does not send support messages.

At most 256 obligations and receipts participate in one coverage evaluation;
detail responses are paged/bounded separately. Duplicate or stale receipts do not
cover another obligation. Updates and run completion must recheck the evidence
inside the persistence transaction. Lost responses recover the same saved result;
API restart must not discard valid receipts or interpret absent data as empty.

The completion command records uncovered turns as `incomplete`, even if the model
requests `completed` or `no_action`. Queen may explicitly finish as `incomplete`
if evidence is unavailable, without an endless finish/retry loop.
An incomplete review remains Queen-owned work. It is not automatically a Needs
You decision, does not resume blocked work, and does not interrupt an engaged
terminal. Queen escalates an actual inability to recover with a concrete request.

## Acceptance

### Checked waits in the operator queue

Expose a separate bounded review-judgment snapshot through the existing
coordinator read, never by adding receipt fields to the Task evidence hash.
At most 64 saved open-task judgments are revalidated using the same revision
and run rules as Queen's exact read; report overflow explicitly. A single
TaskStore-owned cache retains that snapshot only across unchanged local
connection total_changes and SQLite data_version, before the earliest pending
hold deadline. A backwards clock, write, restart or recovery fence prevents
reuse. Errors never return previously cached coverage. No timer, terminal
observation, schema migration or new command authority is introduced.

The UI fences each item against the complete task projection from its separate
task read. Valid operator deferrals may appear under deliberately parked work;
current-run external assessments appear with dependencies/holds. External
assessments needing a fresh run check remain Queen work. Between runs only an
unchanged authenticated operator deferral is reusable for this presentation.
Keep concise condition, check time and expandable evidence/source visible.
Missing, stale or failed coordinator evidence uses recorded ownership, not an
all-clear. These presentation groups never change task state, next-move owner,
review completion, task count, permissions or terminal delivery. The control-room
UI owns the mixed-version unavailable fallback until supported APIs expose it.

Verify unchanged cache reuse without writes, local and external same-second
changes, deadlines/backwards clocks, corrupted evidence, database recovery,
restart, bounds and mixed browser payloads. Measure the cold read and unchanged
read overhead before deployment; a bounded cache alone is not performance proof.

September 7 implementation checkpoint: the isolated Linux fixture passed local
and external-write invalidation, same-second edits, corrupt receipt rejection,
restart, deadline/clock and database-recovery fences. With 64 saved judgments,
one cold validation measured 85,901 microseconds; 100 unchanged cached reads
measured 5,711 microseconds total. These are synthetic debug-build observations,
not live API latency percentiles or browser performance acceptance. Fifty-seven
queue/API web tests and the twenty persistence review tests passed. The served
coordinator contract test and full-workspace strict Clippy passed before the
final unavailable-read isolation; final reruns and full persistence tests are
in progress. No deployment or rendered acceptance is claimed yet.

If judgment validation is unavailable, the coordinator exposes a null judgment
snapshot while preserving its independently available delivery/recovery data.
The UI restores recorded ownership and reports the unavailable details instead
of retaining old checked-wait groups. It never converts this failure to an empty
successful judgment list.

### Review fairness across incomplete turns

An explicitly recorded `insufficient_evidence` assessment records what Queen
checked, the concrete missing fact and its source without asserting a valid wait.
It advances the existing last-assessed ordering so one unresolved investigation
cannot monopolize every focus batch. It never covers a review obligation, changes
ownership, parks a task or authorizes work, including in the run that records it.
The read verdict is `insufficient_evidence`, not covered. Queen must continue
through other outstanding work and pursue the missing fact or request actual
operator assistance; this is not a substitute for a recoverable next action.

Use the existing single bounded assessment per task, exact revision/run fences,
authenticated command and activity event. Preserve saved identities and evidence
through migration 150; do not activate the separate support outbox on execution
Hives. An explicit unknown assessment may supersede a formerly verified wait,
which must then cease to count as covered. Lost-response replay preserves the
original assessment time and event rather than indefinitely refreshing fairness.
Verify same-run and later-run noncoverage, replay, stale revision refusal,
restart/migration preservation, replacement of a valid wait, and UI ownership
fallback before deployment. Reads remain non-mutating.

September 7 live review `01a07ceb-030f-7341-9b22-24c812e13d9b`
handled incoming work and recent recovery, then explicitly left about seventeen
older blocked tasks and four operator-reserved drafts uncovered. Recording
incomplete correctly does not by itself prevent repeated neglect of that work.

Order the bounded Queen review list by absence of an assessment, then oldest
historical assessment, with creation time and identity as stable ties. Retain
at most 64 detail rows with full board counts and an overflow indicator. Read
only bounded receipt timestamps through persistence; do not rehash histories or
observe terminals just to order attention. Historical timestamps are ordering
hints, never valid coverage. Existing evidence checks remain mandatory.

This changes neither execution priority nor ownership, lifecycle or authority.
Queen must advance unchecked backlog alongside urgent incoming work and preserve
authenticated operator deferrals. Failure or overflow of the ordering read is
not an empty or healthy queue. Test ordering, capacity and read-only behavior,
then observe real backlog assessments before claiming the issue resolved.

The first post-deployment review still skipped the older work and incorrectly
described the completed fictional pair as pending. Therefore list ordering alone
has not passed live acceptance. Include at most three current fairness-ordered
task identities at the beginning of each delivered review prompt, using the same
application snapshot after the existing resting check. Explicitly require fresh
history/evidence and preserve reserved drafts and real blockers. This is a focus
within the full review, not a three-task definition of completion. If focus
selection fails, retain the existing bounded delivery failure path rather than
delivering a misleading empty focus. The normal submission gate still protects
session identity and operator input; no new message or timer is introduced.

### Queued briefings without a recorded order blocker

Queen's existing read-only coordination-attention tool may additionally observe
up to eight distinct workers whose briefing is queued without a durable task-order
blocker. Use at most four concurrent reads and a two-second deadline per read;
these supplement, not replace, the existing maximum 32 attention observations.
No polling, retained transcript, dispatch, task transition or new approval follows
from opening this view. Other durable holds are not relabeled as terminal faults.

Recheck worker, task and session identity after each read. Ended/wrong sessions,
truncated snapshots, failed reads and changed assignments stay unavailable.
Protect operator engagement. A current provider question may carry a bounded,
explicitly untrusted rendered excerpt, excluding detected unsent composer input.
It may belong to earlier work, so Queen must correlate history and existing
decisions before asking for a task-linked ruling. The excerpt is neither proof
of operator authorship nor authority to inject an answer. No new customer-facing
or diagnostic export is authorized. This observation does not count as a recovery
receipt or certify review coverage.

### Worker execution ownership does not waive recovery accountability

September 7 recovery-view correction: the compact `active_work_recovery` view
must include all three attention kinds already selected by its bounded terminal
observer: stale owned work, delivered Ready work that has not started, and owned
work never briefed. Previously the observer read all three but the compact view
silently discarded the latter two. Queen's live run reported that view empty
while the coordinator still showed unstarted-work obligations. Share the kind
predicate between observation and projection; retain the 32-row bound and all
resting/background/input safety semantics. Include the attention kind so a
missing briefing is not described as a delivered one. This changes no task,
authority, dispatch, observation frequency or schema. A failing-before regression
must demonstrate inclusion of Ready work and continued exclusion of busy,
background, unknown and missing observations. Real worker pickup still requires
live acceptance after deployment.

September 7 follow-up: a required operator action is not an external-condition
wait. Live Admin/Member Services recovery repeatedly named operator-seeded
sessions without creating pending requests. Queen must create/reuse the exact
task-linked Needs You assistance request and link other genuinely affected tasks;
use the existing `await_operator` disposition with its pending decision identity.
An authenticated, applicable operator deferral remains a deliberate wait, not a
reason to ask again. Mention in a terminal, presence or Queen memory does not
establish delivery or deferral. Never bypass authentication to avoid escalation.
This guidance travels in each bounded review delivery as well as the standing
Queen instructions, so existing sessions receive it without a provider restart.
There is no prose classifier, automatic decision creation, schema change or new
authority. Tests prove delivery of the instruction, not model compliance; live
task-linked assistance and normal resumption remain acceptance requirements.

September 6 dogfood evidence exposed a gap in the initial implementation:
coverage considered only tasks whose next move was Queen-owned. A worker's
Active task could remain at a finished terminal turn while Queen called the
fleet healthy or carried forward an informal environmental-stall note.

Extend review accounting with a separate bounded recovery obligation; do not
change the task's execution owner or move Active work through Ready. Derive
candidates from unchanged task/session attention identities, then obtain fresh
canonical terminal evidence. Unknown, unavailable, changed-session and partial
observations cannot prove a healthy worker. Do not classify every Active task
as stalled or use snapshot sequence movement alone as work progress.

Account separately for these outcomes:

- Current active/background execution: do not interrupt it. Observation is not
  task completion or proof of a durable handoff.
- Operator engagement or real unsent input: preserve the terminal. A dimmed
  provider suggestion is neither engagement nor an operator instruction.
- A pending guarded recovery request: retain its exact delivery identity and
  avoid duplicate requests. This is awaiting delivery, not resumed execution.
- A delivered request with unchanged resting work: Queen still owns verifying
  the response and choosing recovery. Delivery alone cannot settle the concern.
- A real dependency, recorded window, or authenticated operator decision:
  reference the exact current source. A prior Queen note cannot manufacture it.
- Failed recovery: retain the attempted action and concrete remaining obstacle;
  Queen escalates only the actual missing authority or assistance, with a concise
  task-linked decision. Low urgency is not permission to abandon the task.

Persistence must fence any saved assessment to task, session and evidence
identity, and invalidate it when those facts change. External judgments retain
the existing fresh-check requirement. Finishing a turn must remain possible as
incomplete; never trap the provider in repeated finish calls or auto-create an
operator approval merely because coverage is missing. Expose recovery ownership
separately from execution ownership in Queues, with the actual attempted action
and next step rather than an unbounded transcript.

This extension is an implementation requirement, not a claim that the current
receipt table or corrected transcript projection already enforces it. Verify
busy workers, real input, pending versus delivered requests, genuine waits,
failed delivery/restart, stale assessments and unrelated running workers in an
isolated multi-worker scenario before declaring recovery supervision complete.

Prove bounds, duplicate/stale/missing receipts, valid waits without queue-clear
claims, dependency completion, changed/withdrawn decisions, concurrent changes,
lost responses and restart. Then use isolated demo work to show verified gates
remain protected and actionable work moves. Do not call a new receipt table or
domain helper the completed orchestration fix.

### Review focus excludes already-covered waits (September 8)

Live consecutive runs selected the same three oldest authenticated operator
deferrals, although each was already covered. Reusing those valid assessments
correctly leaves their timestamps unchanged; timestamp-only focus selection
therefore starved other obligations. Historical age alone is not review demand.

The bounded review focus must prioritize uncovered obligations before covered
waits. Current validated coverage, or an unchanged authenticated operator
deferral between runs, removes a task from the small prompt focus only. It stays
in ownership counts and the full Queen backlog. External conditions between runs,
insufficient evidence, changed evidence and missing snapshot entries remain
eligible. Fence separate task reads by the complete projection, not timestamp.
The final coverage check remains authoritative and unchanged; focus selection
grants no execution permission, clears no blocker and does not certify recovery.

### Unchanged investigations do not precede fresh checks (September 8)

Live run `01a0808e-b347-77d1-81b0-b7879ad0df1b` reused unchanged
insufficient-evidence investigations for all three focus items and finished
incomplete. Their old assessment timestamps again occupied the small focus.
Within uncovered work, prioritize tasks needing a fresh assessment before an
unchanged insufficient-evidence receipt. Compare complete task projections and
the persistence-validated evidence status; changed or missing evidence removes
this lower priority. Keep both sets in the full backlog and eligible focus list.
Do not refresh receipt timestamps, manufacture coverage, clear blockers or alter
execution priority. This is attention ordering, not permission to abandon the
missing fact. Six focused application tests passed on the isolated Linux tree;
live backlog progression remains an acceptance gate.

### A saved plan is not ongoing execution (September 8)

Eleven of the twelve latest Admin recovery assessments described an empty
resting prompt, no background work and unfinished buildable steps, yet called
that a verified external wait or "between turns." The latest assessment claimed
activity again; this history read does not independently verify that claim.
Earlier snapshot-sequence changes were treated as progress despite unchanged content.
An old delivered instruction is not evidence that the next turn is executing.

The standing brief and every review delivery now explicitly distinguish these
facts. After checking the prior response and actual task gates, Queen should
request a concrete same-task continuation for remaining authorized work. A
blocked portion does not prohibit independent buildable steps inside the same
scope. Real external jobs, dependencies, operator deferrals, uncertain deliveries
and current input/engagement protections remain unchanged. No prose classifier,
automatic task transition or repeated generic kick is introduced.

The new delivered-prompt assertion failed before the correction. All 48 delivery
regressions and the standing-brief test pass afterward. These tests establish
instruction delivery, not model compliance or completed recovery supervision;
live continuation and resulting task progress remain required acceptance evidence.
