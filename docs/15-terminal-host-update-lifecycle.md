# M1 terminal-host update lifecycle

Status: **Implemented**

This increment makes the accepted compatible-host fallback concrete. It keeps
active workers on the existing terminal host during application updates and
provides an atomic drain boundary before the host binary itself is replaced.

## Operator outcome

- API and browser updates do not interrupt workers.
- When the terminal host needs replacement, the product can show that the
  update is pending on a specific number of running sessions.
- Existing terminals remain interactive while drain is active.
- Starting a new worker fails with a specific update-drain error instead of
  racing the replacement.
- The operator can cancel drain and resume normal session creation.
- Exited sessions do not block replacement; their output remains in durable
  history.

No update path silently kills workers. Settings now offers a separate,
operator-confirmed controlled restart when an always-active Queen means the
automatic idle boundary will never arrive.

## Same-user lifecycle contract

The host IPC contract includes:

- `host_status`
- `begin_drain`
- `cancel_drain`

Status contains only:

- IPC protocol version;
- terminal-host binary version;
- drain state;
- running-session count;
- retained in-process session count.

It contains no terminal output, workspace path, command, or credential. The
authenticated HTTP read surface is:

`GET /api/v1/runtime/terminal-host`

Drain mutations remain same-user IPC operations for the packaged updater. The
browser can request one managed maintenance transaction, but cannot issue raw
drain, service, path, or package commands.

## Atomicity

Session creation and the transition into drain mode share the terminal session
registry lock. A start either commits before drain begins and is counted, or it
observes drain and fails. There is no interval in which the updater can observe
drain while an uncounted PTY is created behind it.

Cancellation takes the same lock before allowing starts again. Status counts
live provider processes rather than retained registry entries, so a completed
worker cannot hold an update indefinitely.

## Package update and graceful replacement

A normal compatible package update:

1. verifies the release checksum and exact host protocol;
2. retains the old API/browser release and its hashed assets;
3. atomically switches the API/browser `current` pointer;
4. restarts and health-checks only `swarm-next-api.service`;
5. leaves `host-current`, the host PID, socket, and all worker PTYs untouched.

The explicit `reconcile-host` action:

1. verifies exact protocol compatibility;
2. requests `begin_drain`, atomically preventing new worker creation;
3. reads `host_status` and defers with drain cancellation if any session runs;
4. at zero sessions, switches `host-current` and restarts only the host service;
5. verifies the replacement host through same-user IPC;
6. restores and verifies the retained host release if replacement fails.

The package also installs a bounded user timer. Every two minutes it performs
the same atomic drain-and-status check. Active sessions make that check a
successful no-op and admission is reopened immediately; once the count reaches
zero, the pending compatible host release is activated and health-checked.
Operators therefore do not need to remember a maintenance command after the
last worker naturally stops.

## Operator-confirmed restart

When the API and host releases differ, Settings can prepare a worker-engine
update. A second explicit confirmation names the number of active workers and
states that their terminal processes will close. The authenticated API then:

1. serializes the operation against every ordinary worker start/stop;
2. stops each live process and detaches only its process bindings;
3. retains stable worker ownership, provider conversation IDs, tasks, and
   autostart policy;
4. writes one bounded request marker in the private Hive state directory;
5. lets a systemd path unit run the already constrained package reconciler;
6. waits for the exact API release identity from the replacement host;
7. removes the request marker and revives Queen plus configured autostart
   workers through their normal recovery path.

If the trigger or replacement fails, the lifecycle lock is released and the
same supervisor revives configured workers on the available host. The API has
no generic command execution surface, and the browser supplies no filesystem
path or service name.

The terminal-host executable now handles graceful interrupt by dropping its
listener, which removes its owned socket path. Abrupt process death is still a
distinct failure-recovery case for the service manager; it must verify process
ownership before removing a stale socket.

## Validation

- Drain with a live real PTY reports one running session.
- Existing input/output continues during drain.
- New sessions fail with `HostDraining` after the transition.
- Canceling drain permits creation again.
- A naturally exited process reports zero running sessions even while its
  registry/history entry remains retained.
- Same-user IPC reports drain state and protocol version.
- Authenticated HTTP status is marked `Cache-Control: no-store`.
- A live process-level smoke with a real Claude PTY proved drain preserved the
  existing worker, rejected a second start, cancellation worked, graceful
  interrupt removed the socket, and a replacement protocol-v5 host rebound the
  same path without manual deletion.
- The package lifecycle smoke proves API update and rollback do not stop or
  restart the host with an active session, host reconciliation refuses that
  session, and later reconciliation advances the independent host pointer at
  zero sessions.
- An API integration test starts an explicitly old host with a live real PTY,
  submits managed maintenance, observes the process stop, replaces the host at
  the request marker, verifies the exact current version, and confirms marker
  cleanup.
