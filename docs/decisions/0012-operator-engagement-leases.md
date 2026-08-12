# ADR 0012: Server-authoritative operator engagement leases

Status: **Accepted**

## Context

Operators frequently steer workers directly without creating tasks. Legacy
coordination could inject messages while the operator was typing, breaking the
active thought and sometimes the worker's context. Merely viewing a terminal
must not reserve it, and UI focus alone is not a reliable cross-device fact.

## Decision

The first non-empty authenticated terminal input creates an exclusive,
server-authoritative engagement lease for that worker. Every input surface uses
the same terminal WebSocket boundary, including desktop typing, paste, mobile
voice composition, slash commands, and D-pad controls.

- A lease lasts five minutes and renews while input continues.
- Durable renewal is throttled until half the lease has elapsed, preventing
  per-keystroke SQLite writes and control-room invalidations.
- Stopping or losing the bound process session removes its lease.
- Coordination asks the worker guard before injecting. A live lease queues the
  request; it never grants additional authorization.
- Viewing output, switching workers, and terminal resize do not engage.
- The roster derives explicit Sleeping, Buzzing, With you, and Blocked states.

Provider waiting-for-input will create the same lease only when a trustworthy
provider event or hook is available. Swarm will not scrape rendered terminal
text to infer that state.

## Consequences

- Direct operator work receives one consistent interruption boundary on mobile
  and desktop.
- The lease survives browser reload and ordinary API replacement because SQLite
  owns it; the browser only renders the canonical expiry.
- At most one engagement row exists per worker, so storage remains bounded.
- Passive expiry needs no background timer. Reads compare the durable expiry to
  current time, while the browser uses the returned expiry for local display.
- Future Queen, Steward, Keeper, and automation messaging must use the same
  injection guard rather than writing directly to a PTY.
