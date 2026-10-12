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
  - check strictness;
  - Queen autonomy;
  - Night Watch schedule;
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
