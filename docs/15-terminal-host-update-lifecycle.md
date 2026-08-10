# M1 terminal-host update lifecycle

Status: **Implemented drain foundation**

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

No update path silently kills workers. A security update that cannot wait for
drain requires a separate explicit controlled-restart action that communicates
impact before stopping anything.

## Same-user lifecycle contract

IPC protocol version 5 adds:

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

Drain mutations remain same-user IPC operations for the packaged updater; they
are not browser/API mutations.

## Atomicity

Session creation and the transition into drain mode share the terminal session
registry lock. A start either commits before drain begins and is counted, or it
observes drain and fails. There is no interval in which the updater can observe
drain while an uncounted PTY is created behind it.

Cancellation takes the same lock before allowing starts again. Status counts
live provider processes rather than retained registry entries, so a completed
worker cannot hold an update indefinitely.

## Graceful replacement sequence

The packaged updater will:

1. verify the replacement host protocol is compatible;
2. request `begin_drain` from the current host;
3. keep the current binary and socket authoritative while running sessions are
   non-zero;
4. poll `host_status` with a bounded interval or cancel drain;
5. once running sessions reach zero, send the host a graceful interrupt;
6. wait for process exit and Unix-socket removal;
7. start the replacement host and verify its status;
8. roll back to the retained old binary if replacement health fails.

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
