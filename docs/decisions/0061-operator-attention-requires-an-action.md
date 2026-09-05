# ADR 0061: Operator attention requires an operator action

Status: Accepted for the operator-approved daily-driver maturity program.

## Decision

Elapsed blocked time is evidence for orchestration and Queues, not authorization
to interrupt the operator. This supersedes the earlier twelve-hour UI escalation
policy. Queen escalates when she cannot move the work and needs a specific human
decision or action. Existing decision requests remain the actionable channel.

A pending decision on a Blocked task names the operator as the next owner,
without changing the task's state or bypassing its prerequisites. Resolving or
withdrawing that decision re-derives ownership from the remaining facts: Queen
may review satisfied prerequisites, while an unresolved hard block stays blocked.
Active work remains the worker's responsibility even with a decision alongside
it; an explicit review hand-back likewise preserves the worker's outstanding work.

The same source selection must govern the Needs You page and its count. Moving an
age-only observation to Queues removes it from both. Retain the waiting evidence;
do not hide it or silently claim the queue is healthy.

When independently refreshed observations overlap, present one task once. A task
known to be closed takes precedence over an older blocked observation. Unknown
next ownership is explicitly unknown, never an empty queue or an invented owner.

Ordinary prompt/unsent-text delivery holds are coordination observations, not
operator escalations. Show them in Queues with their recorded reason and target;
do not infer that Queen stopped working or that nothing can route. An explicit
decision can still request operator help. Unconfirmed wakes retain their existing
manual recovery attention until the safe recovery lifecycle replaces that path.
Unknown hold kinds are not silently discarded by this routing change.

Prompt hold observations belong to the worker/session that produced them. A
different binding starts a new occurrence; incompatible prompt reasons for the
same delivery replace each other atomically. A known ended session cannot be
reported as currently holding input. Unbound or unknown session evidence is not
silently treated as recovered. This does not replace the remaining need for
current provider-state reconciliation and generation-safe delivery observations.

Prompt observations no longer expire after three minutes without another retry.
Silence is not evidence of resolution. Queues labels these as last observed holds,
with the observation timestamp and unconfirmed resolution. Explicit clearing and
known ended-session evidence still remove them. Non-prompt refusal freshness is
unchanged pending its own recovery integration. The read projection is capped at
256 observations; overflow returns unavailable, never a partial all-clear. The
coordinator owns future source-state cancellation and generation reconciliation;
this change does not assert that a historical hold is currently blocking work.

Known task-brief subjects are projected only while their task is Ready/Active
and a matching unreleased assignment has a Queued/Dispatching briefing. Recorded
worker/session identity must match when available; new task-brief observations
always include the immutable session. Completed dispatches and tasks that moved
past briefing therefore remove old holds, including late repeated observations.
Unknown legacy subjects remain unconfirmed. New task-brief observations and
successful clears use `task-dispatch:<assignment-id>:<generation>` subjects, preventing an old
assignment result from altering a new assignment's row even within one session.
The projection verifies that exact pending assignment. A valid scoped observation
atomically clears the corresponding legacy task-wide observation. Persistence owns
that compatibility path until task-wide observations are no longer present; no
legacy identity inference is required. Schema 129 adds the briefing generation
so returned work on an unchanged assignment also has distinct observation and
result ownership. Other delivery families retain their own identities below.

Queen automation delivery holds use `queen-run:<run-id>`, not the singleton
`queen-review` subject. Only the exact queued/delivering run in the recorded
Queen session remains a delivery hold; running/completed/uncertain is not that
same condition. Its existing automation status remains authoritative for those
states. A valid scoped observation replaces the legacy singleton observation
atomically. The legacy projection also stops displaying a hold once a known run
has progressed beyond delivery. This says nothing about other work Queen can do.

Worker outcome holds similarly use `outcome-delivery:<id>` and the recipient
session. The exact pending outbox row and matching task target state determine
whether the historical prompt hold remains applicable. A newer valid observation
replaces the task-wide legacy row atomically; completed or superseded handoffs
cannot revive a hold through a late result. Known legacy task-outcome subjects
also require applicable pending work. This does not settle uncertain delivery or
replay a report; those remain distinct outbox states.

Decision-answer holds retain their immutable decision ID and now include the
actual delivery session. Known requests require a resolved decision with a pending
matching outbox row. Delivered or uncertain answers are no longer prompt holds;
the decision's own delivery state remains unchanged and readable. No answer is
replayed or inferred from queue presentation. Unknown legacy subjects stay unknown.
The bounded source-applicability query lives in `coordinator_refusals.sql` within
the persistence boundary so the family-specific checks can be reviewed together.

## Consequences and verification

A failed coordinator read is not evidence that held work resolved. Preserve the
last successful observation until a successful replacement or logout. Needs You
and Queues qualify that evidence as unavailable for refresh, without adding an
attention badge or claiming an all-clear. Visibility/unmount cancellation is not
a service failure; request deadline expiry is. A successful empty response clears
the retained observations and the qualification.

The legacy `blocked_escalations` response name remains a compatibility field for
now; the maturity UI treats it as waiting-age evidence. The orchestration adapter
owns eventual renaming when supported clients no longer require the old field.
No new timer or automatic task transition is introduced by this presentation move.

App tests must prove age-only work is absent from Needs You and its count while
remaining available in Queues. Queue tests must prove deduplication, closed-task
precedence, and explicit unknown ownership. Queen's ability to create a useful
operator escalation and broader delivery reconciliation remain separate P4 work;
this change alone does not complete the attention contract.
