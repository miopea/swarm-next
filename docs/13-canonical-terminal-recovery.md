# M1 canonical terminal recovery

Status: **Implemented and live-validated**

This increment replaces transcript-dependent browser recovery with host-owned
canonical screen reconstruction.

## Operator outcome

- A full browser reload rebuilds the current terminal screen from one bounded
  snapshot, even after the raw delta journal has evicted earlier output.
- Switching workers continues to preserve each independent renderer and
  transport.
- If rendering falls behind, Swarm discards the bounded browser backlog and
  requests a fresh screen instead of consuming memory indefinitely.
- If adversarial terminal state exceeds the host bound, the worker continues
  running and the terminal shows an explicit reset notice.

Reload and PWA restart now recover through the trusted-browser session described
in ADR 0022. Packaged lifecycle and multi-user identity remain later milestones.

## Synchronization contract

`after_sequence: null` means the client has no canonical state. A numeric
cursor, including zero, means a prior snapshot or delta has finished rendering.
This distinction prevents an empty sequence-zero snapshot from being emitted in
a loop while a quiet worker waits for output.

The `swarm-terminal.v3` resume handshake includes the last applied sequence and
the mounted renderer's stable, non-zero rows and columns. Before selecting a
replay response, the API synchronously commits those dimensions to the terminal
host. Its binary frames remain:

- delta: type byte `1`, big-endian `u64` sequence, raw PTY bytes;
- snapshot: type byte `2`, big-endian `u64` sequence, big-endian `u16` rows,
  big-endian `u16` columns, one oversize-reset flag byte, formatted terminal
  state bytes.

The host acquires parser state and the sequence atomically. Output that arrives
after snapshot acquisition receives a later sequence and is delivered as a
delta on the next event-driven wait.

## Memory and security policy

- Canonical parser: 1,000 scrollback rows.
- Visible geometry: at most 200 rows, 320 columns, and 32,000 cells.
- Parser compaction: every 1 MiB of processed PTY input.
- Canonical snapshot: at most 2 MiB after compaction.
- Browser render backlog: at most 3 MiB.

Compaction reconstructs a new parser from its bounded formatted state, dropping
hidden parser history that is not necessary to reproduce the active screen.
The oversize fallback is deterministic, visible, and affects only presentation;
it never kills or replaces the provider process.

## Verification

- Fresh, expired, and covered cursors select snapshot or deltas correctly.
- Snapshot plus deltas converges with uninterrupted parsing.
- Styled cells, cursor state, dimensions, and alternate-screen mode recover.
- A committed resize wakes every attachment at a sequenced snapshot boundary;
  no client applies later output using stale dimensions.
- Renderer mounting, post-font/post-layout fitting, initial resize
  acknowledgement, and first replay occur in that order, including after a
  full browser reload.
- Snapshot restoration is followed by one visible-container refit so replay
  cannot leave xterm at stale server dimensions.
- Resume cursor advances only after the renderer's completion callback.
- A renderer backlog crosses a hard bound and requests a fresh snapshot.
- Pathological parser content compacts, and oversize fallback is visible.
- Full Rust and web quality gates.

### Live executable evidence

A local version 2 host, API, and browser client ran two real Claude Code PTYs.
Both displayed their untouched workspace-trust screen and remained connected
while switching. After a full browser reload and memory-only token unlock, both
original session IDs were present, each selected terminal reconstructed the
same trust screen from its canonical snapshot, and the browser reported no
warnings or errors. Both workers were stopped explicitly, the browser test tab
was closed, and the temporary host, API, and client listeners were verified
absent.

## Remaining M1 work

- Durable time-and-byte-bounded terminal history and scrollback restoration.
- Host upgrade descriptor handoff or explicit compatibility fallback.
- Packaged lifecycle and user-facing authentication.
