# Dogfooding and cutover

Status: **Proposed**

## Isolation

Legacy Swarm and Swarm Next run side by side with:

- different web ports;
- different runtime directories;
- different SQLite databases;
- different service names;
- different terminal sockets;
- separate worker processes and test workspaces.

They must never concurrently own the same PTY, worker process, database, or
mutable configuration directory.

The remote Linux host is the first realistic side-by-side environment once the
walking skeleton meets local acceptance criteria.

## Test rings

### Ring 0: deterministic development

Disposable workspaces, synthetic commands, recorded terminal streams, fault
injection, and browser automation.

### Ring 1: non-critical real work

The primary operator uses Swarm Next for bounded tasks while legacy Swarm
remains the immediate fallback.

### Ring 2: parallel daily use

Swarm Next handles a meaningful portion of normal work. Missing capabilities
are recorded, not automatically copied from legacy.

### Ring 3: default with rollback

Swarm Next is the normal entry point; legacy Swarm remains installed and can be
started independently. Promotion requires completed soak and recovery tests.

### Ring 4: migration and retirement

Selected durable legacy data is imported from a consistent export. Legacy
Swarm becomes read-only/archival, then is retired after an agreed observation
period.

## Feedback loop

The application includes a dogfood feedback action from the walking skeleton.
It captures, subject to preview and redaction:

- expectation and observation;
- application and protocol versions;
- current route and selected worker-session ID;
- recent domain transitions and trace IDs;
- terminal sequence, dimensions, connection state, and buffer usage;
- subsystem health;
- relevant sanitized logs;
- optional screenshot.

Reports become product evidence linked to a capability and journey. A request
to reproduce a legacy feature must state the user outcome it restores.

## Migration policy

- Import from a snapshot or explicit export, never the live legacy database.
- Dry-run reports transformed, skipped, invalid, and unsupported records.
- Imported records retain source identifiers for audit without controlling new
  identities.
- No dual write between legacy and Next.
- Cutover rehearsals include rollback before real migration.

