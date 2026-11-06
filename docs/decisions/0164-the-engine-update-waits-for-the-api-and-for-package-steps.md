# 0164 — The engine update waits for the API and for package steps, and lifts a drain nobody owns

- Status: Accepted
- Date: 2026-11-06
- Decided by: the worker that owns the engine update, on Queen's triage of the
  operator's report: "it could be happening to anyone who is away. we need to
  fix it". Queen asked for the fix direction to be chosen and explained, and
  ruled that startup must not wait on the engine update before the API
  listens.
- Amends: ADR 0156 (a waiting engine update applies itself while every worker
  rests) and ADR 0163 (Swarm stops the workers it vouches for).

## Context

On 2026-11-06 a Hive on 1.18.0 installed 1.18.1 by itself while the operator
was away, and every worker rested.

- The new API runs one worker pass before it listens. That pass now reached the
  engine update. It drained the engine, stopped a resting worker, wrote the
  swap request and waited for the swap.
- The swap is swarm-package's, and it takes the package lifecycle lock. The
  install held that lock while it waited for this same API to answer its health
  check. The swap deferred, the API never listened, and after 30 seconds the
  install gave up and rolled back.
- The rollback killed the API mid-wait, so nothing cancelled the drain. The
  terminal host survives an app rollback on purpose, so it kept draining and
  refused every worker start until the machine restarted.

1.18.0 had the same pass, but its engine could not answer admission, so the
update held up before it drained anything. 1.18.1 is the first install onto an
engine that can answer.

The same holds for a development reload, which takes the same lock and waits on
the same health check.

## Decision

1. **Nothing is tried before the API listens.** The pass awaited before the
   bind still starts workers. The engine update skips it, and is allowed from
   the moment the listener is bound.

2. **Nothing is drained while swarm-package holds its lifecycle lock.** The API
   reads the lock swarm-package takes, `.package-lifecycle.lock` in the state
   root beside the database, with a shared lock taken and dropped at once. While
   an install, a reload, a swap or a migration holds it, the update waits, and
   the card says an install or another package step is running. A lock that
   cannot be read counts as held. The probe never holds an install up. A package
   step that tries for the lock in the same instant defers and runs again on its
   timer.

3. **A drain nothing owns any more is lifted.** A drain is owned while:
   - this process holds the worker lifecycle (an attempt, or the card);
   - swarm-package holds its lock;
   - a swap request is written and not yet taken.

   With none of those, nothing is left that would cancel it. Swarm cancels it
   and says so in the journal, and workers owed a return come back on that pass.
   The update, if one still waits, is tried again on a later pass.

The probe is chosen over holding the lock through an attempt, because the swap
the attempt asks for needs that same lock. Gating on the lock is chosen over a
delay after startup, because correctness here cannot rest on a wait. Lifting the
drain in the API is chosen over teaching the engine a drain lease, because it
needs no protocol change and reaches Hives with the next app install.

## Consequences

- An install, or a reload, of a Hive with an engine update waiting finishes
  first, and the update applies afterwards on a pass that finds the lock free.
- An install that starts in the middle of an attempt can still restart the API
  under it. The attempt is lost, and its drain is lifted once the install lets
  go of the lock. The new build may try the update again, because the attempt it
  lost was recorded against the old build.
- An attempt that timed out waiting for its swap no longer leaves the engine
  draining for good. Its returns stay owed and held while the engine is behind,
  as before.
- A Hive already stuck in a drain left by a 1.18.1 install is lifted by any API
  with this change, once it is installed. A Hive that rolled back to 1.18.0 needs
  the terminal host restarted, or this release installed.
- The API now depends on swarm-package's lock file name and place. A test reads
  both from the package script and the unit file.

## Verification

- With the API not yet listening, the engine is not asked anything. Once it
  listens, the same update applies.
- With the package lock held, nothing is drained or stopped, nothing is owed,
  and the card says why. Once it is let go, the update applies.
- A drain is not lifted while the package lock is held, while a swap request
  waits, or while the worker lifecycle is held. With none of those, it is lifted
  once, and the update still waiting then applies on a later pass.
- Removing any one of the listening gate, the lock check, the lift, or each of
  the lift's three guards fails a test.
- The package script still takes `$state_root/.package-lifecycle.lock`, and the
  API's database still lives in that state root.
