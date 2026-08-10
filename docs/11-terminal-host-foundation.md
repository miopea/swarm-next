# M1 terminal-host foundation

Status: **Implemented foundation**

This increment establishes the process boundary required for terminal sessions
to survive API replacement. It does not yet claim the full walking skeleton.

## Implemented

- Real PTY spawn, input, resize, exit query, and explicit stop.
- Immutable worker-session identity.
- Byte- and frame-bounded sequence journal with deterministic snapshot fallback.
- Configured workspace-root enforcement and a hard session-count limit.
- A dedicated terminal-host executable independent from the HTTP API.
- Versioned, size-bounded JSON control protocol over a Unix socket.
- Runtime directory mode `0700`, socket mode `0600`, and same-user peer checks.
- Bounded concurrent IPC connections and request-read deadlines.
- Provider-specific `StartClaude`; no arbitrary-command operation in IPC or HTTP.
- Operator bearer authentication on all terminal HTTP routes.
- API reconstruction and terminal-host client reconnection with a live PTY.
- Event-driven host waits, one-time attach grants, and browser WebSocket replay
  are implemented in the follow-on browser attachment increment.
- Canonical active-screen snapshots and bounded renderer recovery are
  implemented in the follow-on canonical recovery increment.
- Bounded, checksummed, same-user on-disk output history and content-free
  diagnostics are implemented in the durable-history increment.

## Process ownership

```text
Browser adapter
    |
    | authenticated HTTP (WebSocket attachment follows)
    v
Rust API process
    |
    | same-user Unix socket, typed bounded frames
    v
Rust terminal-host process
    |-- PTY masters and child processes
    |-- bounded sequence journals
    `-- terminal session registry
```

Killing or replacing the API process does not close a PTY descriptor because
the API never owns it. Stopping a worker remains an explicit request to the
terminal host.

## HTTP surface

All terminal routes require `Authorization: Bearer <operator token>`:

- `GET /api/v1/terminal/sessions`
- `GET /api/v1/terminal/history/diagnostics`
- `POST /api/v1/terminal/sessions`
- `GET /api/v1/terminal/sessions/{id}/output?after={sequence}`
- `POST /api/v1/terminal/sessions/{id}/input`
- `PUT /api/v1/terminal/sessions/{id}/size`
- `DELETE /api/v1/terminal/sessions/{id}`

The API binds to loopback. The terminal host independently validates workspace
roots and provider commands, so HTTP authorization is not its only boundary.

## Proven failure behavior

- Output exceeding the byte or frame budget evicts oldest deltas and requires
  a canonical snapshot instead of expanding memory.
- A single frame larger than the journal cannot break the memory limit.
- One hundred thousand sustained frames remain within the configured bound.
- Relative or out-of-root workspaces fail closed.
- Zero terminal dimensions fail closed.
- Exhausted process or connection limits stop accepting more work.
- Symlink runtime directories are rejected before permissions are changed.
- Dropping and recreating the API preserves the host-owned session and output.

## Remaining M1 work

- Bounded historical-scrollback retrieval from the durable store.
- Host restart/update descriptor handoff or explicit compatibility fallback.
- Packaged service lifecycle, diagnostics, and longer resource soak.
