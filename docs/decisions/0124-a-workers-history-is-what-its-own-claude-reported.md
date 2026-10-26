# 0124 — A worker's conversation history is what its own Claude reported

- Status: Accepted
- Date: 2026-10-25
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-25.
  Asked how to clear "Newer conversation history" on Scout: "if I like the chat
  I am in, I want to continue." Offered counting only the conversations a
  worker's own Claude reported, or trusting a confirmed resume, they chose the
  first.
- Builds on: ADR 0064 (provider session start evidence), ADR 0078 (explicit
  conversation choice), ADR 0101 (provider-reported conversation changes).

## Context

The freshness check reads every transcript in a worker's Claude project
directory and reports the worker stale when one is newer than its saved
conversation. That directory belongs to a workspace, not to a worker: any Claude
started in the same directory writes its transcript there.

Scout's workspace is all of `~/projects`. On 2026-10-25 Scout was running the
conversation the operator wanted, and the card said it would resume an older
one. The "newer" transcripts were throwaway probes the Swarm Next worker had
run in `~/projects` on 2026-10-24 while measuring Claude's hooks for ADR 0123: "Reply with only the word PINEAPPLE." and a
700-word story. None was Scout's.

ADR 0101 made the engine report every conversation change it can see (start,
resume, clear, fork, compact), and the saved conversation follows those
reports. So a worker's own history is already known to Swarm. The scan was
treating strangers as the worker.

## Decision

Swarm records each conversation a worker's own sessions report, at startup and
on every later selection, whether or not the saved conversation follows it.
Only those can outrank the saved conversation.

- One table, `worker_reported_conversations` (schema 210), keeps the newest 32
  per worker. It is written only when a new id appears, so the supervisor pass
  that reads reports costs one query when nothing changed.
- Nothing is backfilled. Until a worker reports a conversation, nothing in its
  directory can outrank its saved one.
- ADR 0078 is unchanged: restoring an old saved conversation is still not proof
  of intent. This decides whose transcripts count, not which one is wanted.

## Consequences

- Probes, the operator's own terminals and other tools working in a worker's
  directory no longer raise the card.
- A conversation change Claude never reports is no longer caught by the scan.
  ADR 0101 already names this gap.
- A worker on an engine that does not report selections records nothing, so the
  card stays quiet for it.
