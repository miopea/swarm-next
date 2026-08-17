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
- The authenticated browser device that most recently supplied input owns the
  lease. Selecting a different worker asks the server to release the previous
  lease only when that same device still owns it.
- A release caused by worker selection is explicit and idempotent. Browser
  reload, connection loss, and API replacement do not release the lease; they
  retain the five-minute recovery boundary.
- Stopping or losing the bound process session removes its lease.
- Coordination asks the worker guard before injecting. A live lease queues the
  request; it never grants additional authorization.
- Viewing output, switching workers, and terminal resize do not engage.
- Selecting a new worker therefore clears `With you` from the old worker but
  does not apply it to the new worker until the operator supplies input.
- Terminal geometry authority is tracked separately on the live worker
  session. It remains with the device that most recently supplied input after
  the attention lease expires or is explicitly released, so returning to that
  worker can refit its PTY without falsely restoring `With you`. Input from a
  different device atomically transfers both the attention lease and geometry
  authority. A passive viewer never takes either authority merely by attaching.
- The roster derives explicit Sleeping, Buzzing, With you, and Blocked states.

Provider waiting-for-input will create the same lease only when a trustworthy
provider event or hook is available. Swarm will not scrape rendered terminal
text to infer that state.

## Consequences

- Direct operator work receives one consistent interruption boundary on mobile
  and desktop.
- The lease survives browser reload and ordinary API replacement because SQLite
  owns it; the browser only renders the canonical expiry.
- A phone, desktop, or other browser device cannot release a lease after a
  different device has taken ownership.
- At most one engagement row exists per worker, so storage remains bounded.
- Passive expiry needs no background timer. Reads compare the durable expiry to
  current time, while the browser uses the returned expiry for local display.
- Future Queen, Steward, Keeper, and automation messaging must use the same
  injection guard rather than writing directly to a PTY.

## Rolling compatibility

The terminal resume message accepts an absent device identity during the first
rolling deployment of this change. Such older clients retain expiry-only
leases and cannot explicitly release them. Remove this optional compatibility
path after all supported clients have shipped device identity.
