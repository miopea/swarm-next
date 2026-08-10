# Swarm Next

Swarm Next is a ground-up redesign of Swarm: a persistent control room for AI
coding agents. It preserves proven user outcomes while replacing accidental
architecture, obsolete automation, and implementation-driven product behavior.

The product and architecture baseline is accepted. Runtime development now
begins with the terminal-first walking skeleton.

## Intended product qualities

- Agent sessions outlive browsers, UI components, and application updates.
- Switching terminals feels like switching editor tabs: immediate and stable.
- Reload, sleep, reconnect, and update are routine recovery paths.
- Every queue, buffer, and retained history has an explicit bound.
- Core state transitions are typed, transactional, observable, and testable.
- Integrations extend the product through declared application interfaces.
- Operators install, run, update, and diagnose one application.

## Implementation direction

- Rust modular monolith for the application and terminal/session backend.
- A TypeScript browser adapter, currently rendered with React.
- SQLite as the embedded source of truth, owned by one persistence boundary.
- Versioned HTTP, event, and terminal synchronization contracts.

React does not own terminal or worker lifetime and remains replaceable behind
the browser adapter boundary. See [docs/README.md](docs/README.md) for the
accepted decisions and continuing review sequence.

## Relationship to legacy Swarm

The legacy `miopea/swarm` repository remains the stable daily driver and an
executable source of product evidence. Swarm Next does not port a module merely
because it exists. Each capability is classified as keep, redesign, merge, or
remove before implementation.

## Current milestone

**M1: Terminal foundation**

M1 proves bounded terminal history, immutable worker-session identity, stable
browser attach/detach, and eventual survival across browser and API restarts.

## Development

```sh
cargo test --workspace
pnpm install --frozen-lockfile
pnpm check
pnpm test
pnpm build
```

Run the terminal host and API in separate shells with a shared socket path. The
host and API are separate process owners shipped as one application:

```sh
export SWARM_TERMINAL_SOCKET="$HOME/.local/state/swarm-next/run/terminal.sock"
export SWARM_TERMINAL_HISTORY_DIR="$HOME/.local/state/swarm-next/terminal-history"
export SWARM_WORKSPACE_ROOTS="/absolute/path/to/workspaces"
cargo run -p swarm-terminal-host

# In the API shell, set the same socket and a development-only secret.
export SWARM_OPERATOR_TOKEN="replace-with-a-long-random-development-token"
cargo run -p swarm-api
```

Run the browser client with `pnpm --dir web dev`. It proxies `/health` and
HTTP/WebSocket `/api` traffic to `127.0.0.1:8765`. Unlock the development UI
with the same operator token configured for the API. The token remains in
browser memory; do not commit or log operator tokens.
