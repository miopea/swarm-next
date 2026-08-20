# ADR 0040: Resource-pressure admission for automatic worker starts

## Status

Accepted for dogfooding.

## Context

Queen may assign Ready work to a sleeping worker while the operator is away.
The deterministic coordinator can wake that owner without spending another
model call, but starting a provider process increases memory demand. Doing so
while the machine or worker engine is already under pressure can harm active
work and turn an otherwise recoverable condition into an out-of-memory crash.

Legacy Swarm reacted after pressure rose by suppressing workers. That behavior
needed hysteresis, could make state difficult to interpret, and did not undo
the cost of processes that were already running. Swarm has enough typed
evidence to make a narrower decision before it claims an automatic wake.

## Decision

Before claiming any queued `wake_assigned_worker` action, the deterministic
coordinator samples:

- host memory use and Linux memory PSI when available; and
- the terminal host's owned process-tree memory.

The process-tree admission threshold is deliberately separate from the lower
diagnostic "watch" threshold: a normal loaded provider commonly uses hundreds
of MiB. Automatic starts become advisory at 2 GiB of owned provider processes
and critical at 4 GiB, while machine-wide memory use and PSI can defer earlier.

Critical evidence takes precedence over advisory evidence. Either level
defers all automatic worker starts and leaves their durable actions queued.
An unreachable terminal host also defers the claim, so a transient connection
failure cannot convert an unattempted wake into an uncertain action. On hosts
without machine-wide evidence, a reachable terminal host with normal
process-tree pressure is sufficient.

This admission rule applies only to deterministic unattended wakes. It does
not stop or suspend a running process, inject terminal input, mutate a task or
Jira, or prevent an operator from explicitly waking a worker. Queen and
operator-configured autostart recovery remain separate lifecycle contracts.

The latest admission state is content-free and visible beside the deterministic
coordinator metrics as allowed, deferred for advisory or critical pressure, or
deferred while worker-engine evidence is unavailable.

## Consequences

- Pressure cannot turn an unattempted automatic wake into ambiguous execution;
  the exact queued action is reconsidered on the next coordinator pass.
- Existing active work is never interrupted by this rule.
- Explicit operator intent remains authoritative even when automatic starts
  are deferred.
- Machine-wide sampling is currently Linux-specific; other supported hosts use
  the owned terminal process tree rather than being permanently blocked.
- Pausing configured autostart recovery, killing workers, and automatic memory
  reclamation remain separate policies requiring their own evidence.
