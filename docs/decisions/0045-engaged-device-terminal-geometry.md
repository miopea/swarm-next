# ADR 0045: Bind terminal geometry to the engaged device

## Status

Accepted for dogfooding.

Maturity amendment: [ADR 0062](0062-generation-bound-terminal-control.md)
replaces implicit input takeover with explicit generation-bound control and
atomic geometry transfer. Its implementation is in progress.

## Context

One worker owns one server-side PTY, but the same worker may be visible from a
desktop browser, an installed desktop PWA, and a phone at the same time. Each
browser has different rows and columns. Allowing every attached viewer to
resize the shared PTY makes those viewers fight: Claude redraws for the phone,
then the desktop, then the phone again. The operator sees narrow-column output,
jumping layouts, or a blank terminal even though the provider process remains
healthy.

Viewing a worker is intentionally not operator engagement. Engagement begins
when the operator sends input and is already represented by a durable,
device-owned, bounded lease.

## Decision

Attaching a browser restores canonical terminal output without changing the
server PTY geometry. Each attachment remembers its own latest usable renderer
size locally.

Only the device that owns the current operator-engagement lease may resize the
shared PTY. When a different device sends input, Swarm atomically records that
device as engaged, applies its remembered geometry, and then forwards the
input. Passive desktop and mobile viewers continue receiving the same canonical
output but their ResizeObserver events cannot change the provider process.

Releasing or expiring engagement also removes resize authority. A later input
from any attached device can acquire it again. This rule does not change input
authority, takeover authority, worker ownership, or task state.

## Consequences

- A phone and desktop may observe the same worker without continuously
  reformatting each other's Claude session.
- Geometry follows actual operator interaction instead of whichever browser
  happened to attach or resize last.
- Switching devices produces one deliberate provider resize immediately before
  that device's next input.
- Passive viewers can reflow their local xterm surface without claiming focus
  or interrupting automation.
- Older clients that omit a device identity retain the legacy anonymous
  engagement path, but cannot override a device-owned engagement.

## Verification

- Persistence tests prove that geometry ownership follows the exact unexpired
  engagement device and transfers when another device types.
- A real WebSocket/PTY integration test attaches desktop and phone geometries,
  proves passive attachment and resize are no-ops, then proves each device's
  input transfers authority and applies its own dimensions before the provider
  receives that input.
- Browser acceptance must keep desktop and Android viewers attached to the same
  disposable worker, resize both repeatedly, and confirm canonical output,
  session identity, and provider process identity remain stable.
