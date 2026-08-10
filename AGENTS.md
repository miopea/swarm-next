# Agent development instructions

Swarm Next is an architecture-led replacement, not a mechanical port.

Before changing runtime code, read `docs/00-program-charter.md`,
`docs/04-architecture.md`, and the relevant accepted ADRs.

## Non-negotiable rules

- Do not copy a legacy module without an approved capability decision.
- Describe work in user outcomes and domain behavior, not source parity.
- Keep business rules out of HTTP, WebSocket, database, and UI adapters.
- SQLite is accessed only through the persistence boundary.
- Terminal sessions do not depend on browser component lifetime.
- Every queue, buffer, retry policy, and background task is bounded and owned.
- Correctness cannot depend on arbitrary delays or repeated timers.
- State transitions must be explicit and tested.
- New compatibility paths require an owner and removal condition.
- A vertical slice is not complete without failure and recovery tests.

## Decision discipline

Architecturally significant changes require an ADR in `docs/decisions/`.
Capability status changes must update `docs/01-capability-inventory.md`.
Unresolved product questions belong in `docs/09-open-questions.md`; do not
silently choose behavior that changes the product.

