# ADR 0009: Unprivileged systemd user package lifecycle

Status: **Accepted**

## Context

The primary dogfood deployment must run beside legacy Swarm on an Ubuntu or
Debian host. It must feel like one application, preserve active workers during
updates, avoid root access, and make a failed release recoverable without
editing service files or deleting state by hand.

## Decision

Swarm Next ships one release artifact containing the Rust API, terminal host,
`swarmctl`, and compiled browser assets. Installation uses:

- versioned immutable releases under `~/.local/lib/swarm-next/releases`;
- an atomically replaced `current` symlink and one retained `previous` link;
- systemd user units grouped by `swarm-next.target`;
- configuration under `~/.config/swarm-next`;
- durable data under `~/.local/state/swarm-next`;
- a same-user Unix socket under the systemd user runtime directory;
- the side-by-side HTTP endpoint `127.0.0.1:8766`.

Release identity combines the Cargo version and Git commit. Packaging refuses a
dirty worktree, preventing two different builds from claiming the same
immutable release directory.

The API serves the compiled browser application, so the operator starts and
updates one product even though terminal ownership remains in its independent
process. The terminal host gets write access only to the configured workspace
root and application state; the remainder of home is read-only. Claude's
documented `CLAUDE_CONFIG_DIR` redirects its credentials, settings, session
history, and plugins into an isolated provider directory within that state.
The service PATH explicitly includes the user's local binary directory so a
user-scoped Claude installation is available without depending on login-shell
initialization.
The API has a read-only home and system view. Both use a private temporary
directory, `NoNewPrivileges`, a restrictive umask, bounded restart delay, and
journald.

Only the terminal host owns the shared systemd runtime directory. The API uses
the socket there but does not declare `RuntimeDirectory`; otherwise an API-only
restart can make systemd remove the live host socket from beneath its owner.

An update verifies its checksums and protocol before changing the active link.
It stages the new release, begins drain through the current version's
`swarmctl`, waits for zero live sessions with a bounded timeout, then stops the
product target and switches atomically. A failed health check restores the
previous link and starts the prior release. A drain timeout cancels drain and
leaves the running release unchanged. Protocol changes are rejected until an
explicit rolling-compatibility migration is designed.

Uninstall removes services, commands, and packaged releases only. Configuration
and durable state are preserved by default; data purge requires a separate,
explicit future operation.

## Consequences

- Normal browser/API replacement has a clear path to preserve terminal work.
- A terminal-host replacement waits visibly rather than terminating workers.
- Root privileges and a system-wide daemon are unnecessary for the initial
  single-operator deployment.
- The initial package uses one workspace root so its application allowlist and
  OS write boundary cannot drift apart.
- The isolated Claude profile requires one-time authentication and configuration
  but prevents Swarm Next workers from mutating the operator's host-wide profile.
- User lingering or an active login session is an operator prerequisite if the
  application must survive logout; enabling lingering is deliberately outside
  this unprivileged installer.
- Exact protocol matching limits rolling upgrades for now but fails closed.

## Validation

- The API bind and browser root are configurable and static misses stay 404.
- Both SIGINT and SIGTERM cause graceful socket-owning process shutdown.
- An isolated lifecycle smoke proves install, update, explicit rollback,
  automatic rollback after failed health, and uninstall with data retention.
- Release files are checksum-verified before installation.
