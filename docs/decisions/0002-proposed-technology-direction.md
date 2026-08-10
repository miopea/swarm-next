# ADR-0002: Rust backend and React frontend

Status: **Proposed**

## Context

Swarm combines a persistent process/terminal supervisor with a web control
plane, durable task state, integrations, and agent-facing protocols. The target
must improve process ownership, recovery, resource bounds, frontend lifecycle,
and long-term architectural clarity without creating three permanent product
runtimes.

## Decision

Use a modular Rust application backend, React/TypeScript browser frontend, and
SQLite embedded persistence. Avoid a permanent Python or Node application
backend. Internally isolate terminal supervision only when lifecycle or
security requirements justify a child process.

## Consequences

- Final production code uses two programming ecosystems.
- Rust owns application and terminal state; TypeScript owns presentation.
- npm remains available to the frontend, while backend integrations use Rust
  crates or direct protocols.
- A stronger verification program is required for cross-platform PTY behavior.
- Generated API contracts connect Rust and TypeScript.

## Alternatives considered

- TypeScript control plane plus Rust terminal host: strong product-velocity
  option, rejected provisionally because the desired end state favors one
  backend runtime and tighter state ownership.
- All TypeScript with native PTY dependency: simpler staffing, but weaker fit
  for descriptor survival and process supervision.
- Rust holder plus permanent Python daemon: safest incremental change, but
  retains the architectural ceiling and a third ecosystem after React.

## Validation

Approve only after reviewing the capability inventory and walking skeleton.
The walking skeleton must demonstrate terminal recovery, developer workflow,
integration-test ergonomics, and acceptable implementation velocity.

