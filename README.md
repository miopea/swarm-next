# Swarm Next

Swarm Next is a ground-up redesign of Swarm: a persistent control room for AI
coding agents. It preserves proven user outcomes while replacing accidental
architecture, obsolete automation, and implementation-driven product behavior.

This repository begins with product and architecture discovery. Runtime code is
intentionally deferred until the initial capability decisions, user journeys,
domain model, and walking-skeleton acceptance criteria are approved.

## Intended product qualities

- Agent sessions outlive browsers, UI components, and application updates.
- Switching terminals feels like switching editor tabs: immediate and stable.
- Reload, sleep, reconnect, and update are routine recovery paths.
- Every queue, buffer, and retained history has an explicit bound.
- Core state transitions are typed, transactional, observable, and testable.
- Integrations extend the product through declared application interfaces.
- Operators install, run, update, and diagnose one application.

## Proposed implementation direction

- Rust modular monolith for the application and terminal/session backend.
- React and TypeScript for the browser application.
- SQLite as the embedded source of truth, owned by one persistence boundary.
- Versioned HTTP, event, and terminal synchronization contracts.

These are proposed decisions until accepted through the architecture review.
See [docs/README.md](docs/README.md) for the review sequence.

## Relationship to legacy Swarm

The legacy `miopea/swarm` repository remains the stable daily driver and an
executable source of product evidence. Swarm Next does not port a module merely
because it exists. Each capability is classified as keep, redesign, merge, or
remove before implementation.

## Current milestone

**M0: Product and architecture definition**

M0 exits only when the capability inventory, critical journeys, architectural
constitution, terminal-session model, walking skeleton, and dogfooding plan
have been reviewed and approved.

