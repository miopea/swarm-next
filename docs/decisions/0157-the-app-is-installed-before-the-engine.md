# 0157 — The app is installed before the engine

- Status: Accepted
- Date: 2026-11-04
- Decided by: the operator, after a protocol migration ended every worker on
  install: "the migration and app should update and then the users do the one
  manual worker upgrade. After that then idle workers can update
  automatically".
- Amends: ADR 0009, whose ordinary update rejects any protocol change, and the
  "Protocol 18 and its ordering" section of ADR 0156.
- Amended by: ADR 0167. Auto-install no longer holds every protocol change:
  it reads the downloaded release and holds only one that swaps the engine.

## Context

A release that changed the terminal-host protocol swapped the API and the
terminal host together. `update` drained the host, switched both links and
restarted both services, so installing the app ended every worker session.

Measured on the first protocol 18 migration:

- The API took 90 seconds to stop and was killed.
- Workers came back over about a minute and a half, four per pass, after Queen.
- The card attempt that started the migration was recorded as timed out. The
  API waiting on it was itself replaced partway through.

A protocol bump is usually additive. Protocol 18 added one request
(`AdmitMaintenance`) and one response, both behind
`supports_maintenance_admission`. An API on 18 already sends a protocol 17
engine nothing it cannot read, so it can drive that engine. Only the swap made
the install end every session.

## Decision

The app is installed first and the engine follows on its own step.

- `swarm-terminal` declares `OLDEST_ENGINE_PROTOCOL`, the oldest engine
  protocol this API drives. It is 17. `drives_engine(protocol)` is true from
  that protocol through `PROTOCOL_VERSION`.
- Every request newer than the oldest protocol keeps its own `supports_*` gate.
  A test pins the wire names an engine on the oldest protocol reads, and the
  gate of each request added since. A new request cannot reach an older engine
  without a gate, and the oldest protocol cannot rise without that list
  changing.
- The two places that wanted the exact protocol now accept any engine the API
  drives: native interview hand-off and the conversation-choice fence.
- Both build scripts write the constant into each bundle as
  `OLDEST_ENGINE_PROTOCOL`.
- `swarm-package` reads it through one check, `api_drives_engine`:
  - `update` installs only the app when the new API drives the installed
    engine. The host, its socket and every attached worker are left alone. The
    output names the worker engine update as the step that moves the engine.
  - The host reconcile accepts the gap. The timer still leaves loaded workers
    alone, and the card stops them and swaps the engine as for any other
    engine update.
  - A development reload of such a checkout is an ordinary reload. The reload
    card stops reporting that a migration is required.
  - `apply-release` stops marking such an install as a protocol migration.
  - Rollback accepts a previous API that drives the running engine.
- A protocol the new API cannot drive still migrates in one hop, as before.
  So does any bundle without the file, and any value that does not read as a
  number.

So a protocol change reaches a Hive in three steps:

1. The app installs, by itself or when accepted. Workers keep running.
2. The operator applies the worker engine update once from the card.
3. From then on, same-protocol engine updates apply by themselves while every
   worker rests (ADR 0156).

### Hives in the field

A Hive installs a release by running its own `apply-release`, which hands the
install to the new bundle's `update`. That has been true since v0.8.17. The new
bundle's `update` is what decides, so a Hive on an older release also installs
the app alone. The lifecycle test drives the real v1.17.10 script to prove it.

The manifest cannot say whether a release drives an older engine. Its signature
covers the typed payload, so a new field would fail verification on Hives in the
field. Auto-install therefore still holds any protocol change for the operator
to accept. The release card now says both outcomes plainly, because the Hive
cannot tell which one it gets before the download.

## Consequences

- Installing a release no longer ends worker sessions unless its API cannot
  drive the installed engine.
- The engine swap happens on the card and restarts only the terminal host.
  The API waiting on the swap survives it, so the attempt is recorded by the
  process that started it.
- Raising `OLDEST_ENGINE_PROTOCOL` is how a release drops support for an older
  engine. That release migrates in one hop, with the cost announced.
- An API and an engine may now run one protocol apart until the card is used.
  That pair is the one the API was built and tested to drive.

## Verification

- The `swarm-terminal` test pins the protocol 17 request names, each later
  request's gate, and both ends of `drives_engine`.
- An API test runs native interview hand-off against a fake engine on the
  oldest protocol. The existing protocol 16 test still refuses.
- A runtime test reads this checkout's `ipc.rs` the way the reload card does.
- The lifecycle test runs on a fresh Hive with three workers attached:
  - the app installs, the host link stays, and nothing restarts or drains the
    host;
  - the timer leaves loaded workers alone and does not fail;
  - the card's request moves the engine onto the app's release;
  - a protocol the new API cannot drive still migrates in one hop;
  - a Hive running the real v1.17.10 `apply-release` installs the app alone.
- The package job now fetches tags, because that last case reads v1.17.10 from
  git.

## Not proven

- No Hive has taken a real app-first install yet. The first is the next
  release that changes the protocol.
- `test-field-upgrade.sh` already failed on main at its v0.9.0 vintage, before
  this change. It is not run in CI, and this change does not touch that path.
- A migration the API cannot drive still replaces the API that waits on the
  card's attempt, so that attempt can still be recorded as timed out.
