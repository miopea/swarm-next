# ADR 0082: Evidence-backed Queen review coverage

Status: Accepted direction under the approved maturity scope; implementation and
live acceptance remain in progress.

## Outcome

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

### Review fairness across incomplete turns

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
