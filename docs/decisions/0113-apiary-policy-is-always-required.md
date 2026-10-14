# 0113 — Apiary policy is always required

- Status: Accepted
- Date: 2026-10-12
- Decided by: the operator, 2026-10-12. Asked whether the Keeper should govern
  members' updates, they chose "Keeper can require auto-install", adding "but
  this is part of a larger policy control system the keeper should have over
  hives in its apiary". Asked how a setting binds a member, they chose
  "Always required".
- Reverses: the drift-only model in `docs/specs/apiary-controls-scope.md`
  ("drift is visible rather than prevented; a member may still override
  locally") and the matching comments on `policy_drift` and
  `apply_apiary_policy`.
- Spec: `docs/specs/apiary-fleet-governance.md`. Task 01a13143.

## Decision

1. **A setting in the Apiary's policy is required on every member.** Nobody can
   override it locally: not the member's operator, its Updates card or its API.
   A setting absent from the policy stays each Hive's own choice. A member that
   cannot accept a requirement leaves the Apiary, and its screens say so.
2. **Only settings members apply can be required.** The domain names them
   (`RequiredSetting`), each with the values it accepts. A Keeper is refused,
   with the setting named, when it tries to require anything else. An inert key
   would read as a requirement and change nothing. Each new setting arrives with
   its enforcement.
3. **A requirement overrides the member's own choice wherever that choice is
   read. It never rewrites it.** When the Apiary stops requiring a setting, the
   operator's own choice is still there, and it takes effect again.
4. **A member records a requirement as applied once its release puts it into
   effect.** Its capability report carries that to the Keeper. The Keeper sees a
   Hive only when something required is NOT in effect, with the reason, which is
   also the fix:
   - it has not reported;
   - it has not picked up the current policy;
   - its release cannot apply the setting;
   - it runs a different value;
   - it runs a release below the Apiary's minimum (addendum below).

   A compliant Hive shows nothing extra, under the operator's rule against
   "data that we wouldn't act upon".
5. **Changing policy rings the doorbell** (`FederationChangeKind::Policy`), so
   members take it up within about a second rather than at their paced pass.

## The first setting: ordinary releases install automatically

`release.auto_install` = `on` | `off` turns ADR 0110's automatic install on or
off, locked. Requiring it on also keeps checking for releases on, because a Hive
that never checks never installs, so "Stop checking" would have undone the
requirement while every screen said it was in force. It changes WHETHER a member
installs on its own, never WHEN:
- it still installs only while its operator is away or in Night Watch;
- a protocol change still waits for them;
- it never restarts a Hive.

## What can never be policy

These stay refused in code, as before, and requiring is no exception:
- credentials, secrets and tokens;
- workspace roots and file paths;
- provider execution permissions (what workers may run);
- remote restarts.

## Consequences

- A Keeper's choice is made for other people's machines. Its panel says that
  before it offers a setting, and the member's Apiary page and Updates card say
  who set it.
- The remaining settings the operator chose on 2026-10-12 each need their own
  member-side enforcement before they can be required:
  - check strictness (built as the check gate, ADR 0115);
  - Queen autonomy (built, see the 2026-10-13 addendum);
  - Night Watch schedule (built, see the 2026-10-13 Night Watch addendum);
  - terminal history retention;
  - worker model.

  Until then they are absent from `RequiredSetting`, so they cannot be required
  and nothing pretends otherwise.
- The two-Hive acceptance run checks the whole loop:
  - the member's own choice is off and the Keeper requires on;
  - the member runs it within about a second and cannot undo it;
  - the Keeper sees it in effect;
  - lifting the requirement gives the member its own choice back.

## Addendum 2026-10-12: a minimum version for shared work

The operator chose this as the next setting. `shared_work.minimum_version` takes
a release number, such as `1.16.10`. A development build is refused as a value,
because it is no one's minimum.

**The Keeper enforces it, not the member.** A Hive below the minimum runs an
older release, and an older release does not know the setting exists. A check on
the member's side would be skipped by exactly the Hives it is for. So the domain
says who enforces each setting (`PolicyEnforcer`), and the Keeper judges this one
from the release each member reports. A member never records it as applied,
because nothing on the member made it true.

Below the minimum, a Hive keeps its own work, and the Keeper turns down its
shared work:
- a claim on an Apiary task is rejected, and the receipt carries the reason,
  which the member keeps in its outbox;
- reserving a promoted Jira issue, or accepting a claim handoff, answers 409
  `update_required_for_shared_work`, with the reason.

Moving work it already holds, and filing new work, stay open. Losing those would
strand work mid-flight, which is worse than the gap the minimum closes.

- **Fails closed.** A Hive that has not reported its release cannot be shown to
  meet the minimum, so it is refused, and the reason says that.
- **A development build counts as its release.** A build of 1.16.9 meets a
  1.16.9 minimum.
- **Both sides see it before a claim fails.** The Keeper's roster names the
  release the Hive runs. The member's Apiary page says it cannot take shared work
  and why, judged from its own release, so it knows to update before it tries.

**Engine updates when idle needs no setting.** Every packaged Hive already swaps
its worker engine once no session is mid-turn, with no opt-out, so there is
nothing to lock.

The two-Hive acceptance run checks this loop:
- the Keeper sets a minimum above the member, and the member's own screen and the
  Keeper's roster both say so;
- the member's claim on shared work is turned down, with the reason;
- at the member's own release, the same claim goes through.

## Addendum 2026-10-13: Queen autonomy, and leaving

The operator chose Queen autonomy next, after check strictness turned out to
have nothing to lock: no Hive has a gate on which checks must pass before work
counts as done, so that one needs the gate built first.

**One setting per presence tier.** A Hive already holds its Queen's autonomy as
three levels, for At the Hive, Reachable and Night Watch, so the Apiary sets
them the same way:
- `queen.autonomy.at_hive`
- `queen.autonomy.away`
- `queen.autonomy.night_watch`

Each takes `advisory`, `coordinate` or `local_execution`. A tier the Keeper
leaves unset stays the member's own.

**Applied where it is read.** The member's store applies the required level in
the one function that every reader goes through: the Queen conductor, deployment
grants and the Settings screen. None of them needs to know about policy. The
operator's own level is kept underneath, as with auto-install, and comes back
when the requirement is lifted.

**A locked tier cannot be moved.** Settings shows it disabled with "Set by your
Apiary. Only its Keeper can change this." A request that moves it is refused
with 409 `set_by_apiary`. A request that sends it back at its required level
passes, so the screen can still change the tiers that are free.

**Leaving lifts every requirement.** This ADR says leaving is a member's way
out. Until now, departure kept the received policy and the settings applied for
it, so a Hive that left stayed locked by an Apiary it no longer belonged to.
Departure now clears both, along with the rest of the shared state.

**What an older member does.** A Hive on a release without these keys keeps
its own levels. The Keeper's roster says its release can't apply them.

## Addendum 2026-10-13: Night Watch hours

The operator ruled on what a required schedule means. Asked "should every Hive
use the Keeper's hours in its own local time?", they chose "Hours in each Hive's
time". The option they chose read: "For example, 10 PM to 7 AM wherever each
Hive is. Each Hive keeps its own time zone. A Hive that has never set a time
zone shows as not in effect until its operator picks one."

**One setting, `night_watch.hours`, written `HH:MM-HH:MM`,** such as
`22:00-07:00`. Only the zero-padded form is accepted, so the value a member
applies compares equal to the Keeper's. Requiring hours also turns the schedule
on.

**The time zone stays the member's.** Its operator's night is local to them, so
a member runs the Keeper's hours in its own IANA zone. A Hive that never set up
Night Watch has no zone. It keeps no schedule and does not report the setting as
applied until its operator chooses one. Its Settings say so, with the hours
already filled in and locked.

**The Keeper tells the two reasons apart from the reported release, with no new
field in the report.** A member on a release that applies the setting, but that
has not applied it, is waiting on its operator: "This Hive's operator hasn't
chosen its time zone for Night Watch yet". An older member is told to update.
`RequiredSetting::local_choice_since` names the first release that applies it,
1.17.2, and must be kept in step if that release number changes.

**Applied where presence reads the schedule.** That is one function, so the
scheduled watch, dismissing tonight's watch on return to the desk, and the
Settings screen all see the required hours. The member's own hours stay
underneath and come back when the requirement is lifted. A request that turns
the schedule off or moves the hours is refused with 409 `set_by_apiary`. A new
time zone is accepted. Returning to Swarm on the desktop still ends tonight's
watch, as before: that is the operator being present, not a schedule change.

## Addendum 2026-10-13: check strictness, as `checks.gate`

Check strictness had nothing to lock until ADR 0115 built the check gate. The
Keeper now requires its level as `checks.gate`: `off`, `warn` or `require`. The
member applies it where the level is read and refuses a local change with 409
`set_by_apiary`. The GitHub login it reads CI with stays its own. A member that
cannot read CI at all says so in its health report, and the roster shows it.

