# ADR 0022: Durable trusted-browser session

Status: **Accepted**

## Context

Swarm initially kept the operator token in `sessionStorage`. A refresh remained
unlocked, but closing an installed PWA destroyed that tab-scoped state and made
the operator enter the token again. That is too fragile for a daily-driver PWA,
especially on mobile where the operating system routinely closes web processes.

Persisting the operator token in `localStorage` would survive those restarts but
would expose long-lived credential material to JavaScript and any successful
script injection. The browser needs durable trust without making the operator
secret readable by the client application.

## Decision

- A successful bearer-token unlock creates a 30-day, same-origin browser
  session at `POST /api/v1/auth/session`.
- The API stores the session credential in an `HttpOnly`, `SameSite=Strict`
  cookie. Remote sessions also use `Secure`; loopback development remains usable
  over HTTP.
- The UI stores only a non-secret trusted-session marker in `localStorage`. The
  marker tells the client to attempt cookie authentication after a PWA restart;
  it grants no access by itself.
- API requests, terminal attach-grant requests, presence, notifications, and
  reconnects all use the same cookie-authenticated boundary.
- Lock calls `DELETE /api/v1/auth/session`, clears the cookie and local marker,
  and discards authenticated client state.
- Changing the configured operator token invalidates every browser session
  derived from the previous token.

## Consequences

Closing or restarting a trusted PWA no longer signs the operator out. Explicit
Lock, cookie expiry, site-data removal, or operator-token rotation still does.
No raw operator token is retained in JavaScript-accessible storage.

This remains a private, single-operator trust model. Apiary identity, multiple
human accounts, per-user revocation, and device-management policy require a
future identity system rather than extending this cookie format.
