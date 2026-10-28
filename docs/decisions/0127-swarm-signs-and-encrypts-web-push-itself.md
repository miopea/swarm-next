# 0127 — Swarm signs and encrypts Web Push itself

- Status: Accepted
- Date: 2026-10-27
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-27.
  Offered three ways past a failing audit, they chose "Replace
  web-push-native" over ignoring the advisory or merging past the check.
- Amends: the browser notifications adapter (`docs/01-capability-inventory.md`,
  "Browser notifications").

## Context

The required `rust-audit` check began failing on main on 2026-10-27, and with
it every merge. RUSTSEC-2026-0387 (2026-10-23) reports timing variability in
operations `crypto-bigint` 0.5.5 means to be constant-time; the fix is 0.7.6 or
later. Swarm reached 0.5.5 only through `web-push-native` 0.5.0, by way of
`jwt-simple` 0.12 and `elliptic-curve` 0.13. 0.5.0 was already the newest
`web-push-native`, so there was nothing to upgrade to.

The same chain was why CI ignored RUSTSEC-2023-0071: `jwt-simple` carried an
RSA implementation Swarm never used.

`web-push-native` did two small, fully specified things: sign a VAPID token
(RFC 8292) and encrypt the body for the browser (RFC 8291, in RFC 8188's
`aes128gcm`).

## Decision

`crates/swarm-api/src/web_push.rs` does both on `p256` 0.14 (`elliptic-curve`
0.14, `crypto-bigint` 0.7.10), with `hkdf` 0.12 and `aes-gcm` 0.10.

- **The key format does not change.** A VAPID key is stored as the 32-byte
  P-256 scalar `jwt-simple` wrote, so the Hive's key, and every browser
  subscribed to it, carries over. Startup still refuses a stored private key
  whose public key does not match the stored one.
- **The request does not change in substance.** It is the same POST with
  `TTL`, `Content-Encoding: aes128gcm`, `Content-Type` and `Authorization:
  vapid t=…, k=…`. The token claims `aud` (the push service's origin), `exp`
  (the TTL, capped at a day) and `sub`. One record carries the message, as
  before, declared with RFC 8291's example record size of 4096.
- **Every push takes a fresh salt and sender key** from the system's random
  source.
- **The audit has no exceptions.** `rsa` left `Cargo.lock` with `jwt-simple`,
  so the RUSTSEC-2023-0071 ignore was deleted rather than kept.

## Consequences

Swarm owns about 200 lines of protocol code it used to borrow. That code is
pinned to published vectors rather than to a library's behaviour: RFC 8291
Appendix A is reproduced byte for byte, and the token is verified under its own
key and refused under another.

Owning it also means owning the next advisory. The crates it rests on are the
RustCrypto primitives any replacement library would use, so an upgrade is a
version bump here rather than a wait on a maintainer.

## Validation

- The RFC 8291 example encrypts to its published body, which also proves a
  32-byte private key derives the public key the RFC gives.
- A VAPID header carries a token for the endpoint's origin, expires within a
  day, verifies under its key and fails under another.
- Two pushes never share a salt or a sender key, and a short auth secret or an
  oversized message is refused.
- Read-only against the live Hive on 2026-10-27, its stored private key loaded
  with the new code and derived its stored public key.
- Not proven by tests: delivery through a real push service. That needs a
  reload and a test notification to a subscribed device.
