# 0120 — The operator confirms a ticket's resolution where they asked to

- Status: Accepted
- Date: 2026-10-23
- Decided by: the operator. On decision 01a1690b-1d95-7c61-b196-f710fdb668f5
  they chose "(c) Confirm each Resolved with the operator". Shown the design,
  they said "Yes, we want to have it operator approved for closure", and when
  asked which projects, chose "Support (IS) only".
- Asked for by: the IT support requester on Jira IS-10740, after IS-10736 and
  IS-10688 were resolved with no reply posted.
- Task 01a1693f-3ad7-7271-a647-cea08b9256ed. Investigation: task
  01a16901-82b8-7631-a720-ac9f78c42d21.
- Amends: the operator's rulings of 2026-08-22 and 2026-08-23 that a local move
  never waits on Jira. The local move still never waits; one Jira write does.

## Context

Every transition of a Jira-linked task queues the mapped Jira status
(`queue_jira_transition`). For the IT support project, Completed maps to
Resolved, so whatever completes the task resolves the ticket, whether that is
the coordinator's no-commits sweep, Queen, or the operator.

A support ticket is usually answered rather than built. The worker writes its
answer into the Review handoff, which goes to Queen and the task history and
never to Jira. The sweep then closes the task within seconds, because nothing
was built, and the ticket goes to Resolved with nothing posted on it. IS-10641
on the Keeper went to Resolved 47 seconds after review; its first comment came
23.6 hours later. The requester reopened IS-10736 and IS-10688 to ask what had
happened.

Email already has a guard for this (a completed email task with no reply
delivered stays in Needs you). Jira had none.

## Decision

### 1. A project asks, or it does not

`jira_project_bindings.confirm_resolution`, off by default, is set in Jira
settings as "Ask me before resolving tickets". The operator turns it on for IS.
Website development tickets keep resolving on their own: a dev ticket closed by
a release is the normal case, and asking for each one would be effort the
operator did not choose.

The setting lives on each Hive's own binding, because Jira synchronization
stays direct from each Hive to Jira.

### 2. The task completes; the Jira write waits

On a project that asks, a completion to Completed that the operator did not make
writes the Jira delivery as `held` instead of `queued`. Everything local happens
as before: the task completes, the worker's slot frees, the evidence stands, and
Queen's no-deployment approval works unchanged.

Holding the task in Review instead was rejected. It would turn the sweep and
Queen's completion aside, make Review mean both "unfinished" and "waiting on
the operator", and break the 2026-08 rulings on the local side as well.

`held` lives in `jira_transition_deliveries` itself, not beside it. The sync
treats Jira as the truth whenever no write is outstanding, so a hold kept
elsewhere would let the next sync see "In Progress" and reopen the completed
task.

### 3. Whose completion waits

- Held: the coordinator's sweeps, and Queen.
- Not held: the operator's own completion, which is the confirmation; and a
  change that came from Jira, which writes nothing back.

### 4. What the operator sees and does

One Needs-you card per held ticket, counted in the badge and in push like the
email replies. It says whether a comment has ever reached the ticket ("No reply
has been posted on this ticket") and shows the worker's handoff.

- **Resolve in Jira** moves the write from `held` to `queued`; the ordinary
  sender delivers it.
- **Leave open** moves it to `declined`. The task stays completed and Jira is
  untouched. A later Jira change does not turn it into a conflict to retry.

The task's Jira line reads "Waiting for you to resolve it in Jira" or "Left open
in Jira".

### 5. Nothing times it out

A held resolution waits until the operator answers or Jira moves the ticket
itself. A timeout that resolves is the original bug; one that drops the write
does so silently. If Jira moves the ticket to a status mapped to Completed,
the write counts as made; any other move makes it a conflict, as for a write
that was sent. Turning the setting off affects future completions only.

Bounds: one outstanding write per task (the unique index now covers `held`).
Held writes are not sends, so they neither count against nor are refused by the
256-write send bound.

## Consequences

- Schema 207 adds the column and rebuilds `jira_transition_deliveries` and
  `notification_deliveries` to widen their CHECKs.
- The setting does nothing until the operator turns it on, on each Hive that
  works support tickets.
- The release settle (`settle_released_work`) writes the task state with its own
  SQL and has never queued a Jira transition, so a Jira-linked task closed by a
  release is not resolved in Jira at all. That predates this decision and is not
  changed by it.
