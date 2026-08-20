# Product journeys

Status: **Proposed**

Journeys describe the product contract at the user's altitude. Implementation
work is accepted against these outcomes rather than legacy route parity.

## J1: Resume the control room

The operator opens Swarm after a browser reload, browser restart, sleep, or
network interruption.

Success means:

- the worker list and task state render from one consistent snapshot;
- the selected terminal restores its canonical screen exactly once;
- live output continues after the snapshot without gaps or duplication;
- no stale browser connection can send input to a replacement worker session;
- the UI clearly distinguishes synchronizing from live;
- recovery does not require a hard refresh.

## J2: Move among active workers

The operator switches repeatedly among workers while several produce output.

Success means:

- switching is perceptually immediate;
- switching does not reconnect or reset either terminal;
- scroll position, selection, cursor, title, and wrapping are retained;
- hidden terminals do not send invalid resize dimensions;
- background output is bounded and catches up deterministically.

## J3: Create and drive work

The operator creates a task, provides context, chooses or accepts an assignee,
observes progress, intervenes when necessary, and reviews completion.

Success means:

- task state and worker assignment are never contradictory;
- all material transitions appear in a human-readable history;
- automatic decisions disclose their policy and evidence;
- the operator can override automation without corrupting the workflow;
- completion and verification are distinct, explicit states when verification
  is enabled.

## J4: Start and recover a worker

The operator starts an agent, observes startup, and recovers from process exit
or provider failure.

Success means:

- every process incarnation receives an immutable session ID;
- startup failure has a clear cause and safe retry;
- recovery never confuses the old and new process;
- retained task association is deliberate and visible;
- terminal history from an old session is not presented as live state for a new
  session.

## J5: Update Swarm during active work

The operator installs a new version while workers are active.

Success means:

- compatibility is checked before cutover;
- PTYs and workers continue running;
- API and browser clients resynchronize automatically;
- health verification determines success;
- failure rolls back the application version without losing workers;
- the UI reports each phase in plain language.

## J6: Handle attention and decisions

An agent needs input, a policy decision, credentials, conflict resolution, or
operator judgment.

Success means:

- one decision inbox consolidates attention across subsystems;
- the reason, requesting worker, affected task, risk, and suggested action are
  visible;
- provider-native approval modes are not duplicated;
- time-sensitive items remain distinguishable from informational messages;
- resolving an item changes the underlying domain state atomically.

## J7: Work remotely

The operator reaches Swarm through an approved remote-access adapter.

Success means:

- authentication and session behavior match the remote threat model;
- terminal input requires an explicit, short-lived authorization;
- secrets do not appear in URLs or logs;
- a dropped mobile/background connection resumes without a full replay storm;
- remote access can be disabled without affecting local operation.

## J8: Diagnose a problem

The operator reports or investigates unexpected behavior.

Success means:

- the UI identifies the unhealthy subsystem;
- feedback captures version, trace IDs, session IDs, recent state transitions,
  terminal sequence position, and sanitized logs;
- diagnostics distinguish browser, API, terminal plane, provider, database, and
  integration failures;
- collection is previewable and privacy-safe.

## J9: Install or migrate

The operator installs Swarm or imports selected legacy data.

Success means:

- one installer and one service entry point are presented;
- prerequisites are checked before mutation;
- legacy data is read from a snapshot, never a live shared database;
- import reports kept, transformed, skipped, and invalid records;
- rollback never modifies the legacy installation.

