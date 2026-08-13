# M1 `swarmctl` lifecycle client

Status: **Implemented CLI foundation**

`swarmctl` is the platform-neutral operator and packager client for the
same-user terminal-host lifecycle contract. It keeps update truth in typed Rust
IPC while allowing systemd, an installer, a remote Linux deployment, or a
future desktop packager to orchestrate the same behavior.

## Commands

```text
swarmctl status
swarmctl drain
swarmctl cancel-drain
swarmctl wait-ready [timeout-seconds]
swarmctl stop-session UUID
```

All successful commands print one compact JSON terminal-host status object per
line. Expanded for readability, the shape is:

```json
{
  "protocol_version": 5,
  "host_version": "0.1.0",
  "draining": true,
  "running_sessions": 2,
  "retained_sessions": 3
}
```

`wait-ready` observes an already-started drain. It does not begin or cancel
drain implicitly. The default timeout is five minutes; callers may select one
through 86,400 seconds. Polling is a bounded status observation optimization,
not a correctness mechanism: the registry's drain state and running count are
authoritative.

## Exit behavior

- `0`: command succeeded; for `wait-ready`, running sessions reached zero.
- `1`: IPC, host rejection, unexpected response, protocol mismatch, or waiting
  without an active drain.
- `2`: invalid command or timeout argument.
- `3`: drain remained blocked by running sessions at the timeout.

Errors go to standard error and never include terminal bytes, operator tokens,
workspace paths, or provider input.

## Safety boundary

`swarmctl` never deletes sockets or replaces binaries. `stop-session` is the
one explicit maintenance escape hatch: it asks the owning host to stop one
exact UUID and then reports authoritative host status. It has no bulk, name,
or wildcard form. The service manager still owns process identity and installed
binary paths; ordinary updates establish drain state and replace the host only
when readiness is true.

The socket path defaults to
`$HOME/.local/state/swarm-next/run/terminal.sock` and may be overridden with
`SWARM_TERMINAL_SOCKET` for isolated deployments and side-by-side dogfooding.

## Development usage

```sh
cargo run -p swarm-cli --bin swarmctl -- status
cargo run -p swarm-cli --bin swarmctl -- drain
cargo run -p swarm-cli --bin swarmctl -- wait-ready 300
cargo run -p swarm-cli --bin swarmctl -- cancel-drain
cargo run -p swarm-cli --bin swarmctl -- stop-session SESSION_UUID
```

## Validation

- Unknown commands and out-of-range timeouts fail closed.
- A protocol mismatch is distinguished from ordinary host rejection.
- Real same-user IPC drives begin/cancel status transitions.
- A live session produces exit-code-3 timeout behavior and reports its count.
- `stop-session` stops one exact real IPC session and returns the authoritative
  zero-running status; malformed UUIDs fail before connecting.
- Waiting without drain fails instead of silently changing host state.
- A live executable smoke with a real Claude PTY proved compact JSON output,
  exit code 3 with one active session, cancellation, and zero-session readiness.
