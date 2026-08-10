# ADR 0004: Replaceable browser presentation adapter

Status: **Accepted**

## Context

AI-assisted delivery reduces the value of choosing a language solely because
the current team already knows it. It increases the value of explicit
contracts, compiler feedback, mature diagnostics, and boundaries that prevent
generated code from coupling unrelated lifecycles.

Swarm needs a rich browser control room and an imperative terminal renderer.
The browser ecosystem and xterm.js are native to JavaScript. A Rust/WASM UI
would still cross a JavaScript bridge for terminal and some browser behavior,
while making ordinary browser diagnosis and package integration less direct.

## Decision

TypeScript is the browser presentation adapter. React is the current view
renderer inside that adapter; it is not an architectural owner.

React components may attach to terminal controllers, but they do not own:

- provider or worker processes;
- terminal sessions or canonical terminal state;
- terminal WebSocket connections or replay cursors;
- xterm instances or their durable lifecycle;
- committed terminal dimensions.

Those resources live behind framework-independent TypeScript controllers and
versioned Rust contracts. Component cleanup detaches a view. Only an explicit
session-lifecycle operation disposes its controller.

## Consequences

- React can be upgraded or replaced without changing backend domain modules,
  terminal protocols, or persistence.
- React remounts, route changes, and Strict Mode cannot restart sessions.
- The terminal controller receives direct lifecycle, resource-bound, and
  attach/detach tests outside component tests.
- UI business rules remain in typed application models rather than components.
- A framework replacement must preserve accessibility, browser diagnostics,
  and the terminal-controller contract.

## Revisit condition

Revisit React when measured product or maintenance evidence shows the renderer
is the constraint. Familiarity or novelty alone is not sufficient evidence.
