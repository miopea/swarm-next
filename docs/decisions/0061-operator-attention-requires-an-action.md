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

## Consequences and verification

The legacy `blocked_escalations` response name remains a compatibility field for
now; the maturity UI treats it as waiting-age evidence. The orchestration adapter
owns eventual renaming when supported clients no longer require the old field.
No new timer or automatic task transition is introduced by this presentation move.

App tests must prove age-only work is absent from Needs You and its count while
remaining available in Queues. Queue tests must prove deduplication, closed-task
precedence, and explicit unknown ownership. Queen's ability to create a useful
operator escalation and broader delivery reconciliation remain separate P4 work;
this change alone does not complete the attention contract.
