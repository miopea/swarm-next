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
[ADR 0006](decisions/0006-canonical-terminal-snapshots.md). Durable scrollback
restoration remains pending.

Snapshot acquisition and live subscription form one atomic operation. Every
frame identifies the worker session and sequence. Frames from a stale session
are rejected even if the durable worker name has been reused.

## Rendering rules

- Switching visible workers never reconnects or resets a session.
- React component remounts do not own WebSocket or xterm lifetime.
- Hidden terminals never commit zero or intermediate dimensions.
- A resize is sent only after a stable non-zero ResizeObserver measurement.
- The server acknowledges the committed dimensions.
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
- Terminal runtime update: compatibility check and descriptor handoff; on
  failure, preserve the old runtime or perform an explicit controlled restart.

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
- Descriptor handoff preserves live processes and terminal streams.
