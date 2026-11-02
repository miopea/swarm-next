# Live control-room events

Status: **Implemented foundation**

Swarm now refreshes roster, session, task, and runtime snapshots without
requiring the operator to press Refresh. The feed is an authenticated,
content-free invalidation channel rather than a second copy of domain state.

## Contract

- `GET /api/v1/control-room/events?after=<cursor>` long-polls for up to 20
  seconds and returns a typed page with a resumable cursor.
- Events contain only sequence, Hive identity, event kind, and timestamp. Task
  text, workspace paths, terminal output, and credentials never enter the feed.
- The local database retains at most 4,096 events and each response returns at
  most 128. A stale or invalid cursor returns `reset_required` so the browser
  reloads canonical snapshots.
- Task, worker, and durable session-binding changes record their event in the
  same database transaction as the state change.
- The browser owns one abortable feed per authenticated application instance.
  Reconnect delay is bounded at five seconds and is cancelled on logout,
  unmount, or token change.
- A browser acknowledges a cursor only after snapshot invalidation succeeds.
  Transient snapshot failures therefore retry the same event instead of losing
  it.

## Recovery model

The feed never applies business mutations in React. It tells the browser which
typed subsystem changed, and the browser reloads the canonical snapshots that
kind can change (ADR 0142). A reset, startup, a runtime change or an unknown
kind reloads all of them. A worker's provider activity is a worker change.
Worker and task changes are held for 250 ms, and at least a second after the
previous refresh, then read again from the same cursor, so a burst costs one
refresh. Manual Refresh remains available as an explicit recovery control.
Notify wakeups reduce latency, while the long-poll timeout guarantees another
database read even if a wakeup is missed.

## Verification

Domain tests cover typed event parsing. Persistence tests cover atomic writes,
bounded retention, pagination, cursor reset, and schema migration. API tests
cover authentication, cache prevention, cursor responses, and the absence of
sensitive content. Browser-unit tests cover cursor resume, cancellation,
bounded retry, reset recovery, and no acknowledgement before refresh succeeds.