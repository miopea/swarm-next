# 0142 — An open page reloads what an event can change

- Status: Accepted
- Date: 2026-11-02
- Decided by: the operator, on the optimization pass before the next release
  (2026-11-02). They chose "Do everything listed first"; this is its first
  item, "control room refetch: refetch by event kind plus a minimum gap or
  coalescing".
- Amends: ADR 0024, which made an activity change "a content-free runtime
  event". It is now a worker event. `docs/22-live-control-room-events.md` is
  updated to match.

## Context

Every open control room long-polls the event feed and reloads when an event
arrives. Measured read-only on the operator's Hive over the 24 hours to
2026-11-02 14:46Z:

| Event | Count |
| --- | --- |
| workers_changed | 1,404 |
| runtime_changed | 941 |
| decisions_changed | 841 |
| tasks_changed | 779 |
| presence_changed | 104 |
| sessions_changed | 27 |

Each event except presence and notifications reloaded the whole snapshot.
That is seven endpoints, plus Queen's automation status. Measured one call at
a time on the same Hive:

| Endpoint | Size | API CPU |
| --- | --- | --- |
| hive | 0.5 KB | 1 ms |
| terminal sessions | 7 KB | 2 ms (host 8 ms) |
| workers | 30 KB | 19 ms (host 15 ms) |
| workspaces | 9 KB | 7 ms |
| tasks | 172 KB | 13 ms |
| decisions | 1,050 KB | 26 ms |
| Jira task links | 32 KB | 4 ms |
| Queen automation status | 0.3 KB | 31 ms |

All eight in parallel took 118 ms of API CPU, 32 ms of host CPU and about
1.3 MB. At one reload per event, that is up to about 5 GB a day for one open
page.

Three things made it worse than it needed to be:

1. **A worker's activity was a runtime change.** Starting or finishing a turn
   recorded `runtime_changed`, and a runtime change also reloads Queen's
   policy, presentation preferences and engine capabilities. Provider activity
   is the emitter that fires on every turn. The others record settings, Apiary,
   federation and engine maintenance changes, so they are expected to be rare.
   The live check below tests that.
2. **The decision inbox came with every reload.** At 1 MB it is the largest
   read, and a worker change cannot move it.
3. **The settle window did not coalesce.** Worker and task pages waited 250 ms
   before refreshing, but the feed then acknowledged only the page it already
   had. Events that landed during the wait came back as the next page and
   cost a second refresh. Of 3,991 gaps between refreshing events, 1,408 were
   under a second (same timestamp) and 545 were one second.

## Decision

1. **An activity change is a worker change.** The roster look records
   `workers_changed` when any worker's activity, background work, turn failure
   or refusal changes. All four are part of the worker's roster row and
   nothing else the browser reads.
2. **A page reloads only what an event's kind can change.** That was read from
   what each endpoint is built from, and which event each write to its tables
   records:

   | Event | Reloads |
   | --- | --- |
   | workers_changed, sessions_changed | hive, sessions, workers, workspaces, tasks |
   | decisions_changed | hive, sessions, workers, tasks, decisions |
   | tasks_changed | hive, sessions, workers, tasks, decisions, Jira links, steward assists |
   | runtime_changed, a reset, startup, or a kind the browser does not know | everything, as before |
   | presence_changed, notifications_changed | nothing in the snapshot, as before |

   - Workers, sessions and tasks are read for every kind:
     - a worker's row carries decisions, deliveries and presence;
     - a task's next move carries decisions and worker liveness;
     - terminal selection reconciles workers against sessions.
   - The decision inbox reads decision requests, deliveries, clarifications,
     native-answer refusals and task-decision links. Every production write to
     them records `decisions_changed` in the same transaction. The exception is
     task-decision links, which record `tasks_changed`.
   - The parts a page reloads replace their old values in one commit. A
     command that commits while they load still supersedes them, as before.
3. **Worker and task changes wait, then read again.** The feed holds a page
   made only of worker and task changes for 250 ms, and at least one second
   after the last refresh began. Then it reads again from the same cursor.
   That page carries every change that landed during the hold, so they cost
   one refresh. A page is held once per refresh, so a steady stream still
   refreshes about once a second. A page that carries a decision, runtime,
   session, presence or notification change is not held. A decision that lands
   while a worker or task page is already held goes with it when the hold
   ends, up to a second later.
4. **Two reads follow the new kinds.**
   - Which workers run behind the installed provider release changes when one
     restarts onto it. So the page re-reads provider capabilities on a worker
     change too, as it did when every activity flip was a runtime change.
   - Turning the "answer in the terminal" switch now records
     `decisions_changed`. Every inbox entry's label reads that switch, and the
     write recorded no event at all.

## Consequences

Estimated from the per-endpoint costs above. These are not a measurement of the
whole:
- A worker change costs 6 reads, Queen's status included: about 73 ms of API
  CPU and about 220 KB. Before, it cost 8 reads, about 103 ms and about
  1.3 MB, and an activity flip three more reads as a runtime change.
- A decision or task change saves the reads its kind cannot move: 7 to 11 ms.
- On the day measured, about 2,345 worker changes (1,404 plus 941 activity
  flips) would each skip the decision inbox: up to about 2.4 GB of the 5 GB.
- Coalescing removes refreshes on top of that. How many depends on sub-second
  timing the event table does not record.

Accepted:
- **Roster and board changes show up to a second later** during a burst,
  against 250 ms before. A lone change still shows after 250 ms. A decision
  that lands during a held burst waits for it too.
- **Some displayed values change with no event at all.** Examples are a
  session's CPU and memory figures, a new folder under a workspace root, and
  Jira settings writes. They refreshed by coincidence with every event. Now
  they refresh with the events that read them. Worker changes are the most
  frequent kind and reload sessions and workspaces.
- **The Apiary page's busy time refreshes less often.** Its refresh key
  follows runtime and task changes, and activity no longer counts as runtime.
  Its figure is approximate by design.
- **The map is a maintained contract.** A new endpoint added to the snapshot,
  or a new write to a table one reads, has to keep its row true. A kind the
  browser does not know still reloads everything.

## Not proven

- **The live effect.** It needs a reload. Prediction: `runtime_changed` falls
  from about 940 a day to a handful, `workers_changed` rises by about as many,
  and API CPU per open page falls. This is wrong if `runtime_changed` stays
  near its old rate, which would mean another frequent emitter.
- **Browser cost.** Parsing a 1 MB inbox and re-rendering what reads it on
  every worker change is not measured here.
- **Completeness of the write audit.** It covered the decision inbox and Jira
  link tables by reading their write sites. The roster, sessions and board are
  read for every kind, so they did not need one.

## Evidence

Tests:
- `an_activity_change_is_a_worker_change_not_a_runtime_change`: an activity
  change records exactly one `workers_changed`, and no change records nothing.
- `%s reloads exactly what it can change`: the map for each kind, including an
  unknown kind and a reset (all parts), and presence and notifications (none).
- `a page carrying several kinds reloads what any of them can change`.
- `a worker change reloads the roster, sessions, board and workspaces, and keeps
  the decision inbox`: the partial read merges, and the inbox keeps its value.
- `a partial read asks only the endpoints for its parts`: a worker change
  requests exactly five paths.
- `changes that land while a burst is held cost one refresh, read from the same
  cursor`.
- `worker and task refreshes keep a minimum gap, and a decision is not held by
  it`.
- `a worker change re-reads which workers run behind the installed release`:
  the Settings release card appears after a worker change alone.
- `turning_the_switch_tells_open_pages_the_inbox_changed`: the switch records
  exactly one `decisions_changed`.
- The existing feed and model tests still pass: cancellation, supersession, no
  acknowledgement before a refresh succeeds, retry and the hung-poll ceiling.

Ablations, each failing at least one of those tests, with the file restored
byte for byte afterwards:
- Activity recorded as `runtime_changed` again.
- Worker changes also reloading the decision inbox.
- Task changes no longer reloading the decision inbox.
- The held page refreshed without the second read.
- The hold without the minimum gap.
- Provider capabilities re-read on runtime changes only.
- The switch recording no event.
