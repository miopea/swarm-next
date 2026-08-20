# ADR 0003: Provider-owned terminal permissions

Status: **Accepted**

## Context

Legacy Swarm observes terminal prompts and applies a growing rules engine to
approve routine provider actions. Modern providers now expose native permission
modes, allow/deny policies, sandboxes, and explicit approval interfaces. A
second approval authority creates races, stale interpretations, ambiguous
audit ownership, and a broad path from terminal text to command execution.

Swarm still has product-level decisions that providers cannot own: assigning
work, approving a deployment or external side effect, changing resource
budgets, and accepting a generated plan.

## Decision

The provider adapter owns provider tool permissions. Swarm will configure
and report the provider's declared permission posture but will not scrape a
terminal prompt, maintain regex approval rules, or inject approval keystrokes.

Swarm owns a separate typed operator-decision protocol for product actions. A
decision records its actor, scope, expiry, result, and correlated operation.
Provider prompts may be surfaced as provider events when an official interface
exists, but they do not become Swarm's independent authorization engine.

## Consequences

- Routine approval drones and auto-tuning rules are not ported.
- Security policy has one authority at each layer.
- Provider adapters must expose permission posture and capability detection.
- Unsupported provider behavior fails closed and remains visible to the
  operator.
- Swarm-level decisions can be tested without parsing terminal presentation.

## Revisit condition

Revisit only if a required provider lacks a safe native permission mechanism
and an explicit operator journey cannot be served without mediation. A new ADR
and threat model are required before adding such mediation.
