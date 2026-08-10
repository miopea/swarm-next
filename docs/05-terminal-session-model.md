# Terminal-session model

Status: **Partially implemented**

The terminal is a persistent server resource. A browser terminal is a rendered
attachment to that resource.

## Server-owned state

For each worker session, the terminal runtime owns:

- PTY master and child process identity;
- committed rows and columns;
- terminal parser and canonical active screen;
- cursor, modes, title, and alternate-screen status;
- bounded scrollback and byte journal;
- monotonic output sequence;
- attached clients and acknowledgement position;
- bounded input and output queues;
- lifecycle, health, and resource counters.

## Attach protocol

A client attaches with:

- worker-session ID;
- short-lived attach grant;
- viewport dimensions known to be stable and non-zero;
- last applied server sequence, when resuming;
- client protocol version.

The server responds with either:

1. missing deltas when the journal still covers the client's cursor; or
2. one canonical snapshot followed by deltas after its sequence.

The active-screen form of this contract is implemented by
[ADR 0006](decisions/0006-canonical-terminal-snapshots.md). Bounded durable
scrollback and post-host-restart archival replay are implemented by
[ADR 0007](decisions/0007-host-owned-durable-terminal-history.md).

Snapshot acquisition and live subscription form one atomic operation. Every
frame identifies the worker session and sequence. Frames from a stale session
are rejected even if the durable worker name has been reused.

Attachment begins only after the browser terminal is mounted and opened, fonts
are ready, and xterm's fit adapter can propose real dimensions during a bounded
post-layout fit window. The initial `swarm-terminal.v3` resume message carries
those stable dimensions. The API commits them to the host before acquiring the
first snapshot or delta response, making renderer readiness and initial
geometry part of the same synchronization boundary.

## Rendering rules

- Switching visible workers never reconnects or resets a session.
- React component remounts do not own WebSocket or xterm lifetime.
- WebSocket startup never precedes mounting, opening, fitting, and measuring
  the renderer.
- Hidden terminals never commit zero or intermediate dimensions.
- A resize is sent only after a stable non-zero ResizeObserver measurement.
- A changed resize advances the canonical sequence, invalidates byte-only
  cursors, and wakes every attachment with one canonical snapshot.
- An identical resize is acknowledged as a no-op so renderer echoes cannot
  create a synchronization loop.
- Reload produces one synchronization transition, not reset plus repeated
  replay guesses.
- Background terminals may suspend rendering, but their session cursors and
  bounded catch-up policy remain explicit.

## Recovery rules

- API restart: terminal runtime and PTYs survive; clients resynchronize.
- Browser restart: attach from no cursor and receive a canonical snapshot.
- Short disconnect: resume using missing deltas when available.
- Slow client: drop queued deltas at the bound and require a fresh snapshot.
- Worker exit: close the session with final status; never silently attach the
  client to a replacement process.
- Terminal runtime update: compatibility check and drain the old compatible
  host; preserve it while workers remain, or perform an explicit controlled
  restart. Descriptor handoff remains a future optimization.

## Resource policy

Every bound is configuration with a safe product default and observable use:

- scrollback bytes/lines per session;
- delta journal bytes per session;
- client outbound queue bytes;
- pending input bytes;
- attachments per session;
- terminal sessions per installation;
- inactive retention period;
- total terminal-memory budget.

Crossing a bound has a deterministic response: compact, require snapshot,
disconnect, reject, or archive. No bound silently expands.

## Proof obligations

- No missing or duplicate output across attach.
- No input reaches a stale worker session.
- Snapshot plus deltas converges to the same state as uninterrupted playback.
- Repeated attach/detach has bounded memory.
- Alternate-screen applications recover correctly.
- Resize during output preserves canonical state.
- Drain transition is atomic with session creation and preserves existing live
  processes and terminal streams.
