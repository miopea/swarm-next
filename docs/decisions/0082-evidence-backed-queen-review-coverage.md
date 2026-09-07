# ADR 0082: Evidence-backed Queen review coverage

Status: Accepted direction under the approved maturity scope; implementation and
live acceptance remain in progress.

## Outcome

A Queen turn ending is not proof that her outstanding work was examined. A
review may be settled only when its bounded obligations have current, durable
dispositions, or the obligations have actually moved out of Queen ownership.
Neither a summary sentence nor the outcome `no_action` supplies that evidence.

## Boundaries

For returned Review work, an open task-linked operator decision owns the next
move before the worker's outstanding review request. This does not erase the
request, change the assignee, move Review backward, or approve work. Resolving
the decision re-derives worker ownership if the request remains outstanding.
Likewise, a pending decision suppresses an older idle-worker attention record;
the same task must not simultaneously be described as an unexplained worker
stall while the system is waiting for a recorded human ruling. This does not
infer an operator decision from prose or alter ordinary Active-task ownership.

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

## Bounded execution and recovery

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

Prove bounds, duplicate/stale/missing receipts, valid waits without queue-clear
claims, dependency completion, changed/withdrawn decisions, concurrent changes,
lost responses and restart. Then use isolated demo work to show verified gates
remain protected and actionable work moves. Do not call a new receipt table or
domain helper the completed orchestration fix.
