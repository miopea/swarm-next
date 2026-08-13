# ADR 0024: Derive worker attention from canonical provider output

## Status

Accepted

## Context

The roster is an operator's primary fleet overview. A live process is not necessarily doing work: it may be idle at a provider prompt, actively generating, or waiting for an operator decision. Treating every live process as `Buzzing` makes the roster operationally misleading and encourages unnecessary terminal inspection.

The terminal host already owns the bounded canonical terminal surface. That surface is the closest durable evidence of provider activity without scraping browser renderers, retaining a second unbounded transcript, or asking each web client to poll terminals.

## Decision

Swarm derives provider activity from the host-owned canonical terminal snapshot through provider-specific classifiers:

- `Sleeping`: no live terminal process is attached to the durable worker.
- `Resting`: the provider process is live and idle at its prompt.
- `Buzzing`: the provider is actively working or its current output cannot safely be classified as idle.
- `Awaiting operator`: the provider is showing a confirmation or choice that requires operator input.
- `With operator`: an active operator engagement lease overrides provider activity.
- `Blocked`: a runtime failure overrides every other state.

Classifiers operate on the bounded visible terminal surface and are covered by provider-fixture tests. Raw terminal output is never copied into control-room events or persistence. The existing bounded worker supervisor observes all live sessions and emits a content-free runtime event only when the classified activity map changes. Direct roster reads also refresh the observation, so explicit refresh remains authoritative.

## Consequences

The operator can distinguish loaded-but-idle workers from unloaded workers and safely keep a complete durable roster. Provider UI changes may require classifier fixture updates, but unknown live states fail conservatively to `Buzzing`. Status propagation uses the existing control-room feed and supervisor cadence rather than introducing browser polling or per-session unbounded state.
