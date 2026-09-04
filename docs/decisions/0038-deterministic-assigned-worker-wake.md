# ADR 0038: Deterministic assigned-worker wake

## Status

Accepted for dogfooding.

## Context

Queen can assign durable work while its repository worker is sleeping, but a
stable assignment alone did not start that worker. The next brief therefore
waited for an operator to choose **Wake worker**, preventing useful unattended
coordination. Asking Queen to notice and wake the worker would spend another
model turn on a policy-complete mechanical action.

Automatically waking every operator assignment would be surprising and would
claim authority the operator did not grant. Retrying a crash-ambiguous process
start could also create duplicate provider sessions.

## Decision

A Queen-originated assignment of a **Ready** task to a sleeping worker creates
one durable coordinator action in the same database transaction. Its
idempotency key includes the exact assignment activity revision. The
deterministic coordinator claims a bounded batch, starts the repository-owning
worker, binds her immutable session, and lets the existing guarded task-brief
outbox deliver the work.

The rule does not apply to operator-originated assignments. A changed task,
assignment, or already-running worker cancels a queued action before execution.
A crash or transport failure after claim becomes **uncertain** and is never
silently replayed. The operator UI reports completed actions, avoided Queen
calls, queued actions, and uncertainty without exposing task content.

## Consequences

The approved daily-driver maturity policy excludes experimental providers from
Night Watch wake admission. Filter before the bounded claim so deferred work
stays queued and eligible work can pass it. Leaving Night Watch makes deferred
work eligible again under the existing assignment and resource guards. Promotion
is builder-owned: the existing non-alpha Claude and Codex catalog is retained;
Gemini, Grok, OpenCode, and unknown stored providers are not admitted. Installation
or model availability cannot promote a provider. This wake boundary does not by
itself enforce the policy on other startup or delivery paths.

- Queen can assign local work overnight without a second model call or operator
  wake action.
- Sleeping workers remain unloaded until Queen gives them Ready work.
- Operator-directed assignments preserve manual lifecycle control.
- The first coordinator rule is narrow and auditable; additional legacy-drone
  outcomes require their own typed preconditions and evidence.
- Resource-pressure admission and active-task crash recovery remain separate
  follow-on policies.
