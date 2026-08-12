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

**M1: Durable work foundation**

M1 proves bounded terminal history, immutable worker-session identity, stable
browser attach/detach, survival across browser and API restarts, and one
persisted task-to-worker journey. Promotion requires the two-worker browser
recovery test and 24-hour resource soak; implementing later product surfaces
does not waive that evidence gate.

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

Run the browser client with `pnpm --dir web dev` on the fixed development URL
`http://127.0.0.1:8766`. It proxies `/health` and
HTTP/WebSocket `/api` traffic to `127.0.0.1:8765`. Unlock the development UI
with the same operator token configured for the API. The token remains in the
current tab's session storage so refreshes stay unlocked; Lock or closing the
tab clears it. Do not commit or log operator tokens.

Inspect or drive the terminal-host update drain through the typed lifecycle
client. It never kills workers or deletes sockets:

```sh
cargo run -p swarm-cli --bin swarmctl -- status
cargo run -p swarm-cli --bin swarmctl -- drain
cargo run -p swarm-cli --bin swarmctl -- wait-ready 300
cargo run -p swarm-cli --bin swarmctl -- cancel-drain
```

## Ubuntu/Debian dogfood package

Build the release on Linux, extract it on the target host, and install it as
unprivileged systemd user services:

```sh
./packaging/linux/build-release.sh
tar -xzf dist/swarm-next-0.1.0-<commit>-linux-x86_64.tar.gz
./swarm-next-0.1.0-<commit>-linux-x86_64/swarm-next-package install ./swarm-next-0.1.0-<commit>-linux-x86_64
```

The packaged UI listens on `http://127.0.0.1:8766`. Releases are staged under
`~/.local/lib/swarm-next`, configuration is written once under
`~/.config/swarm-next`, and durable terminal history remains under
`~/.local/state/swarm-next`. Content-hashed browser assets are retained under
`~/.local/lib/swarm-next/assets` so a tab opened before an update can still load
a deferred terminal module afterward. The writable workspace defaults to
`~/swarm-workspaces`; set `SWARM_WORKSPACE_ROOT` during the first install to
choose a different absolute path. Use `update RELEASE_DIR`, `rollback`, or
`uninstall`. Uninstall preserves configuration and state.
Compatible updates switch the API and browser release, then restart only
`swarm-next-api.service`; the independently versioned terminal-host process and
its worker PTYs stay alive. Run `swarm-next-package reconcile-host` when workers
are idle to move the sidecar to the current release.
When a release changes the terminal protocol, stop all Swarm Next workers and
run `swarm-next-package migrate-protocol RELEASE_DIR`. The migration drains the
old host, refuses active sessions, switches the API and sidecar together, and
restores both previous pointers if health verification fails.

Claude runs with an isolated profile at
`~/.local/state/swarm-next/providers/claude`, keeping the host-wide profile
read-only. Authenticate that profile once before dogfooding:

```sh
CLAUDE_CONFIG_DIR="$HOME/.local/state/swarm-next/providers/claude" claude
```

The user service runs while the user manager is active. A remote host that must
keep running after logout may require an administrator to enable user lingering;
the installer does not elevate privileges or change that host policy.

Each immutable release ID includes its Git commit. The builder refuses a dirty
worktree so an installed artifact always maps back to one reviewable source
state and can coexist with later `0.1.0` dogfood builds.
