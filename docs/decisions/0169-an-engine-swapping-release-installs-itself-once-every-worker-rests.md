# 0169 — A release that swaps the engine installs itself once every worker rests

- Status: Accepted
- Date: 2026-11-10
- Decided by: the worker that owns release installs, on Queen's go to build it
  as a draft, with two rulings: (a) it waits for rest only, as an engine
  update does; (b) it reuses the engine's idle-update switch, with no new
  setting, on the condition that a member names clearly when an automatic swap
  was attempted.
- Amends: ADR 0167 §1 (a release that swaps the engine waits for the
  operator) and ADR 0156 §4 (nothing is tried while a protocol migration is
  prepared).

## Context

After ADR 0167, the only release a Hive did not install by itself was one
whose API cannot drive the running engine. Installing that one swaps the API
and the engine together, so it ends every worker session. It waited for the
operator's press on the Updates card, and nothing asked the operator. A member
Hive whose operator is rarely there stayed behind until somebody noticed.

The engine update already swaps the engine by itself (ADR 0156). It waits until
the engine says every session rests, stops exactly that set, records a return
for each, and brings them back on the new engine. A release that swaps the
engine interrupts the same sessions in the same way. The difference is that it
replaces the API too, which the engine update never did.

## Decision

1. **An automatic install prepares it, and stops nobody.** Where the engine's
   idle update would take the swap, the automatic install asks the install
   unit to prepare the release instead of installing it. The request carries a
   second line, `mode=prepare-migration`. The install unit then runs
   `migrate-protocol-if-idle`, not `update`. With a worker loaded, that defers:
   the release is pending, both links stay where they are, and the install
   unit marks the migration as the automatic install's
   (`protocol-migration.automatic`, naming the release it prepared). With
   nobody loaded there is nobody to interrupt, and it completes at once.

2. **The engine's idle update swaps it once every worker rests.** A migration
   marked as the automatic install's is the idle update's to apply, through
   every step it already takes for an engine update: the engine's busy count at
   zero, the drain, a return recorded for every running session, and the
   engine's admission of that exact set. The swap request names the prepared
   release, and the install unit completes only the migration a request names.

3. **Where the idle update would take it (ruling b):**
   - automatic install is on;
   - the engine's idle-update switch is on, with no new setting;
   - this installation replaces the engine;
   - the engine can be asked whether its sessions rest (protocol 18 or later);
   - nothing its operator prepared is pending.

   Anywhere else the release waits for the operator's press, as before.
   Turning either switch off after the release was prepared stops the swap
   from going ahead by itself; the operator's maintenance action can still
   apply it.

4. **Rest only (ruling a).** Being away is not enough, unlike an ordinary
   release. The engine's admission is the only judge of whether a session
   rests. **Swarm overrules none of its refusals for a migration.** For an
   engine update Swarm stops workers the engine refused only for what it
   counted (ADR 0163), and afterwards tells each of them that its scheduled
   jobs are gone. That message comes from the API, and a migration replaces
   the API, so it would never be sent.

5. **One attempt per release.** An attempt that stopped workers and did not
   finish is not tried again by itself, whoever started it. That covers both a
   failed attempt and one never heard from again. A migration replaces the API
   that started it, so the API that comes up settles the attempt from what the
   engine reports. Until then the attempt stays open. An open attempt counts as
   unfinished.

6. **A member says where its swap stands (Queen's condition).** Two new
   `release_hold` values:
   - `swap_waits_for_rest`: prepared, and it swaps by itself once every worker
     rests;
   - `swap_attempted`: a swap of it stopped workers and did not finish, and is
     not tried again by itself.

   `protocol_change` still means it waits for the operator's press.

7. **The Updates card says so before it happens.** The release status gains
   `engine_swap` (`waits_for_rest`, `waits_for_press`, `attempted`). Where it
   is `waits_for_rest`, the card says the release installs itself once every
   worker rests, swapping the engine and restarting each worker. Once the
   release is prepared, the card says Swarm applies it and does not send the
   operator to the maintenance action. The press still swaps at once and ends
   every session, as before.

## What it can interrupt

**Every worker session, once, while each one rests.** Queen's session is
included, as for an engine update (ADR 0156 §2). Each session loses its live
process, not its conversation: it is recorded as owed a return and brought
back on the new engine. A Remote Control connection to a stopped session drops
with its process. The API and the web app restart, and an open page reconnects.

**What protects a worker mid-turn.** The engine's admission refuses the whole
set before it stops anything. A running turn, a queued message, unsent input,
a background task or a scheduled job each refuse. Swarm overrules nothing here
(decision 4).

## Consent

**A release that restarts every worker now installs by itself on any Hive with
automatic install and the engine's idle-update switch both on.** Both are on
by default. The operator can overrule this before any release carries it, by
withdrawing this change or by turning either switch off.

It needs a Hive that already runs a release containing it: the install unit
that reads the request, and the API that applies the swap, are the running
release's own. So the first release it moves is the next one that swaps the
engine after it is installed. A Hive whose engine is older than protocol 18
cannot be asked whether its sessions rest. Its swap waits for the operator, as
before.

## Rejected

- **A setting of its own.** Ruled out (ruling b). It would be a third switch
  for one interruption that two switches already describe.
- **Swapping while the operator is away, resting or not.** Ruled out (ruling
  a). Away says nothing about whether a worker is mid-turn.
- **Letting the package's timer complete it.** Its idle check counts loaded
  sessions, not resting ones, so it can only ever wait for all of them to stop.
  An autostart worker is revived within seconds, so that wait never ends
  (measured; see `apply_release`).

## Consequences

- An older install unit reads the two-line request as a path and refuses it.
  That is the safe misreading: nothing is prepared, and the member reports the
  install refused.
- The package's reconcile timer drains the engine and lifts the drain again on
  each pass. A pass can therefore lift the drain while an idle update is under
  way, as it can for an engine update today. If a session starts in that gap,
  the install unit does not complete the swap, because it completes one only
  with no session running. The attempt then counts as unfinished (decision 5).
  This exposure is not new, and is not fixed here.

## Not proven

- **A real migration on a Hive.** The tests use a fake engine, a fake install
  unit, and the package's lifecycle test against stub releases.
- A failed swap rolled back on a real Hive, with the workers it stopped coming
  back on the old engine.
- A timer pass overlapping an attempt. No test covers it.
