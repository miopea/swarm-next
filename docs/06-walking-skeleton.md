# Walking skeleton

Status: **Accepted**

The first runtime milestone proves one complete, production-shaped journey. It
is not a prototype that bypasses authentication, persistence, recovery, or
packaging.

## User story

As the operator, I can install and start Swarm Next, create one task, start one
worker in a configured workspace, interact with its terminal, reload the
browser, recover the exact terminal state, and complete the task. I can update
the application backend without killing the worker.

## Included vertical path

- Single-operator authentication suitable for local testing.
- One workspace and one Claude Code provider adapter. The adapter contract is
  designed against both Claude Code and Codex before implementation.
- Create, view, assign, activate, review, and complete one task.
- Start and stop one immutable worker session.
- Persistent Rust terminal session with bounded history.
- React worker list, task view, and terminal workspace.
- HTTP contract generation and application event resume.
- SQLite migrations, integrity check, and backup.
- Structured traces and an in-app diagnostic report.
- Local service packaging and version endpoint.
- Backend restart with worker and PTY survival.

## Explicitly excluded

- Queen and automated assignment.
- Drones.
- Jira, Outlook, or remote tunnel.
- Pipelines and playbooks.
- Multiple providers.
- Broad legacy import.
- Mobile/PWA packaging.

## Exit criteria

- The full journey works without manual database or terminal intervention.
- Browser reload during continuous output has no missing or duplicate frames.
- Switching between at least two sessions does not reconnect either terminal.
- Backend restart preserves workers and client recovery.
- All queues and buffers expose configured bounds and current usage.
- A 24-hour two-worker soak shows no sustained application memory growth.
- Failure injection covers killed sockets, stalled clients, API restart, worker
  exit, and interrupted snapshot synchronization.
- The feedback bundle is sufficient to distinguish frontend, API, terminal,
  provider, and database failures.
