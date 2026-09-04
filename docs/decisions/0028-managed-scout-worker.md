# ADR 0028: Managed Scout worker identity

Status: **Accepted**

## Context

Queen needs a safe way to delegate deliberate cross-repository preparation and
worktree setup. Treating the existing project-root worker as an ordinary roster
entry makes that capability easy to rename, remove, or confuse with a
repository owner. Giving it Queen authority would instead blur the clean
operator → Queen → worker hierarchy.

Existing dogfood installations may already have a durable `Project Root`
worker with a valuable provider conversation and task history. Replacing it
would discard exactly the continuity Swarm promises.

## Decision

Swarm records **Scout** as a managed worker system role separate from its
authorization role. Scout remains an ordinary worker for task and coordination
authority. The system role only protects its durable product identity and
presentation.

On startup, Swarm may promote an exact, active `Project Root` worker at the
configured projects root. Promotion:

- keeps the worker ID, provider conversation, terminal history, task history,
  description, and provider;
- changes its display name to `Scout` and marks the managed system role;
- leaves it sleeping by default rather than consuming provider memory;
- pins it directly after Queen in the roster; and
- prevents rename, removal, and ordinary drag ordering while continuing to
  allow provider, description, and always-active policy changes.

Promotion fails safely when the exact worker and workspace do not match. It
does not guess from similar names or create a duplicate. Fresh-install Scout
provisioning remains a separately tested installation concern.

## Consequences

- Existing operators keep continuity while gaining a recognizable protected
  cross-repository worker.
- Queen can delegate broader preparation without granting Scout Queen powers.
- Sleeping Scout adds no loaded provider process or terminal-memory cost.
- The managed marker is explicit in persistence and API contracts rather than
  inferred forever from a mutable name.

## Validation

### Queen routing evidence (daily-driver maturity)

The Queen-only MCP roster includes `scout_routing`: a single-query durable
snapshot of the managed Scout ID, open session binding, operator engagement
at the supplied observation time, and active-task ownership. A worker merely
named Scout does not qualify. Missing managed identity is explicit null.
The existing worker array remains unchanged.

This snapshot is neither a reservation nor a terminal-idle observation. It
explicitly reports terminal activity as unobserved and grants no dispatch
authority. Read failure returns an error rather than an optimistic idle state.
Delivery must still revalidate engagement and live terminal evidence, and
second-opinion admission must protect ongoing tasks. This projection does not
complete that separate enforcement work.

Persistence tests prove exact-only in-place promotion, stable identity and
conversation, sleeping default, pinned ordering, protected rename/removal, and
continued provider/description configuration. API and browser tests expose and
render the managed role without treating it as Queen.
