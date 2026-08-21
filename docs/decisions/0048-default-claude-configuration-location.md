# ADR 0048: Workers use the default Claude configuration location

## Status

Accepted for dogfooding. Reverses the `CLAUDE_CONFIG_DIR` redirect recorded in
[ADR 0009](0009-systemd-user-package-lifecycle.md).

## Context

ADR 0009 pointed `CLAUDE_CONFIG_DIR` at
`~/.local/state/swarm/providers/claude` so a worker's credentials,
settings, session history, and plugins stayed inside application state. The
service units already run with `ProtectHome=read-only`, so the redirect was also
the only reason Claude could write anything at all.

Dogfooding showed the redirect costs more than it protects.

Claude resolves user-level custom slash commands, skills, agents, and plugins
from `CLAUDE_CONFIG_DIR`. The redirected directory contains none of them, so
every worker started with zero of the operator's authored shortcuts. On this
machine that silently removed sixteen commands that remained present and
untouched at `~/.claude/commands`, including the shipping procedures the
operator's own instructions require workers to follow.

The organization installs shared Claude configuration with an installer that
writes to the documented default location. A product that redirects the
variable cannot consume that configuration, and cannot assume where a different
operator keeps their files. For anyone who is not maintaining Swarm itself this
is a blocker rather than an inconvenience.

The redirect also breaks conversation resume. `--resume` reads transcripts from
the configured directory, so a conversation created anywhere else is invisible.
Swarm compensated by copying transcripts from `~/.claude/projects` into the
redirected directory before resuming. That staging step is pure accidental
complexity created by the redirect, and any conversation it fails to stage
cannot be resumed at all.

Isolation was never complete in any case: the same units already point every
worker at the operator's `~/.claude/settings.json`, so machine-wide permissions,
hooks, and auto-mode policy were deliberately shared from the start.

## Decision

Swarm does not set `CLAUDE_CONFIG_DIR`. Workers use Claude's default
configuration location, exactly as the operator's own terminal does.

The service units grant write access to `~/.claude` and `~/.claude.json`
instead, declaring exactly the two provider-owned paths Claude needs rather
than opening home.

Those grants are correct by declaration but are not currently load-bearing.
Measured on the dogfood host on 2026-08-18: `ProtectHome=read-only` and
`ProtectSystem=strict` do not take effect for these units. Running the same
directives under `systemd-run --user` leaves the whole of home writable with or
without the grants, because Ubuntu sets
`kernel.apparmor_restrict_unprivileged_userns=1` and an unprivileged user
manager cannot create the mount namespace the directives require. systemd 255
applies no protection and fails no unit. The redirect was therefore never the
only thing making Claude's state writable, and removing it cannot break a
worker's ability to write.

This also means ADR 0009's claim that "the remainder of home is read-only" did
not hold in practice on this host. That gap predates this decision and is not
resolved by it: the units keep declaring the intent so the protection applies
wherever the namespace is available, and restoring real enforcement is separate
work.

Swarm still passes `--settings ~/.claude/settings.json` explicitly, because a
service-started worker does not inherit login-shell state.

The transcript staging that copied conversations into the redirected directory
remains in place and becomes inert: its destination now equals its source, and
it already returns early when the destination file exists. It continues to
serve its second purpose of reconciling the older workspace-encoding scheme.

## Consequences

- Operators get their real Claude environment in every worker: commands,
  skills, agents, plugins, hooks, and shared organizational configuration.
- A shared configuration installer targeting the default location works for
  Swarm workers with no Swarm-specific step.
- Conversation resume no longer depends on a staging copy having succeeded.
- Authenticating a separate provider profile is no longer required before
  dogfooding, removing a setup step and a class of credential confusion.
- Swarm and legacy Swarm now share one mutable Claude configuration
  directory. The isolation requirement in
  [dogfooding and cutover](../07-dogfooding-and-cutover.md) still holds for the
  database, PTYs, worker processes, service names, ports, and runtime
  directories, which remain separate. Claude's own configuration is explicitly
  outside that boundary: it belongs to the operator, not to either product.
  Sessions are keyed by identifier and working directory, so concurrent use is
  expected rather than exceptional.
- Worker session history is written to `~/.claude/projects` and is no longer
  removed by clearing Swarm application state. Backup and restore of Swarm state
  therefore no longer captures or replaces provider transcripts.
- Changing either unit requires a worker-engine restart to take effect, which is
  a warned maintenance action rather than a normal App/API release.
