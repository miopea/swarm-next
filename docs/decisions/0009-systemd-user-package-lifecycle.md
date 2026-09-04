# ADR 0009: Unprivileged systemd user package lifecycle

Status: **Accepted**

## Context

The primary dogfood deployment must run beside legacy Swarm on an Ubuntu or
Debian host. It must feel like one application, preserve active workers during
updates, avoid root access, and make a failed release recoverable without
editing service files or deleting state by hand.

## Decision

Swarm ships one release artifact containing the Rust API, terminal host,
`swarmctl`, and compiled browser assets. Installation uses:

- versioned immutable releases under `~/.local/lib/swarm/releases`;
- an atomically replaced `current` API/browser symlink and one retained
  `previous` link;
- an independent `host-current` symlink for the terminal sidecar;
- systemd user units grouped by `swarm.target`;
- configuration under `~/.config/swarm`;
- durable data under `~/.local/state/swarm`;
- a same-user Unix socket under the systemd user runtime directory;
- the side-by-side HTTP endpoint `127.0.0.1:8766`.

Release identity combines the Cargo version and Git commit. Packaging refuses a
dirty worktree, preventing two different builds from claiming the same
immutable release directory.

The API serves the compiled browser application, so the operator starts and
updates one product even though terminal ownership remains in its independent
process.

Content-hashed browser files are also published into a stable asset library
under `~/.local/lib/swarm/assets`. Updates retain existing files and add
the previous and incoming release assets before switching the current link. An
open tab can therefore load a deferred module from its own release after an
update. The current release remains the fallback source, unknown asset names
remain 404s, and release directories stay immutable and checksum-verifiable.

The terminal host declares write access only to the configured workspace root,
application state, and the operator's Claude configuration, with the remainder
of home read-only. Measured on 2026-08-18, that confinement does not actually
take effect on an Ubuntu host that restricts unprivileged user namespaces; see
[ADR 0048](0048-default-claude-configuration-location.md). The declaration is
retained so it applies where the namespace is available. The service PATH
explicitly includes the user's local binary directory so a user-scoped Claude
installation is available without depending on login-shell initialization.

This record originally redirected Claude's credentials, settings, session
history, and plugins into an isolated provider directory using the documented
`CLAUDE_CONFIG_DIR`. [ADR 0048](0048-default-claude-configuration-location.md)
reverses that redirect; the write grant above replaces it.
The API has a read-only home and system view. Both use a private temporary
directory, `NoNewPrivileges`, a restrictive umask, bounded restart delay, and
journald.

Only the terminal host owns the shared systemd runtime directory. The API uses
the socket there but does not declare `RuntimeDirectory`; otherwise an API-only
restart can make systemd remove the live host socket from beneath its owner.

An update verifies its checksums and protocol before changing any active link.
When a Hive database already exists, the package first downloads a consistent
authenticated online backup from the running API, verifies it with the current
release, and retains it under the managed state directory. The approved daily-driver
policy keeps three managed pre-update backups: the active operation's rollback
copy plus the two newest others. The current copy is protected even after clock
rollback or future-dated older files. Manual snapshots are not pruned. A failed API or protocol activation restores that
exact database snapshot before reviving the previous release, so a successful
forward migration cannot strand an otherwise valid rollback on a newer schema.
For an exact protocol match it publishes retained browser assets, switches
`current`, and restarts only the API. The terminal-host process, socket, PTYs,
and `host-current` link remain untouched. A failed API health check restores
the previous API/browser link without restarting the host.

The explicit `reconcile-host` action begins drain before checking session
count, closing the race with new worker creation. It changes `host-current` and
restarts the sidecar only at zero sessions; otherwise it cancels drain and
defers. A failed host restart restores the old host link. Protocol changes are
rejected by ordinary update. The explicit `migrate-protocol` action likewise
drains first and refuses active sessions, then stops the product target,
switches `current` and `host-current` together, and verifies both host and API.
Any failure restores both independently pinned links and starts the previous
release; configuration and durable state never move.

Uninstall removes services, commands, and packaged releases only. Configuration
and durable state are preserved by default; data purge requires a separate,
explicit future operation.

## Consequences

- Normal browser/API replacement preserves terminal work by construction.
- A terminal-host replacement is a separate, explicit zero-session operation
  rather than a side effect of application deployment.
- Root privileges and a system-wide daemon are unnecessary for the initial
  single-operator deployment.
- The initial package uses one workspace root so its application allowlist and
  OS write boundary cannot drift apart.
- The isolated Claude profile requires one-time authentication and configuration
  but prevents Swarm workers from mutating the operator's host-wide profile.
- User lingering or an active login session is an operator prerequisite if the
  application must survive logout; enabling lingering is deliberately outside
  this unprivileged installer.
- Protocol changes require a brief, explicit zero-worker maintenance window;
  ordinary exact-protocol updates remain sidecar-preserving.

## Validation

- The API bind and browser root are configurable and static misses stay 404.
- The package lifecycle retains current and previous content-hashed assets, and
  the API serves both through the stable asset root.
- Both SIGINT and SIGTERM cause graceful socket-owning process shutdown.
- An isolated lifecycle smoke proves install, update, explicit rollback,
  automatic rollback after failed health, and uninstall with data retention.
- The lifecycle smoke proves compatible and protocol updates create verified
  pre-update backups and restore them after simulated post-migration failures.
- The lifecycle smoke performs update and rollback with a simulated active
  session and asserts that neither stops nor restarts the terminal host.
- Host reconciliation refuses a live session and succeeds after it exits.
- Protocol migration refuses a live session, switches both process pointers at
  zero sessions, and restores both after failed health verification.
- Release files are checksum-verified before installation.
