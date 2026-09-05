# ADR 0074: Withdraw obsolete operator requests without approving them

Status: Accepted under ATT-01 and QUEEN-01.

## Decision

A requesting worker can withdraw its own pending request with a nonempty,
bounded explanation. Queen may also withdraw a pending Hive request after
checking that operator judgment is no longer needed. This is a distinct
Withdrawn state, never an operator resolution, answer, dismissal or grant.
Resolved decisions cannot be withdrawn or rewritten by agents.

Persist withdrawal actor, explanation and time atomically with DecisionsChanged
and TasksChanged events. Withdrawal creates no decision delivery or command
grant. A same-actor, same-reason retry is idempotent; different text or actor
cannot overwrite history. The decision leaves pending counts, attention and
push eligibility immediately on the next authoritative read. Its history
remains inspectable and clearly says withdrawn, not approved.

The application authenticates a live agent and persistence rechecks requester
or Queen authority in the local Hive. Ordinary workers cannot withdraw a peer's
request merely because they inherit its task. No inference from elapsed time,
task completion alone, or arbitrary terminal text withdraws a decision.

Schema 137 rebuilds the existing constrained decision table with the third state,
preserving data, indexes and references through the normal migration boundary.
Additive withdrawal metadata records the history. No new retry loop or queue.

## Verification gates

Migration with pending/resolved requests and child records; atomic failure and
reopen; authorization, duplicate and conflicting retry; reject resolved requests;
no fabricated operator action, delivery or grant; pending counts and task owner
change together; MCP role and full-ID verification; browser inbox/history labels;
real demo Queen withdrawal. The feature is not accepted merely because its
persistence slice passes.
