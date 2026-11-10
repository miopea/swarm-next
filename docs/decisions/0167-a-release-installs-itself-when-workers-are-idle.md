# 0167 — A release installs itself when its workers are idle, judged from the release

- Status: Accepted
- Date: 2026-11-10
- Asked for by: the operator, on 2026-11-10, after seeing member Hives still
  behind: "We should make it so they update when workers are idle just like
  the worker engine right? why would we not use the same idle check?" Then:
  "What about my suggestion of idle? not just away? app updates are even less
  destructive?"
- Amends: ADR 0110 §1 (when an ordinary release installs itself, and what
  counts as ordinary) and ADR 0157 ("Hives in the field").

## Context

Measured read-only on the operator's Keeper on 2026-11-10:

- Five member Hives were on 1.17.10, the last release on terminal protocol 17.
  Every release from 1.18.0 (2026-11-05) speaks protocol 19.
- A Hive installed a release by itself only when the manifest's protocol for it
  equalled the running engine's. Anything else was held as a protocol change
  for the operator, and nothing asked the operator. So none of the five would
  ever install a 1.18 release by itself.
- That hold was stale. Since ADR 0157 a release whose API still drives the
  installed engine installs the app alone, and every worker keeps running.
  Every 1.18 release drives an engine on 17. A Hive on 1.17.10 hands its
  install to the new release's own `update`, which makes that choice.
- The Keeper's roster said of a member on 1.17.10 that its pending engine
  update "swaps by itself once its workers are idle". That release has no idle
  engine update (ADR 0156 first shipped in 1.18.0). An engine older than
  protocol 18 cannot be asked whether its workers rest, so it never would.
- An ordinary release installed only while the operator was away. The engine's
  own update, which stops every session, waits only for every worker to rest.

## Decision

1. **The release says whether it swaps the engine.** A Hive downloads the
   offered release first, then reads its `PROTOCOL` and
   `OLDEST_ENGINE_PROTOCOL` as its own `update` will. The same protocol, or an
   API that drives the running engine, installs the app alone and is ordinary.
   Anything else swaps the engine and waits for the operator, as before. So
   does a release or an engine that cannot be read. Two other places use the
   same answer once the release is downloaded: the Updates card, and the list
   of workers written down for a return before an install.

2. **Idle workers are enough.** An ordinary release installs once every worker
   is idle, by the engine's own count (no session mid-turn, none it cannot
   read), or while the operator is away. Before, only away counted. The idle
   count is a status field that engines on protocol 17 already report. It is
   not the admission an engine swap needs, because an app install stops
   nobody. What the restart can break is a busy worker's Swarm calls, so a busy
   worker still holds it while the operator is at the Hive.

3. **A release is listed under older protocols, oldest last.** Hives through
   1.18.3 still judge from the manifest's number. Listing a release under 17
   tells a Hive on 1.17.10 what is true: installing it stops no worker. That
   code keeps the last of equal versions. This code takes the highest
   protocol, the release's own, wherever it is listed. `publish-release-manifest.sh`
   orders the list itself, highest first, so the caller cannot get it wrong.

4. **The roster says "swaps by itself" only where it does.** A member reports
   whether its waiting engine update applies itself: the switch is on, an
   install manages the engine, no migration is prepared, and the engine is on
   protocol 18 or later. A report without the field, from an older member, is
   read as "it does not". Then the roster names the Worker engine card.

## Consequences

- A member on 1.17.10 takes the next release by itself while its operator is
  away, under its own older rule. The app installs alone and its workers keep
  running. Its engine stays on 17 until its operator presses the Worker engine
  card once. From then on releases and engine updates both install while its
  workers are idle.
- A Hive on 1.18.0 to 1.18.3 also reads the protocol 17 entry. If its engine is
  on 17 it installs the next release by itself. If its engine is already on 19,
  it holds that one release for a press, once.
- A protocol change the new API cannot drive still waits for the operator.
- The download now comes before the protocol is judged. A release held as a
  swap is already downloaded for the Install button.

## Rejected

- **Install whenever, with no gate.** The API restart breaks a busy worker's
  Swarm calls. Idle is the cheapest honest gate.
- **The engine's admission for app installs.** It needs protocol 18. The
  members this is for are on 17, and nothing is stopped, so nothing needs
  admitting.
- **A new manifest field saying which engines a release drives.** The
  signature covers the typed payload, so Hives in the field would refuse the
  manifest (ADR 0157).

## Not proven

- That a member on 1.17.10 installs the next release by itself. That needs the
  release published with protocol 17 listed, and its operator away.
- That the idle rule fires on a real Hive. It needs a Hive on the release that
  carries it, and a later release to install.

## Addendum 2026-11-10: a member says why it is behind

The roster could say a member was behind and never why: busy workers, a release
waiting for its operator's press, a failed download and the switch turned off
all looked the same.

Each automatic install pass now records where it stopped, and the member's
health report (ADR 0114) carries it as `release_hold`. The value is replaced on
every pass, and is absent when nothing held the release. A pass that cannot read
its own store or status keeps the last value rather than inventing one. Two
holds are new, because nothing explained them before: the download failed, and
the Hive refused to start the install it decided on.

The Keeper shows the hold on the member's health line only when that member is
behind or schema-behind past the grace a new release gets, and only from a
member it has heard from recently. Between releases a hold is every Hive's
normal state, and showing it would be noise. Each hold names who moves it: the
Hive by itself, or its operator from the Updates card.

A hold this Keeper does not know reads as unrecognised and is not shown. The
rest of the report is still read, because members and their Keeper run different
releases as a matter of course. The field is optional, so the report's schema
version is unchanged and an older Keeper ignores it.
