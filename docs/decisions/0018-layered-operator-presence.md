# ADR-0018: Layered, server-authoritative operator presence

Status: Accepted

## Context

Swarm Next needs to route attention differently when the operator is actively
working at the Hive, away from the computer, or intentionally running a Night
Watch. A browser tab alone cannot authoritatively answer that question: tabs
may be hidden or suspended, devices can disconnect, and operating-system lock
signals are permission-gated and browser-specific.

Presence affects interruption and notification policy. It must not become an
authorization signal, a permanent activity log, or a timer-driven source of
business truth.

## Decision

Swarm Next derives one effective operator-presence mode from bounded,
expiring per-device observations plus an optional explicit override.

- Effective modes are At Hive, Away, and Night Watch.
- Automatic mode prefers a current active observation, then a current locked or
  idle observation, and otherwise resolves to Away after observations expire.
- An explicit At Hive, Away, or Night Watch override wins until the operator
  returns to Automatic.
- Each device reports only its stable random device ID, class, observation
  state, and expiry. No activity content, window title, or keystrokes are
  retained.
- Device observations are capped at 16 per Hive. Expired records are pruned
  before admitting a replacement.
- The browser uses one 60-second heartbeat. Active observations expire after
  150 seconds; hidden, idle, and locked observations use bounded state-specific
  leases.
- Chromium Idle Detection may improve locked and idle classification only after
  an explicit operator permission action. Lack or denial of that capability is
  a supported state, not a failure.
- A content-free `presence_changed` event is emitted only when effective
  presence changes. Repeated heartbeats do not churn the control-room feed.
- Presence tunes attention routing and future notification delivery. It never
  grants control, authenticates a device, or overrides the operator-engagement
  lease in ADR-0012.
- Night Watch is initially an explicit mode. Scheduling and lock-driven policy
  may be added later without changing the stored presence contract.

## Consequences

Desktop and mobile clients share one durable, privacy-minimal view of operator
availability. Temporary network loss degrades safely to Away, manual intent is
preserved, and optional platform capabilities can improve the experience
without making Chromium a required architecture layer.

Push transport remains a separate step. A future service worker may handle
push display and notification navigation only; it must not cache application
assets or become presence authority.

## Validation

- Migration tests prove upgrades through schema 14 and the bounded tables.
- Persistence tests cover precedence, expiry, capacity, and event de-duplication.
- HTTP tests cover authentication, observations, manual overrides, and reads.
- Browser tests cover single-owner heartbeat cleanup, bounded in-flight writes,
  visibility changes, lock-capability fallback, and settings controls.