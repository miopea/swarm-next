# Working on Swarm itself

Moved out of the README on 2026-09-11, when the operator settled that the README
is the front door for someone **installing** a Hive. None of this is wrong; it is
simply not what a person arriving to install Swarm needs first.

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
the browser adapter boundary. See [README.md](README.md) in this directory for
the accepted decisions and continuing review sequence.

## Relationship to legacy Swarm

The legacy `miopea/swarm` repository remains an executable source of product
evidence. Swarm does not port a module merely because it exists. Each capability
is classified as keep, redesign, merge, or remove before implementation.

## Checks

These are the same checks CI runs. Read the exit code; never grep the output.

```sh
cargo fmt --all --check
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_INCREMENTAL=0 cargo test --workspace
pnpm install --frozen-lockfile
pnpm check
pnpm test
pnpm build
```

`CARGO_INCREMENTAL=0` is required in this workspace: six rustc ICEs came from
the incremental cache, each masking real errors.

## Running it from a checkout

The terminal host and the API are separate process owners shipped as one
application. Run them in separate shells with a shared socket path:

```sh
export SWARM_TERMINAL_SOCKET="$HOME/.local/state/swarm/run/terminal.sock"
export SWARM_TERMINAL_HISTORY_DIR="$HOME/.local/state/swarm/terminal-history"
export SWARM_WORKSPACE_ROOTS="/absolute/path/to/workspaces"
cargo run -p swarm-terminal-host

# In the API shell, set the same socket and a development-only secret.
export SWARM_OPERATOR_TOKEN="replace-with-a-long-random-development-token"
cargo run -p swarm-api
```

Run the browser client with `pnpm --dir web dev` on the fixed development URL
`http://127.0.0.1:8766`. It proxies `/health` and HTTP/WebSocket `/api` traffic
to `127.0.0.1:8765`. Unlock the development UI with the same operator token
configured for the API. A successful unlock creates a 30-day, same-origin,
HttpOnly browser session so refreshes and PWA restarts stay unlocked. The raw
operator token is never saved in Web Storage. Use Lock to revoke the
trusted-browser session; rotating `SWARM_OPERATOR_TOKEN` also invalidates it.
Do not commit or log operator tokens.

## Driving the terminal host

The typed lifecycle client never kills workers or deletes sockets:

```sh
cargo run -p swarm-cli --bin swarmctl -- status
cargo run -p swarm-cli --bin swarmctl -- drain
cargo run -p swarm-cli --bin swarmctl -- wait-ready 300
cargo run -p swarm-cli --bin swarmctl -- cancel-drain
```

## Installing your own build

From a clone, without cutting a release:

```sh
./packaging/linux/build-development-release.sh /tmp/swarm-build
sh /tmp/swarm-build/swarm-*/swarm-package install /tmp/swarm-build/swarm-*
```

To produce a distributable tarball instead, tag the commit first. A release
version comes from a tag so two releases can be compared, and `build-release.sh`
refuses an untagged commit by design, refuses a tag whose version disagrees with
`Cargo.toml`, and refuses to start without enough free disk to finish:

```sh
VERSION=1.8.1                                          # whatever you are cutting
git tag -a "v$VERSION" -m "Swarm $VERSION"
./packaging/linux/build-release.sh                     # prints the tarball path — read it, never guess
tar -xzf "dist/swarm-$VERSION-linux-x86_64.tar.gz"
"./swarm-$VERSION-linux-x86_64/swarm-package" install "./swarm-$VERSION-linux-x86_64"
```

Cutting a release that reaches other people is more than this — notes, CI on the
exact SHA, artifact verification, a signed manifest — and lives in the `deploy`
skill under `.claude/skills/deploy`.

## Where things go on disk

Releases are staged under `~/.local/lib/swarm`, configuration is written once
under `~/.config/swarm`, and durable terminal history and the database live
under `~/.local/state/swarm`. Content-hashed browser assets are retained under
`~/.local/lib/swarm/assets` so a tab opened before an update can still load a
deferred terminal module afterward. The writable workspace defaults to
`~/swarm-workspaces`; set `SWARM_WORKSPACE_ROOT` during the first install to
choose a different absolute path.

Uninstall preserves configuration and state.

Each immutable release ID includes its Git commit, and the builder refuses a
dirty worktree, so an installed artifact always maps back to one reviewable
source state.

## Updates, from the packaging side

`swarm-package update RELEASE_DIR` switches the API and browser release and
restarts only `swarm-api.service`; the independently versioned terminal-host
process and its worker PTYs stay alive. `swarm-package reconcile-host` moves the
sidecar to the current release, and a systemd timer runs
`reconcile-host-if-idle` every two minutes, which does the same thing once no
session reports being mid-turn.

When a release changes the terminal protocol, `update` refuses on purpose — a
host and an API speaking different protocols is the failure that guard exists to
prevent. Use `swarm-package migrate-protocol RELEASE_DIR`. It drains the old
host, defers while any worker session is live, switches the API and sidecar
together, and restores both previous pointers if health verification fails.

## The user service and logout

The user service runs while the user manager is active. A remote host that must
keep running after logout may require an administrator to enable user lingering;
the installer does not elevate privileges or change that host policy.
