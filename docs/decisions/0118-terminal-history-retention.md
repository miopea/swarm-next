# 0118 — Terminal history retention, changed while the host runs

- Status: Accepted
- Date: 2026-10-15
- Decided by: the operator, on 2026-10-15, in the Swarm Next worker's terminal:
  "start terminal history retention". It was the third setting left from their
  first policy set, and its order had been theirs to choose.
- Builds on: ADR 0113. Retention is a setting a Keeper can require only once a
  member enforces it, and this is what enforces it.
- Task 01a1376b. The design is note 20253.

## Context

The terminal host keeps every worker's output on disk so it can be scrolled
back after a restart. That history is bounded by a maximum age, seven days, and
by a total size, 512 MB. Both limits were fixed when the host opened its
history store, from defaults or from `SWARM_TERMINAL_*` variables read once at
startup.

Changing either limit meant restarting the terminal host, which stops every
worker session, and ADR 0113 rules out remote restarts. The other way to tell
the host something is a new `HostRequest`. A guard test makes that a protocol
bump, and a protocol bump is a migration that stops every worker on every Hive.

## Decision

### 1. The setting

Each Hive keeps 1, 3, 7, 14 or 30 days of terminal history, set in Settings >
Maintenance > Terminal history.
- Seven is every Hive's starting point, as before.
- The 512 MB cap still applies, and the card says so, because a busy Hive may
  keep less.
- Shortening deletes older history straight away. Lengthening keeps only what is
  still there.

### 2. Two files, not a host request

The API and the host exchange the setting through two files beside the history
directory:
- `<state>/terminal-history.settings.json`: what the Hive asks for. The API
  writes it at startup, after the operator changes the setting, and after every
  Apiary policy pass on a member.
- `<state>/terminal-history.applied.json`: what the host is keeping. The host
  writes it when it starts and whenever that changes.

The host reads the request when it starts and once a minute. That runs in one
bounded task, owned by the same `select` as its server. The store's maximum age
is an atomic that the prune path reads, and changing it prunes under the store
lock at once. No protocol change and no restart are involved.

### 3. In effect means the host says so

The minute is latency only. Whether retention is in effect is decided by the
host's own `applied` file, compared with the setting:
- **In effect:** the host keeps exactly the days set.
- **Applying:** the host keeps something else and will pick this up within the
  minute.
- **Awaiting the engine update:** the host has never written the file, so it
  predates this setting.

The card shows which. A member records `terminal.history_days` as applied only
when its host keeps exactly the required days.

### 4. The Keeper

The Keeper requires the days as `terminal.history_days` (ADR 0113 addendum). On
the member the setting is locked, and a local change is refused with 409
`set_by_apiary`.

`RequiredSetting::engine_applies_since` names the release that first carries
the host side, 1.17.4. A member on that release or later that has not applied
the setting is in a new policy gap, `engine_update_pending`. The roster says
"This Hive applies it after its worker engine update", which is a different fix
from updating the release.

## Consequences

- Schema 203 adds `terminal_history_settings`: the Hive's own days, and what
  its host last said it keeps.
- A Hive gets the host side only through a worker engine update. Engine
  updates swap by themselves once a Hive's workers are idle, or when its
  operator applies one. Until then its card and its Keeper both say it is
  waiting for one.
- `SWARM_TERMINAL_HISTORY_MAX_AGE_SECONDS` sets only the age the host starts
  with. The Hive's setting replaces it at the host's first check.
