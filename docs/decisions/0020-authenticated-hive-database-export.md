# ADR 0020: Authenticated Hive database export

Status: **Accepted and implemented**

## Context

Dogfooding needs a user-visible way to leave the installation with its durable
Hive data. Copying the live SQLite files is not a consistent backup, and a
browser export must not expose the operator credential or repository contents.

## Decision

The operator-only API creates an online SQLite backup through the persistence
owner and returns it as a no-store attachment. The Settings surface downloads
that consistent file with an explicit private-data warning. It includes the
canonical Hive database: identity, workers, provider conversation bindings,
tasks, decisions, policies, notification keys, and audit state. It excludes
repositories, terminal output/history, provider login material, deployment
secrets, and machine-specific workspace-root configuration.

Restore is deliberately not performed by the running web process. The package
lifecycle provides a verified, offline restore that checks integrity and
compatibility, creates a rollback snapshot, replaces state while the API is
stopped, restarts only the API, and rolls back on failed health. A later full
encrypted export may combine the database with explicitly selected portable
configuration; it must never silently copy machine credentials.

## Consequences

- The first useful backup is consistent and immediately available to the
  operator without interrupting workers.
- The downloaded database is sensitive and unencrypted, so the UI warns the
  operator and the response cannot be cached.
- Repositories and host-specific secrets cannot be mistaken for portable Hive
  state.
- Restore is an explicit package command and preserves the terminal host and
  repositories while restarting only the API.

## Validation

- The route requires operator authentication.
- SQLite's online backup produces a reopenable database that passes integrity
  checks.
- The response is a no-store attachment with a stable SQLite media type.
- Browser tests verify the Settings action downloads the returned snapshot.
- Package lifecycle tests prove restore verification, API-only restart, and
  terminal-host preservation.
