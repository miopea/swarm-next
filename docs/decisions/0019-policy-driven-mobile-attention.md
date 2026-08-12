# ADR-0019: Policy-driven, durable mobile attention

Status: Accepted

## Context

Mobile is a first-class Swarm Next surface. The operator needs important Queen
and worker decisions to follow her when she is away without turning ordinary
Hive activity into notification noise or exposing development context on a
lock screen. Browser push endpoints are untrusted input, delivery is fallible,
and an earlier broad service worker contributed to browser-process memory
instability in legacy Swarm.

## Decision

Swarm Next uses Web Push as a narrow delivery adapter for the unified Needs you
inbox.

- Notifications are eligible only while effective presence is Away or Night
  Watch. They are suppressed while the operator is At the Hive.
- The default policy sends only time-sensitive decisions. Operators may choose
  every pending decision or disable delivery.
- Device registration always follows an explicit browser permission action.
  Startup never registers a service worker or requests notification permission.
- Push payloads are encrypted and content-free: generic title/body, a stable
  replacement tag, urgency, and a navigation target. Repository names, task
  titles, evidence, credentials, terminal output, and decision details never
  enter the payload.
- One installation-scoped ES256 VAPID key pair is generated once and stored in
  the local private state database. The browser receives only its public key.
- Subscriptions are capped at 8 and deliveries at 128. A durable outbox claims
  at most 8 sends at a time, recovers interrupted claims, retries transient
  failures with bounded backoff, and removes expired endpoints.
- Subscription endpoints must be HTTPS URLs on an allowlist of known browser
  push services. Redirects, credentials, custom ports, fragments, IP literals,
  and arbitrary hosts fail closed.
- The service worker handles only `push` and `notificationclick`. It has no
  install, activate, fetch, or Cache Storage behavior and is unregistered when
  the device is disabled.
- Push is an attention convenience, not durable business truth. Decisions and
  their resolution remain authoritative in the local database and Needs you
  inbox.

## Consequences

A phone can receive private, low-noise prompts while the API and browser are
not continuously connected. API restarts preserve both subscriptions and
pending delivery attempts without involving the terminal sidecar. The bounded
queue and single shared HTTPS client prevent notification work from becoming an
unbounded memory or connection source.

Apple and browser push-service host changes require an explicit allowlist
update. Delivery failure never blocks decision creation, presence changes, or
worker operation.

## Validation

- Persistence tests cover presence gating, queue bounds, crash recovery,
  retry timing, endpoint expiry, and schema 14-to-15 migration.
- API tests cover authentication, no-store responses, content-free payloads,
  endpoint allowlisting, and SSRF-shaped rejection.
- Browser tests cover explicit opt-in, denial, registration, and persisted
  subscription material.
- The production build is inspected to confirm the public service worker has no
  fetch or cache path.