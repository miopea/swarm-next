# M1 durable terminal history

Status: **Implemented durable-history checkpoint**

This increment gives the Rust terminal host a bounded, crash-recoverable record
of PTY output. It deliberately does not claim that a terminal-host restart
preserves an interactive PTY; descriptor handoff remains a separate milestone.

## Ownership and format

- The terminal host writes output after canonical state assigns its sequence.
- Each immutable session has a private directory containing append-only
  `.swh` segments.
- Every record carries a format version, kind, sequence, timestamp, payload
  length, and CRC32 checksum.
- Every segment starts with a canonical screen checkpoint so eviction never
  leaves a retained stream dependent on discarded ANSI or resize context.
- Startup validates every record and truncates an incomplete or corrupt tail
  to the last trustworthy boundary.
- General application SQLite tables and browser storage never receive terminal
  output.

The default root is
`$HOME/.local/state/swarm-next/terminal-history`. Override it with
`SWARM_TERMINAL_HISTORY_DIR`.

## Resource policy

The provisional foundation defaults are:

| Bound | Default | Environment override |
| --- | ---: | --- |
| Record | 2 MiB | `SWARM_TERMINAL_HISTORY_MAX_RECORD_BYTES` |
| Segment | 4 MiB | `SWARM_TERMINAL_HISTORY_MAX_SEGMENT_BYTES` |
| Session | 64 MiB | `SWARM_TERMINAL_HISTORY_MAX_SESSION_BYTES` |
| Installation | 512 MiB | `SWARM_TERMINAL_HISTORY_MAX_TOTAL_BYTES` |
| Age | 7 days | `SWARM_TERMINAL_HISTORY_MAX_AGE_SECONDS` |

Limits are validated as a hierarchy: a segment must hold the largest record,
a session must hold a segment, and the installation must hold a session.
Closed segments are pruned oldest-first. If active segments consume the entire
installation budget, new history is dropped and counted instead of exceeding
the bound or blocking terminal output.

These values are starting points, not product conclusions. Dogfooding and a
representative sustained-output trace must establish final defaults.

## Security and diagnostics

On Unix, history directories are mode `0700`, segment files are mode `0600`,
and paths owned by another user or represented by a symlink are rejected.

Authorized operators can inspect content-free counters at:

`GET /api/v1/terminal/history/diagnostics`

The response includes configured limits, retained bytes, session and segment
counts, dropped records/bytes, recovered truncated bytes, and corrupt-segment
count. It never returns terminal payloads, commands, workspace paths, or
operator credentials.

Terminal content is available only through separate authorized routes:

- `GET /api/v1/terminal/history/sessions`
- `GET /api/v1/terminal/history/sessions/{id}`
- `GET /api/v1/terminal/history/sessions/{id}?segment={n}&record={n}`

History pages are bounded to 2,048 records and 512 KiB of ordinary output. One
canonical checkpoint may exceed the page target, but remains independently
bounded by the approximately 2 MiB record limit. The returned cursor identifies
the next record. If retention evicts the cursor's segment, the host sets
`reset: true` and restarts the page at the oldest retained canonical
checkpoint. A caller can therefore replace its historical view instead of
joining bytes onto an invalid ANSI state.

## Recovery semantics

- Browser or API restart: the terminal host still owns the PTY and serves the
  live canonical snapshot, as before.
- Terminal-host process restart: validated history remains listable and
  pageable, but the PTY is not reported as live. Descriptor handoff builds on
  this foundation in a separate increment.
- Explicit worker stop: the final segment is synced and becomes eligible for
  normal age and byte eviction.

## Validation

- Real PTY output is stored at the same sequence exposed by canonical state.
- Ten thousand sustained writes remain inside the session and installation
  bounds.
- Multiple sessions trigger deterministic global eviction.
- Active segments drop new history rather than violating the total bound.
- Torn writes and checksum failures preserve all earlier valid records.
- Durable history remains listable and pageable after the store is reopened.
- Pagination never duplicates records; an evicted cursor resets to a retained
  canonical checkpoint.
- Replaying the oldest retained checkpoint and its later output converges with
  uninterrupted canonical terminal state after earlier segments are evicted.
- Age pruning and Unix permission tests pass.
- The full Rust workspace passes formatting, Clippy with warnings denied, and
  all tests.
