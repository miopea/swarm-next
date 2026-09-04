# ADR 0061: Operator attention requires an operator action

Status: Accepted for the operator-approved daily-driver maturity program.

## Decision

Elapsed blocked time is evidence for orchestration and Queues, not authorization
to interrupt the operator. This supersedes the earlier twelve-hour UI escalation
policy. Queen escalates when she cannot move the work and needs a specific human
decision or action. Existing decision requests remain the actionable channel.

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
