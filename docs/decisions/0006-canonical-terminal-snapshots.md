# ADR 0006: Host-owned canonical terminal snapshots

Status: **Accepted**

## Context

Raw PTY replay is sufficient only while every byte after a browser cursor is
still retained. It cannot reconstruct a terminal after journal eviction, makes
reload cost proportional to recent output, and leaves parser memory outside an
explicit product bound. Browser-owned serialization would make React/xterm
lifetime authoritative again and would not help a newly attached client.

## Decision

The independent Rust terminal host parses PTY output into canonical terminal
state under the same lock that assigns output sequences. A client with no
canonical state receives one snapshot at an exact sequence. A covered client
receives only later deltas; an expired cursor is replaced atomically by a new
snapshot.

Use `vt100` 0.16.2 behind the `swarm-terminal` state boundary. Snapshots are
formatted terminal bytes plus committed dimensions and an oversize-reset flag,
so the browser adapter remains replaceable. Explicitly preserve alternate-screen
mode in addition to the parser's formatted screen and input modes.

Absence of a cursor is distinct from sequence zero. IPC protocol version 2 and
WebSocket subprotocol `swarm-terminal.v2` carry that distinction and the new
snapshot frame.

Canonical state is bounded by:

- 1,000 parser scrollback rows;
- 200 rows, 320 columns, and 32,000 visible cells;
- parser reconstruction after each 1 MiB of PTY input;
- a 2 MiB maximum reconstructed snapshot;
- a 3 MiB browser render backlog.

If reconstructed state exceeds its bound, the host resets only the rendered
view, inserts a visible memory-safety notice, and preserves the worker process
and sequence. This event is carried to the browser instead of failing silently.

## Consequences

- Reload and stale-cursor recovery cost is proportional to the bounded visible
  screen, not the volume of recent worker output.
- Snapshot acquisition and subsequent deltas cannot race past one another.
- Browser resume cursors advance only after xterm confirms that bytes rendered.
- Slow rendering reconnects from a snapshot instead of accumulating messages.
- The current snapshot preserves the active screen, cursor, drawing state,
  supported input modes, and alternate-screen mode. Restoring historical
  scrollback awaits the bounded durable-history increment.
- The parser is an implementation detail and can be replaced without changing
  domain or React ownership.

## Alternatives considered

- Replay the bounded raw journal: cannot recover an evicted cursor and makes
  reload behavior depend on output volume.
- Serialize xterm in the browser: couples correctness to a presentation
  adapter and cannot serve a fresh client independently.
- Build a terminal emulator from scratch: creates a large correctness and
  security surface without product differentiation.
- Allow parser and renderer queues to grow: directly violates the resource
  policy that motivated Swarm Next.

## Validation

- Snapshot plus retained deltas converges with uninterrupted parsing.
- Styled content, cursor state, resize, and alternate-screen applications
  reconstruct from a fresh parser.
- Journal eviction returns a snapshot rather than a terminal failure.
- Pathological cell content compacts within the snapshot bound; forced
  oversize fallback is visible.
- Browser tests prove snapshot-before-delta ordering, applied-cursor semantics,
  bounded renderer recovery, and visible truncation state.
- A live two-worker reload test must preserve both session identities and
  reconstruct the selected active screen.
