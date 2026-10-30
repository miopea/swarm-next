# 0132 — Settings that need a worker restart say which workers

- Status: Accepted
- Date: 2026-10-29
- Decided by: the operator. They had turned on Queen releases (ADR 0131)
  straight after restarting every worker, and asked "what do you mean turn it
  on?". Then: "if a setting change requires a worker [restart], we should flag
  the user so the know that, and the notification in the left panel should show
  it", and "This should apply for any setting that requires a worker restart, or
  if there are settings that require a app restart too". Task 01a18576.
- Builds on: ADR 0116 (the worker model and its launch record), ADR 0122 (Remote
  Control for workers), ADR 0131 (Queen releases), and the rail's runtime
  update rows.

## Context

Four operator settings reach a Claude worker only when it next starts, because
Swarm writes them beside the worker's config before launch and Claude reads them
once:

| Setting | Where it is set | What a running worker keeps |
| --- | --- | --- |
| Worker model (ADR 0116) | Settings → Workers | the model it started on |
| Remote Control (ADR 0122) | Settings → Workers | `remoteControlAtStartup` as launched |
| Queen releases (ADR 0131) | Settings → Workers | its auto-mode rule and its brief |
| System access, per worker | the worker's own settings | `permissions.allow` as launched |

Each card said so in a line of small print. Only the worker model counted the
workers still behind, and only on its own card. Nothing in the rail, which the
operator watches, said that a change was waiting on restarts.

No setting in the UI needs an App and API or worker engine restart. The values
read once at process start are environment variables in `swarm.env`, which the
UI does not write. The operator token is swapped into the running process.
Terminal history retention already reports when the running worker engine is
too old for it. App and API builds and worker engine updates already have their
own rail rows.

## Decision

1. **Each Claude launch records what it carried.** At start, Swarm writes the
   Remote Control and Queen releases choices and the worker's system access
   into its settings file. Schema 216 adds `worker_launch_choices`, one row per
   worker, keyed to the session it describes. Each writer reports the value the
   file now holds, and a value it could not write is recorded as NULL. The
   model keeps its own launch record from ADR 0116.
2. **A running worker is behind a setting when:**
   - its recorded launch value differs from the current one. A setting turned
     on and back off before anyone restarted asks nothing.
   - Swarm could not write the value. A restart writes it again.
   - its session has no record because it started before this one was kept, and
     the setting changed at or after the session started. The same second
     counts as behind.
   - for the model, ADR 0116's own rule applies.

   The rule is `swarm_domain::session_behind_setting`. Queen is not counted for
   Queen releases, because her review prompt carries it at her next review and
   she never runs the commands its rule allows.
3. **`GET /api/v1/runtime/settings-restart`** lists each running worker that is
   behind, with the settings it is behind on. **`POST`** restarts exactly those
   workers, through the same stop-and-revive path as restarting every worker.
   The list is read under the worker lifecycle lock, never taken from the
   caller, so a page left open cannot end a session that already carries the
   change.
4. **The rail shows one row while any worker is behind:**
   - its label is "Queen releases needs a restart", or "Settings need a
     restart" for more than one setting;
   - its action is "Restart", which confirms the cost first;
   - its cost line is "Restarts N running workers".

   Settings → Updates has the full card, naming each worker and its settings.
   The rail is refreshed as soon as the control-room feed reports a runtime,
   worker or session change, not at the next 15-second tick. A failed read
   keeps the row and offers no restart, as the provider row does.
5. Each restart-bound card's small print says the left panel lists the workers
   still on the old setting.

## Consequences

- Workers running when this ships have no launch record, so they are judged
  by time until each next starts.
- A successor session from continuation recovery relaunches from its parent's
  settings file. It takes over the parent's launch record, as it takes over the
  model's, so it is not mistaken for a session started after the change.
- Claude's in-place `/restart` for a provider update (ADR 0122) keeps the
  session, and the launch record with it. That is correct: the host does not
  rewrite a worker's startup settings for an in-place restart, so the worker
  still carries what it launched with.
- A worker whose settings file is not a JSON object is listed as behind on
  Remote Control and Queen releases on every start, because Swarm never writes
  into that file. That file is the operator's to fix, as ADR 0116 already says
  of the model.
- Codex workers are never listed. None of these settings reach them.
- The restart interrupts whatever the listed workers are running, as the
  provider restart does. Nothing restarts them on its own.
