# 0042: Serialize resource-aware automatic worker wakes

## Status

Accepted for dogfooding.

## Context

Queen may assign several Ready tasks to sleeping workers before the next
coordination pass. A single normal resource sample previously allowed the
coordinator to claim and start up to eight workers. Starting those providers
changes process-tree memory, but the original batch continued without a fresh
sample. This could turn a safe decision into a memory burst and undermine the
admission policy it was meant to enforce.

## Decision

One coordination pass may claim at most one automatic worker wake. Every other
valid action remains durably queued. The next supervisor pass obtains current
API and worker-engine resource evidence before it may claim the next worker.

The rule applies only to deterministic Queen-originated automatic starts.
Operator-requested wakes remain immediate, resource pressure never stops an
already running worker, and an ambiguous claimed wake is still marked uncertain
rather than replayed.

The coordinator API exposes the batch limit, and Settings explains that Swarm
starts one worker at a time and checks memory again before continuing.

## Consequences

- A large Queen dispatch drains gradually instead of creating a provider-start
  burst.
- Each new automatic start can influence the next resource decision.
- Queued work remains visible and durable during pressure or between passes.
- Automatic fleet startup is slower by design; explicit operator starts are not
  delayed.
