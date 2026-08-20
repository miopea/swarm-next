# Swarm definition set

Read and review these documents in order:

1. [Program charter](00-program-charter.md)
2. [Capability inventory](01-capability-inventory.md)
3. [Product journeys](02-product-journeys.md)
4. [Domain model](03-domain-model.md)
5. [Target architecture](04-architecture.md)
6. [Terminal-session model](05-terminal-session-model.md)
7. [Walking skeleton](06-walking-skeleton.md)
8. [Dogfooding and cutover](07-dogfooding-and-cutover.md)
9. [Quality and acceptance](08-quality-and-acceptance.md)
10. [Open product questions](09-open-questions.md)
11. [M0 evidence review](10-m0-evidence-review.md)
12. [M1 terminal-host foundation](11-terminal-host-foundation.md)
13. [M1 browser terminal attachment](12-browser-terminal-attachment.md)
14. [M1 canonical terminal recovery](13-canonical-terminal-recovery.md)
15. [M1 durable terminal history](14-durable-terminal-history.md)
16. [M1 terminal-host update lifecycle](15-terminal-host-update-lifecycle.md)
17. [M1 swarmctl lifecycle client](16-swarmctl-lifecycle-client.md)
18. [ADR 0009: unprivileged systemd user package lifecycle](decisions/0009-systemd-user-package-lifecycle.md)
19. [M1 two-worker soak gate](17-two-worker-soak.md)
20. [M1 minimal task workflow](18-minimal-task-workflow.md)
21. [Visual design system](19-visual-design-system.md)
22. [Daily-driver readiness](20-daily-driver-readiness.md)
23. [Apiary and Hive product contract](21-apiary-hive-product-contract.md)
24. [Live control-room events](22-live-control-room-events.md)
25. [Privacy-safe diagnostics](23-privacy-safe-diagnostics.md)
26. [Browser dogfood acceptance](24-browser-dogfood-acceptance.md)
27. [Component and engineering-principles audit](25-component-and-engineering-audit.md)
28. [Legacy Swarm evolution atlas](26-legacy-evolution-atlas.md)
29. [Legacy final product contract audit](legacy/final-contract-audit.md)
30. [Legacy stable release boundaries](legacy/stable-release-boundaries.md)
31. [Legacy atlas completion audit](legacy/atlas-completion-audit.md)
32. [Ring 1 observation log](legacy/ring1-observation-log.md)
33. [Transparent developer guidance](decisions/0047-transparent-developer-guidance.md)
34. [Worker context surfaces](29-worker-context-surfaces.md)

`decisions/` contains architecture decision records. `legacy/` contains
measured facts about the existing system. Legacy facts inform decisions but do
not automatically become requirements.

## Decision labels

- **Accepted**: approved direction and safe to implement.
- **Proposed**: recommended, awaiting explicit review.
- **Investigate**: evidence is incomplete.
- **Rejected**: considered and intentionally not selected.

The definition set is expected to evolve during M0. Once runtime development
starts, accepted architectural invariants change only through an ADR.

Current decision records:

- [ADR 0001: Architecture-first replacement](decisions/0001-architecture-first-replacement.md)
- [ADR 0002: Proposed technology direction](decisions/0002-proposed-technology-direction.md)
- [ADR 0003: Provider-owned terminal permissions](decisions/0003-provider-owned-permissions.md)
- [ADR 0004: Replaceable browser presentation adapter](decisions/0004-replaceable-browser-adapter.md)
- [ADR 0005: Independent terminal-host process](decisions/0005-independent-terminal-host.md)
- [ADR 0006: Host-owned canonical terminal snapshots](decisions/0006-canonical-terminal-snapshots.md)
- [ADR 0007: Host-owned bounded durable terminal history](decisions/0007-host-owned-durable-terminal-history.md)
- [ADR 0008: Drain-compatible terminal-host updates](decisions/0008-drain-compatible-terminal-host-updates.md)
- [ADR 0009: Unprivileged systemd user package lifecycle](decisions/0009-systemd-user-package-lifecycle.md)
- [ADR 0010: Apiary federation and stewardship](decisions/0010-apiary-federation-and-stewardship.md)
- [ADR 0011: Durable provider conversation recovery](decisions/0011-provider-conversation-recovery.md)
- [ADR 0012: Server-authoritative operator engagement leases](decisions/0012-operator-engagement-leases.md)
- [ADR 0013: Scoped agent application bridge](decisions/0013-scoped-agent-application-bridge.md)
- [ADR 0014: Unified operator decision inbox](decisions/0014-unified-operator-decision-inbox.md)
- [ADR 0015: Guarded durable decision outcome delivery](decisions/0015-guarded-decision-outcome-delivery.md)
- [ADR 0016: Guarded durable task assignment dispatch](decisions/0016-guarded-task-assignment-dispatch.md)
- [ADR 0017: Guarded worker outcomes to Queen](decisions/0017-guarded-worker-outcomes-to-queen.md)
- [ADR 0018: Layered operator presence](decisions/0018-layered-operator-presence.md)
- [ADR 0019: Policy-driven mobile attention](decisions/0019-policy-driven-mobile-attention.md)
- [ADR 0020: Owned runtime resource evidence](decisions/0020-owned-runtime-resource-evidence.md)
- [ADR 0023: Jira canonical sync boundary](decisions/0023-jira-canonical-sync-boundary.md)
- [ADR 0024: Provider-derived worker attention](decisions/0024-provider-derived-worker-attention.md)
- [ADR 0025: Authenticated federation bootstrap](decisions/0025-authenticated-federation-bootstrap.md)
- [ADR 0031: Retry-safe Apiary departure](decisions/0031-retry-safe-apiary-departure.md)
- [ADR 0032: Confirmed Jira claim handoff](decisions/0032-confirmed-jira-claim-handoff.md)
- [ADR 0037: Bounded Queen conductor](decisions/0037-bounded-queen-conductor.md)
- [ADR 0038: Deterministic assigned-worker wake](decisions/0038-deterministic-assigned-worker-wake.md)
- [ADR 0039: Deterministic stale-owned-work attention](decisions/0039-deterministic-stale-owned-work-attention.md)
- [ADR 0040: Resource-pressure admission for automatic worker starts](decisions/0040-resource-pressure-admission-for-automatic-starts.md)
- [ADR 0041: Deterministic exited-worker owned-work attention](decisions/0041-deterministic-exited-worker-owned-work-attention.md)
- [ADR 0042: Serialized resource-aware worker wakes](decisions/0042-serialized-resource-aware-worker-wakes.md)
- [ADR 0043: Loaded worker before Active work](decisions/0043-loaded-worker-before-active.md)
- [ADR 0044: Delivered Ready work attention](decisions/0044-delivered-ready-work-attention.md)
- [ADR 0045: Engaged-device terminal geometry](decisions/0045-engaged-device-terminal-geometry.md)
- [ADR 0047: Transparent developer guidance](decisions/0047-transparent-developer-guidance.md)
- [ADR 0048: Workers use the default Claude configuration location](decisions/0048-default-claude-configuration-location.md)
