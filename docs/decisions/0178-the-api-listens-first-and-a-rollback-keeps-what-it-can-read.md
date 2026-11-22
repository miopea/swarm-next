# 0178 — The API listens first, and a rollback keeps the database it can read

- Status: Accepted
- Date: 2026-11-21
- Decided by: the worker that owns reloads and installs, on Queen's triage of
  its report of a failed reload. Queen asked for a plan on three points first:
  the order of the bind and the startup passes, what the health window should
  measure, and what a rollback does with writes made after the pre-update copy,
  keeping the discarded state at minimum. She ruled out any change to ADR
  0122's behaviour beyond the ordering.
- Amends: ADR 0164, point 1 ("The pass awaited before the bind still starts
  workers"). ADR 0122 changes only in when its pass runs.

## Context

On 2026-11-21 the Keeper reloaded onto a correct build, and the reload failed.

- The pre-update database copy was written. The old API was told to stop and
  took 45 seconds, still writing.
- The new API ran one worker pass before it bound its port. A newer Claude CLI
  had been installed the day before, so that pass typed `/restart` into each
  Claude worker behind it (ADR 0122), about ten seconds each. The record of who
  was asked is kept in memory, so every API start asks again.
- swarm-package probes health 30 times, a second apart. Every probe was
  refused. It declared the build failed and rolled back.
- The rollback restored the pre-update copy. The schema was the same before and
  after, so the restored release could have read the live database as it
  stood. Everything the Hive wrote between the copy and the restore, about 78
  seconds across every worker, was lost, and nothing kept a copy of it.

ADR 0164 had already moved the engine update after the bind for the same
reason. The rest of the pass stayed in front of it.

Every ordinary install, automatic install (ADR 0110) and development reload
runs `update`, with the same 30-probe window. Only `migrate-protocol` waits
longer, 240 probes, since the 2026-08-28 failure its comment records.

swarm-package asked a release for its schema by having `verify-database`
create and migrate a file that did not exist. Since 2026-09-04
`verify-database` refuses a missing file, so that probe has answered nothing on
any real release, and `swarm-package rollback`'s refusal of a database ahead of
its target has never fired since. The package tests did not see it, because
their stub `swarmctl` accepted the missing file.

## Decision

1. **Nothing that waits on workers runs before the API listens.** The
   supervisor's background service runs its first pass at once, after the bind
   and beside the listener, instead of being awaited before it. Health answers
   while that pass reconciles bindings, starts autostart workers or types
   `/restart`. The asks themselves are unchanged.

2. **The health wait follows the API process it started, not a fixed number
   of seconds.**
   - While the swarm-api unit's MainPID is the process this wait first saw,
     the wait goes on up to `SWARM_HEALTH_ALIVE_ATTEMPTS`, default 240, the
     ceiling `migrate-protocol` already uses.
   - Once that process has exited or been replaced, or when systemd cannot
     name one, the ordinary window applies, counted from the start. A crash is
     never given longer than before.
   - An explicit `SWARM_HEALTH_ATTEMPTS` is also the longer ceiling unless
     `SWARM_HEALTH_ALIVE_ATTEMPTS` is set too, as `migrate-protocol` already
     defers to an explicit budget.

3. **A rollback restores the pre-update copy unless it is proven that the
   release it returns to reads the live database as it stands.**
   - Proven means four readings agree: the database's schema before the
     update, its schema now, and the migration version each of the two builds
     carries, asked of the builds themselves. Then the release being restored
     verifies a private copy of the live database, with its `-wal` and `-shm`.
     If that passes, the live database stays and nothing is restored. Same
     schema is the rule `swarm-package rollback` already relies on to run an
     older release over the live database.
   - The builds are asked with a new `swarmctl schema-version`, which migrates
     an in-memory database and touches no file. Both are asked when the
     pre-update copy is taken, while the old API still serves, so a rollback
     spends no extra time with the Hive down.
   - Any reading that is missing or disagrees is doubt. Doubt, a moved schema,
     or a copy that does not verify restores the pre-update copy as before. First, the database it replaces is
     kept beside the pre-update copies as `backups/discarded-<version>-<time>.sqlite3`,
     with its `-wal` and `-shm`, at mode 0600. The installer says where.
   - Kept databases are bounded at three, newest kept, and pruned only among
     themselves. Each holds writes nothing else has.

The bind order is chosen over a longer window alone, because no window can be
right for a pass whose length depends on how many workers are behind. The
liveness check is chosen over a longer fixed window, because a fixed window
either calls a slow start broken or makes a crash wait minutes. Keeping the live
database is chosen over moving the pre-update copy until after the old API
stops, because it saves the new build's writes too and needs no extra downtime.

## Consequences

- A reload or install of a correct build is no longer rolled back because
  workers are slow to restart.
- A build that keeps running and never answers now takes up to four minutes to
  roll back instead of 30 seconds. The API is down for that long; workers are
  not affected. A build that crashes rolls back as before.
- A rollback with no schema change keeps every write the Hive made during the
  attempt, including the old API's writes after the copy was taken. It needs
  both builds to answer `schema-version`, so it starts with rollbacks onto the
  release that carries this. A rollback onto an older release is doubt: it
  restores the copy and keeps what it replaced.
- `swarm-package rollback` refuses a database ahead of its target again,
  wherever the target answers `schema-version`. The package tests' `swarmctl`
  stub now refuses a missing file as the real one does.
- A rollback that must restore no longer loses what it replaced. Getting it
  back is a manual `swarm-package restore` of the kept file. Nothing merges it.
- Workers are asked onto a new Claude release after the API listens rather
  than before. When the asks are made and whom they go to is unchanged.
- The first worker pass can now overlap requests. It writes only durable
  state, which requests read as they always have during later passes.
- Hives get this with the release that carries it. Its own install still runs
  the previous release's API stop and the new release's `update`, so a Hive
  installing it already has the new health wait and rollback.

## Not proven

- The fix has not been observed on a real reload. The Keeper's next reload
  was onto 1.22.0, without this fix, and succeeded; reloading it onto this
  release is the operator's step.
- The health wait's reading of MainPID is tested against a systemctl stub, not
  a real user manager.
- `schema-version` and the installer's reading of it were run against a real
  build (231) and the installed release (refused, so doubt). The keep branch
  itself has run only against the stub.
