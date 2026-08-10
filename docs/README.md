# Swarm Next definition set

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
