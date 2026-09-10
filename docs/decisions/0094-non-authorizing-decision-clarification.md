# ADR 0094: Clarification is not an operator decision

Status: Accepted implementation direction under the approved ATT-01/QUEEN-03
UX maturity scope. End-to-end implementation and acceptance remain open.

## Decision

Attach an immutable, bounded question/reply exchange to the exact Needs You
decision. Asking for explanation must never resolve, dismiss, withdraw, alter
allowed actions, mint command permission or resume a task. The original decision
is still pending. A worker's explanation is not an operator answer.

The operator can ask a question, read the reply and then use the original final
answer controls. Final answers remain possible while clarification is outstanding.
Only one unanswered clarification per decision is admitted. A new round follows
an explicit reply, not a timer. Keep questions and replies verbatim; UI labels
must distinguish Ask a question from Say something else (a final answer).

The authenticated operator asks. The requesting worker or Queen may answer the
exact clarification ID through an authenticated application command; another
worker cannot impersonate either. Record the actual replying worker/session.
Question and reply retries use immutable IDs and exact text. Identical replay
returns the saved record, conflicting reuse fails without overwrite or delivery.

Each decision has an explicit, unique admission-order round index. History and
latest-reply identity follow that durable order, not timestamps, random browser
UUID ordering or physical database row order. A reply attention cycle uses the
exact round ID while retaining the original decision as its source link.
Successful clarification notifications retain one receipt per round/subscription,
bounded by 4,096 retained rounds times eight subscriptions. Foreign-key deletion
prunes these receipts with either owner. Queue acknowledgement cannot cause the
same reply to re-notify; a distinct reply round remains a new attention cycle.

## Ownership, bounds and recovery

- Domain rules own admissibility and next-move meaning. The application owns
  authentication. Persistence owns identity checks, transactions and the outbox.
  HTTP, MCP and browser adapters contain no independent permission policy.
- Limit each question/reply to 4,000 UTF-8 bytes, 32 rounds per decision and
  4,096 retained rounds per Hive (at most 32,768,000 text bytes). Reject capacity
  exhaustion explicitly. Admission may prune rounds whose parent decision has
  been resolved/withdrawn for over 90 days; never prune open-decision evidence.
  Retention is admission-time pruning, not a guaranteed periodic deletion.
- A question and its delivery state commit together with a content-free event.
  Delivery states are queued, dispatching, delivered, uncertain and cancelled;
  neither delivery acknowledgement nor elapsed time means the worker replied.
- The existing exclusive coordinator and guarded terminal submission own
  delivery. Claim at most 16 rows, fence by unique claim and exact session,
  recheck decision/round applicability before writing, and preserve engagement,
  unsent input and provider-readiness guards. No new polling loop or engine update.
- Operator questions wake the existing owned supervisor through a coalescing
  notification (one pending permit), so receipt does not wait on terminal I/O or
  the next scheduled pass. Passes remain serial and graceful shutdown stops new
  admission before joining the current pass; no per-question detached sender.
- Abandoned dispatch claims become uncertain; never automatically resend an
  ambiguous write. Definitive pre-write deferral returns to queued. Explicit
  reconciliation must name the observed claim and acknowledge duplicate risk.
- Operator recovery records either a checked delivery confirmation or an
  acknowledged retry for the exact claim/session. It is operator-authenticated,
  never a worker-token route or automatic loop. The immutable choice is audited
  with the operator identity and timestamp; an identical delayed replay cannot
  requeue a newer attempt. Cap recovery records at eight per question (32,768
  Hive-wide under the existing retention bound), cascading with the question.
  Exhaustion preserves uncertainty and the original final-answer controls.
- Final resolution/withdrawal or an exact reply cancels an unclaimed question.
  Bytes already in flight cannot be recalled: retain the claim's evidence and
  mark late replies historical, never reopen a settled decision.
- A pending unanswered clarification makes the requester the next mover. Needs
  You keeps the exchange accessible under Waiting for a reply, outside the count
  of actionable operator decisions. Queues uses the same authoritative summary.
  A reply returns attention to the operator without replacing the original request.
- Task projections retain every waiting clarification's exact requester and
  delivery state, including shared decision links. A different pending decision
  still needing an operator answer preserves operator ownership. When all pending
  decisions await explanations, all-Queen requesters use Queen ownership;
  mixed or repository requesters use Worker ownership with every requester named.
  This changes attention only, never assignment or pending-decision execution
  gates. Mixed-owner Queen orchestration must be verified before acceptance.
- Inbox lists carry a compact summary; one expanded decision loads its bounded
  exchange history. Do not add a poll per card or copy message text into telemetry.

## Validation and delivery gate

Cover exact duplicate/conflicting retries, wrong worker and stale session,
no-task decisions, parent resolution racing a question/reply/dispatch, interrupted
claims, capacity, retention, transaction rollback and restart reads. Prove that
clarification cannot create any operator resolution or command grant.

Then exercise fictional HTTP -> guarded requester delivery -> MCP reply ->
browser attention -> explicit final answer, including failed-send draft retention,
desktop/phone keyboard and refresh. Expose no production Ask a question control
until this vertical path is connected. A UI mock or persistence tests alone do
not close the requirement. Direct terminal/AskUser answer correlation remains the
separate ADR 0065 gate; do not claim it fixed by this exchange.
