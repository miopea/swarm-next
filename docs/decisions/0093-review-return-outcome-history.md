# ADR 0093: Review returns are measured from exact request and answer records

Status: Accepted under DOG-01; implemented and locally verified, live acceptance pending.

Queen run finishes cannot establish review yield. A returned review leaves the
task in Review and replaces its current next-move marker, so counting task state
changes or the current marker also loses repeated returns.

Record each actual return using its immutable request-message ID in the same
transaction that records the request. Record an answer only when the existing
exact reply-to validation accepts it. Retries of that answer do not create a
second observation. Retain the API build at return, return time and confirmed
answer time; no prompt, answer, task title, path or terminal bytes. This records
follow-up traffic, not proof that the follow-up was useful or the task completed.
Missing answers do not establish a currently pending request: it may have been
superseded, reassigned or removed. Do not invent old episodes from current markers.

Persistence owns at most 4,096 records for 30 days, pruning on admission and read.
The bounded private history must disclose coverage and not treat missing or
clock-inconsistent timing as zero. Build identifies the return, not necessarily
the build that produced the worker's original work or its eventual answer.
No timer, new Queen prompt, message delivery or operator alert is introduced.

Schema 156 uses the existing backup/migration boundary. History admission and
answer updates share the authoritative transaction; failure rolls back the action
rather than acknowledging partial state. Retention never deletes source messages,
changes current ownership, or prevents a valid late answer after history expires.
Downgrade requires a compatible backup. Test repeated returns, exact answer retry,
conflicting/stale answers, failure/recovery, bounds, migration and private reads.

This is the review-return portion of DOG-01, not completion of review quality,
task throughput, duplicate operator question or recovery outcome acceptance.

The private Queen-history response includes a separate `review_returns` page with
its own retained count and the same 100-row read ceiling. The browser reports an
absent field as unavailable during mixed App/API generations, never zero. This
compatibility path is owned by the runtime-history UI and may be removed when the
old pre-156 API is outside the supported App/API update window. Records are
grouped by return build, with sample counts and exact-answer timing; unanswered
observations must not be used as a second task waiting queue.
