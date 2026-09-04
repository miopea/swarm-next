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

Cover migration without fabricated links, atomic save/event failures, exact
duplicate and conflicting replies, superseded requests, changed assignment,
ordinary messages, role isolation, and a real MCP hand-back/answer round trip.
No native terminal transcript inference or semantic answer matching is added.
