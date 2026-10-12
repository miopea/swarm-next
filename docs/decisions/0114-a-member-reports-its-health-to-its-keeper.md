# 0114 — A member reports its health to its Keeper

- Status: Accepted
- Date: 2026-10-12
- Decided by: the operator, on 2026-10-12. Asked what helps "keeping my apiary
  hives working well, reporting in, monitoring and such", they chose:
  - one health status per Hive;
  - a 24-hour contact history;
  - push alerts for faults only;
  - usage figures.

  They also set the rule over all of it: "we don't want to generate too much
  random noise for the sake of data that we wouldn't act upon".
- Amends: ADR 0058, for one case: disk about to run out on a member, pushed to
  its Keeper.
- Spec: `docs/specs/apiary-fleet-governance.md`. Task 01a1315b.

## Context

On 2026-10-01 the WSL member failed DNS from 08:08 to 11:15 UTC and had a worker
engine update pending. Neither was visible from the Keeper; the operator found
both by reading logs. All the Keeper knew about a member was the release it last
reported and when it last made contact.

## Decision

### 1. Health travels on its own channel, not in the capability report

A member sends a `HiveHealthReport` with `PUT /api/v1/federation/health`, on its
node credential. The report is bounded and content-free: no paths, no task or
worker names, nothing typed. It carries:
- the sync condition, the step that failed, the last success, and the failure
  count;
- whether a newer worker engine is waiting, and how the last swap ended;
- the tightest disk's pressure and free bytes (ADR 0083's three roles);
- memory pressure (used memory, not stall, which flaps);
- how many workers are stuck: the coordinator's stale and never-briefed work
  findings (ADR 0039).

**Why not the capability report:**
- The Keeper checks a capability report's signature against its own
  re-serialisation, so a field an older Keeper does not know fails the signature.
- Every changed field bumps the report's revision. Fast-changing readings would
  bump it on every pass.

**When it is sent:**
- On every pass, before the pacing and refusal gates, as the log is (ADR 0112).
  A member stopped at a sync step is the one whose Keeper needs to hear which
  step.
- Sampled at most every ten seconds, and sent when it changes or once a minute.

**Across releases:**
- An older Keeper answers 404. That is an ordinary rolling-update state, not an
  error.
- An older member sends nothing. That is not itself a fault.

### 2. The Keeper judges one status per Hive

The domain (`hive_conditions`) lists what is wrong with a member, worst first:
1. quiet;
2. failing sync at a step;
3. schema behind;
4. behind;
5. engine update failing;
6. low disk;
7. short of memory;
8. workers stuck;
9. engine update pending.

A healthy Hive has none, and the roster shows nothing extra for it. A quiet
Hive's own report is not believed, because it describes the time before it went
quiet.

Thresholds reuse what exists:
- **Quiet:** ten minutes without contact (the 2026-09-30 ruling).
- **Low disk:** under 2 GiB is shown, under 512 MiB is pushed (ADR 0083).
- **Short of memory:** memory pressure critical, 95% used.
- **Failing sync is "sustained":** ten minutes without a success.

The roster shows each faulted Hive's worst condition with its next step, and
names the rest. The existing version and contact line goes on saying quiet and
behind, so the health line does not repeat them.

### 3. Contact history: a day of five-minute slots

The Keeper marks the five-minute slot each authenticated member request falls in.
It writes where contact is already recorded, so at most once per half minute per
member. Anything older than a day is deleted as each slot is written, which caps
it at 289 rows per Hive.

The history is shown when a Hive's log is opened, as a strip with the gaps named
in words. It raises nothing by itself.

### 4. Push for faults only, once per episode

Three faults are raised to the Keeper's Needs you, which pushes them through the
existing path (ADR 0019):
- failing sync that ten minutes of retrying has not fixed;
- an engine update whose last swap failed or timed out;
- disk under 512 MiB.

Schema behind is already raised by "Hives are behind". Nothing is raised for a
quiet Hive, or for memory.

- **One raise per Hive per fault episode.** A persisted episode records when the
  fault began, and the raise carries that as its key. An answer holds while the
  fault lasts; a fault that clears and comes back is a new episode, and is
  raised again.
- **It clears by itself.** The raise is withdrawn when the fault ends.
- **An episode ends only when its condition is gone, not when it stops being
  push-worthy.** A disk raised below 512 MiB stays raised until it is back above
  2 GiB, and a sync failure stays raised until a sync gets through. Without that
  hysteresis, a reading hovering at the line would push on every report.
- **A quiet Hive's episodes are left as they are.** Going quiet is not getting
  better.

**Amending ADR 0058.** ADR 0058 refuses a threshold push because it fires
whenever a busy machine crosses the line. This change departs from that for one
reading, disk, and only between Hives:
- the operator asked for low disk explicitly;
- disk does not flap the way CPU and memory do;
- under 512 MiB is imminent failure, not "busy";
- the hysteresis above stops it re-firing at the line.

Memory pressure stays unpushed, as ADR 0058 decided.

### 5. Usage

Usage is part of the same ruling, and the addendum below records how it is
built.

## Consequences

- Schema 198 adds three Keeper tables:
  - `apiary_hive_health`: the latest report per Hive;
  - `apiary_member_contact_slots`: bounded as above;
  - `apiary_hive_fault_episodes`: at most one row per Hive per fault.
- A member on an older release shows only its contact and version until it
  updates.
- The two-Hive run refuses the member's catalog through its proxy. It checks that
  the Keeper sees the member failing at that step while it is refused, and that
  the condition clears by itself once the member recovers.

## Addendum 2026-10-12: usage, tokens and busy time

The operator asked for usage "time or token based or both", and both are built.

**Busy time is new, and it is approximate.**
- Each Hive credits the time between two looks at its workers to the
  workspaces whose worker was busy (provider activity `Active`) at the first
  look.
- The looks are the supervision passes that already run, so busy time adds no
  timer of its own.
- An interval longer than 90 seconds counts as 90, so a machine that slept
  counts for nothing.
- Busy time is worker time, summed across a workspace's workers: two workers
  busy for an hour is two hours.
- It is kept per UTC day and workspace, pruned to the same 60 days as the
  token figures. Each row is bounded to a day for each of 256 workers.

**Tokens already existed on each Hive** (from provider transcripts, by workspace,
day and model) and are reused unchanged.

**A member sends both on its own channel**, `PUT /api/v1/federation/usage`, with
30 days of rows:
- It is sampled at most once a minute, and sent when it changes or every ten
  minutes.
- A report keeps its newest 4,096 rows of each kind.
- Totals only, never per message. Workspace names travel with the figures,
  under the "share as is" ruling of ADR 0112.
- An older Keeper answers 404, which is an ordinary rolling-update state.

**The Keeper keeps each member's latest report** and shows a Usage panel:
- each Hive's weighted tokens and busy time, its own Hive included;
- the Apiary's totals and its split by model, over 7 or 30 days;
- each Hive's workspaces when it is opened.

The weighting is the local Usage page's, so the two panels agree: a comparison
in input-token equivalents, never a price. A member that has never sent figures
reads "no figures yet", not zero. The panel is read with the rest of the
Keeper's page, never on a timer of its own.

Schema 199 adds `worker_active_daily` on every Hive and `apiary_hive_usage` on a
Keeper. The two-Hive run checks that a member's usage reaches its Keeper.
