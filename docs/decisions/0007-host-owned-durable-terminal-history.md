# ADR 0007: Host-owned bounded durable terminal history

Status: **Accepted**

## Context

The canonical terminal snapshot makes browser reload independent of recent raw
output volume, but it intentionally preserves only the active terminal state.
Operators still need bounded scrollback and trustworthy recovery evidence after
the API or terminal host restarts. Keeping that history in React, a browser
database, or the general application event tables would put terminal lifetime
back in the wrong process and recreate the unbounded-storage failure mode that
motivated Swarm Next.

The legacy application retained 1 MiB of raw output per terminal and later
capped browser replay at 256 KiB after an out-of-memory investigation. A
separate transport path required 8 MiB to carry a measured snapshot of roughly
1.3 MiB. Those measurements prove that terminal retention, replay, and
transport need independent bounds. They do not establish an appropriate
on-disk retention period.

Terminal output can also contain source code, credentials, provider prompts,
or other sensitive workspace data. Durability therefore expands the security
surface and must be opt-in at the host boundary rather than incidental logging.

## Decision

The independent Rust terminal host owns durable terminal history. It writes
versioned, checksummed records to per-session append-only segment files under a
same-user state directory. The general SQLite application database stores
session metadata, not terminal bytes.

Each record contains the immutable session identity (by directory), the host
sequence, a wall-clock timestamp, a bounded payload length, and a CRC. Every
segment begins with a canonical screen checkpoint; later output records share
the live host sequence space. This keeps a retained segment replayable after
older segments and their ANSI/resize context are evicted. Startup scans
validate records and truncate only an invalid or incomplete tail, preserving
the last trustworthy record. Corruption and dropped-history counters expose
health without exposing terminal content.

Retention has four independent bounds:

- maximum record bytes;
- maximum segment bytes;
- maximum bytes per session;
- maximum total bytes and maximum age across the store.

The foundation defaults are deliberately provisional: 2 MiB per record, 4 MiB
per segment, 64 MiB per session, 512 MiB total, and seven days. The host reports
these limits and actual retained/dropped bytes so representative trace and soak
tests can replace the defaults with evidence before cutover.

Directories are mode `0700` and files are mode `0600` on Unix. Symlinked state
directories are rejected. Terminal bytes are never copied into logs or
diagnostic responses.

Durable bytes do not imply a live PTY. After terminal-host failure, validated
history may be recovered as archived output, while the session remains exited
until descriptor handoff or a process-supervision boundary can prove the PTY is
still owned. Swarm must not present an archive as an interactive worker.

## Consequences

- Browser and React memory remain unrelated to retained history volume.
- A partial final write cannot poison all earlier session history.
- Disk use and retention time are deterministic even under sustained output.
- When every remaining segment is active and the total bound cannot be met,
  new history is dropped and counted rather than exceeding the bound or
  blocking the PTY reader.
- Cross-host live-session continuity remains a separate descriptor-handoff
  increment; this decision provides the durable substrate without faking it.
- Historical scrollback can later be streamed through a bounded API without
  changing the storage owner or the browser terminal adapter.

## Alternatives considered

- Store output in SQLite: mixes a high-volume byte stream with authoritative
  application state and increases database write amplification and exposure.
- Store output in IndexedDB or Cache Storage: ties recovery and resource use to
  browser/profile lifetime, the exact boundary Swarm Next is removing.
- Keep only raw in-memory replay: cannot survive host restart and has already
  demonstrated dangerous browser allocation behavior in the legacy system.
- Persist unframed terminal bytes: cannot distinguish a valid prefix from a
  torn or corrupt tail after a crash.

## Validation

- Sustained writes never exceed per-session or installation byte limits.
- Age pruning removes expired closed segments.
- A truncated final record recovers the preceding records and records the lost
  tail in diagnostics.
- A checksum failure truncates from the first untrustworthy record.
- Session directories and segment files have same-user permissions.
- Real PTY output is appended under the same sequence assigned by canonical
  state, and a storage failure does not terminate the worker.
- Diagnostics report counts and byte totals only, never terminal payloads.
