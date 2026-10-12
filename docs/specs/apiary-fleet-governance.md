# Apiary fleet governance: Keeper policy, fleet health and usage

Settled with the operator on 2026-10-12. Tasks: 01a13143 (policy control) and
01a1315b (fleet health and usage). The design record for watching and takeover
is `apiary-controls-scope.md`; this extends it and reverses one of its
decisions, as stated below.

## The rule over everything here

> "We don't want to generate too much random noise for the sake of data that we
> wouldn't act upon."

**Every signal names the action it asks for.** Data that would not change what
the operator does is neither collected nor shown. A healthy Hive, or one fully
in policy, shows nothing extra. A problem appears with its next step attached.

## Keeper policy controls its Hives

**Always required.** Every Apiary setting is locked on every member, with no
local overrides. ⚠️ This reverses `apiary-controls-scope.md`, where drift was to
be visible and a member could still override. Drift now means "not in effect
yet", and it is a fault to fix, not a choice. A member's only way out of a
setting is to leave the Apiary, and its policy screen says so.

**The first set:**

| Setting | What a member does with it |
| --- | --- |
| Ordinary releases install automatically | Auto-install (ADR 0110) locked on. It still installs only while that Hive's operator is away or in Night Watch; protocol changes still wait for them. |
| Engine updates apply when idle | Already true on every packaged Hive: its timer swaps the worker engine once no session is mid-turn, with no opt-out. Nothing to require. |
| Minimum version for shared work | Below it, a Hive keeps its own work but cannot claim Apiary tasks until it updates. |
| Check strictness | Warnings allowed, and which checks must pass before work counts as done. |
| Queen autonomy level | How much each Hive's Queen may do without its operator. |
| Night Watch schedule | When each Hive runs unattended. |
| Terminal history retention | How long each Hive keeps terminal history (7 days today). |
| Worker model and provider | The default AI model workers use: the fleet's cost control. |

**Never policy, refused in code:** credentials, secrets and tokens; workspace
roots and file paths; provider execution permissions (what workers may run);
and remote restarts. "Worker model" chooses which model is used, not what it is
allowed to execute.

**What the member sees:** each locked setting reads "set by your Apiary" with
its value. What the Keeper sees is a Hive's policy state only when a setting is
not in effect, with the reason.

**Built so far (ADR 0113):** the control system end to end, with two settings:
- `release.auto_install`, applied by each member;
- `shared_work.minimum_version`, enforced by the Keeper, because a Hive below it
  is too old to know it exists.

Engine updates already apply when idle on every packaged Hive, so there is
nothing to require. The other five need enforcement before they can be required.

## Fleet health

- **One status per Hive on the roster:** Healthy (nothing extra shown) / Behind
  / Quiet / Failing at a named step / Engine update pending / Low disk. Each
  non-healthy status names its next step. Members report:
  - their sync condition and the step that failed;
  - whether an engine update is pending or deferred;
  - resource pressure;
  - stuck workers.
- **24-hour contact history**, shown only when a Hive is opened, so a recurring
  drop is visible at a glance. No alert of its own.
- **Push alerts** through the existing browser push, for faults only: failing
  sync, schema behind, an engine update that will not apply, low disk. One per
  Hive per fault; it clears silently. Never for a Hive that is merely quiet; the
  2026-09-30 ruling is to warn about silence when you go to use the Hive.
- Not chosen: raising when shared work sits on a quiet Hive.

**Built (ADR 0114):** all three. The member sends its health on its own channel.
The roster shows each faulted Hive's worst condition with its next step, and the
Hive's log view has the day of contact. Push covers sustained sync failure, an
engine update whose swap failed, and disk under 512 MiB, once per fault episode.
Schema behind is pushed by "Hives are behind" already.

## Usage

"Time or token based or both" — both:
- **Tokens** already exist on each Hive, from its provider transcripts by
  workspace and day. Members report them, and the Keeper shows per-Hive and
  Apiary totals by period and model.
- **Active time** is new: worker-busy time from terminal session activity.
- Totals only, no per-message telemetry. Workspace names travel with the
  figures, under the same "share as is" ruling as logs (ADR 0112).

**Built (ADR 0114 addendum):** both. Busy time is credited from the supervision
passes that already run, summed across a workspace's workers, and approximate.
The Keeper's Usage panel shows each Hive, its own included, with Apiary totals
and a split by model over 7 or 30 days.

## Still to decide when building

- The values each setting can take (for example the Queen autonomy levels), and
  what the Keeper's defaults are before it sets anything.
- ~~How "low disk" is defined~~: settled in ADR 0114 with ADR 0083's
  thresholds. It is shown under 2 GiB free and pushed under 512 MiB, and it stays
  raised until the disk is back above 2 GiB.
