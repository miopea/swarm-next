# ADR 0066: A review answer names the request it answers

Status: Accepted implementation decision under approved QUEEN-01 and QUEUE-01.

## Context

Handing Review work back now commits its request and next-move owner atomically.
The advertised return leg is not wired: the worker's message to Queen never marks
the review answered. Treating every message as an answer would hide unresolved
requests when the worker merely asks a clarification or reports progress.

## Decision

Use the immutable task-message ID as the review-request identity. Schema 133
links each current returned-review marker to its request message, assigned worker,
and optional answer message. A hand-back returns that request ID. Worker replies
may explicitly name it; ordinary messages do not change review ownership.

The application service authenticates the worker. Persistence atomically checks
the current task assignment and Review state, exact current request ID, and the
request's worker before saving the answer and returning the next move to Queen.
It publishes the durable Tasks Changed event in the same transaction. An exact
retry returns the saved answer without another delivery; conflicting text,
superseded requests, or another worker cannot overwrite it. This acknowledges a
worker answer, not successful task completion, operator approval, or delivery.

Request and answer text retain the existing 4,000-byte task-message limit. There
is one current marker per task; no polling loop, timer, or new unbounded queue is
introduced. The existing safe-prompt message delivery remains the transport owner.

Historical markers have no authenticated request identity. Do not infer links
from timestamps or text. Queen can reissue a specific hand-back to establish one.
The API owns this nullable historical compatibility, removable only after all
legacy open markers are explicitly answered, replaced, or retired with their task.
Task history exposes the current request ID, request worker, saved answer ID,
and status (awaiting_answer, answered, superseded, or legacy_unlinked). Queues
assigns a returned-review obligation to a worker only when the linked request
belongs to the current assignee; unlinked or differently assigned requests remain
Queen's coordination responsibility, not a debt inherited by an uninformed worker.
Reassignment to another worker, unassignment, and leaving Review invalidate an
unanswered request in the same transaction by clearing its current request-worker
binding. Its immutable message ID and message recipient remain historical evidence.
A linked message without that current binding is superseded, not legacy-unlinked.
Returning to the earlier worker or Review state cannot revive it; Queen must issue
a new request. A same-worker rebind alone preserves the current question.
Existing provider sessions need their normal tool refresh to use the optional
reply selector; ordinary messaging stays available without falsely settling work.

## Verification

The task read projection includes the current request ID and exact bounded
message text only while the task remains in Review, unanswered, and bound to
the current assignee. Queues can therefore show what Queen requested without
another per-row fetch or inferring a question from message order. Answering,
reassigning, superseding or leaving Review removes/replaces the projection on
the same subsequent task read. The immutable message remains in task history.
This read adds no delivery claim and does not settle the task.

Cover migration without fabricated links, atomic save/event failures, exact
duplicate and conflicting replies, superseded requests, changed assignment,
ordinary messages, role isolation, and a real MCP hand-back/answer round trip.
No native terminal transcript inference or semantic answer matching is added.

September 7 live inspection found that task-message delivery still advertised
an ordinary worker report without the reply selector. A worker reported fixes
and said it re-submitted, while the exact review request remained unanswered.
Every delivered message now includes its own immutable identity, not only the
first batching marker. Queen-to-worker messages explain conditional use of that
identity as `reply_to_message_id`: verify it is the current returned-review
request, and omit it for progress, clarification or a superseded request. This
changes presentation of the existing contract, not authorization, semantic
matching or the atomic persistence transition. Live acceptance remains required.

### September 7 live acceptance in progress

The clarified delivery text passed all 47 guarded-delivery tests and strict API
checks. Main `e1d98e0a` is serving as
`e1d98e0a4b94-20260907183121-205234`, healthy with the prior engine identity.
Formatting-only follow-up `a1362674` is in the Linux clone and passes full-tree
`cargo fmt --all --check`; full CI run `34152143722` subsequently passed.

Fictional task `01a07d29-2c1f-7642-a82d-736d17fd6adb`, titled
"Dogfood e1d98e0a: exact returned-review answer", runs only in the existing demo
repository on Swarm Dogfood. Setup verified an idle, unengaged worker with no
open assigned task, refused duplicate titles, and used normal creation, Ready
transition and assignment. The worker started and submitted Review truthfully
leaving its local Node execution verification outstanding. At this checkpoint
the task is Queen-owned with no returned-review identity yet; Queen review
`01a07d27-dfeb-7281-b9f5-26f0543b2519` is actively running.

Required next evidence: Queen returns this exact task; an ordinary progress
message preserves worker ownership; the final answer names the exact request
and returns ownership atomically without a Review-to-Review transition; the
task then settles normally on truthful evidence. No controller-forced hand-back,
answer, completion or real-project mutation is part of this fixture. Do not
declare acceptance from the phase-one Review state alone.

### September 7: fixture failed on automatic settlement

Authoritative activity now shows system completion at sequence 7918
(`1788806698`), followed by the worker's explicit failure report at sequence
7919. The worker recorded an empty commit report before sending its correlated
answer. The no-deployment settlement sweep completed the task, superseding the
unanswered review request. The worker reported nine passing Node tests and a
clean unchanged HEAD, but correctly did not call the review round trip a pass.
No controller forced this completion. Preserve this completed fixture as failure
evidence rather than resetting it or counting it as successful acceptance.

Inspection locates the bypass in
`settle_reviewed_work_without_deployment_page`: candidate eligibility checks
prerequisites and outstanding email replies, but not the current returned-review
obligation. Its completion transition then invalidates that obligation. A fix
must preserve routine no-build/docs-only bypass when there is no outstanding
request, protect an unanswered request against the automatic path (including
selection/completion races), and recover automatically after an exact answer.
The separate whole-task deployment policy explicitly preserves an earlier
operator decision to close over reviewer holds; do not silently change that
policy as part of the no-deployment fix.

The added batched-delivery regression passed independently: two request messages
retain distinct reply selectors and the first transport marker, with the submit
terminator preserved. This verifies delivery text, not the failed settlement
boundary above.
