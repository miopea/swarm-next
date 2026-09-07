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
