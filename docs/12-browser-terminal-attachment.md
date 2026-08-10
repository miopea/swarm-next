# M1 browser terminal attachment

Status: **Implemented foundation**

This increment makes a host-owned PTY usable from the browser without giving
React ownership of the worker, terminal session, WebSocket, replay cursor, or
xterm renderer.

## Operator outcome

- Unlock the local runtime with the development operator token.
- View and explicitly refresh the bounded worker-session list.
- Start Claude in an allowed absolute workspace.
- Attach xterm to a live host-owned PTY and send input or committed resizes.
- Switch between two terminal views without reconnecting either session.
- Resume after an unexpected WebSocket disconnect without duplicating output.
- Stop a worker only through an explicit lifecycle action.

The development token remains in JavaScript memory and is cleared from the
input after unlock. It is not persisted in browser storage. A full browser
reload therefore requires unlocking again during this foundation phase.

## Attachment protocol

1. The browser uses the operator bearer token over HTTP to request a grant for
   one immutable worker-session ID.
2. The API returns a cryptographically random, one-time grant with a 30-second
   lifetime and `Cache-Control: no-store`.
3. The browser sends the grant in `Sec-WebSocket-Protocol`, never in a URL. The
   server selects and echoes only the stable `swarm-terminal.v1` protocol.
4. The first WebSocket message commits the browser's last applied sequence.
5. The API waits on terminal-host output notifications over local IPC. It does
   not poll from the browser or run a per-terminal timer loop.
6. Output frames carry a monotonic sequence and raw PTY bytes. The browser
   rejects gaps, ignores duplicates, and advances its cursor only after a frame
   is accepted.

The current bounded journal can return `snapshot_required`. The browser makes
that recovery requirement visible and does not guess or replay from an unsafe
cursor. Canonical snapshot generation is the next terminal-state increment.

## Ownership and bounds

- terminal host IPC connections: 64 maximum;
- active terminal WebSockets: 32 maximum;
- outstanding attach grants: 128 maximum;
- inbound WebSocket message or frame: 64 KiB maximum;
- outbound browser queue: 64 messages maximum;
- browser automatic reconnect attempts: five bounded delays;
- terminal bytes and frames: existing journal byte and frame limits.

A disconnected API wait immediately cancels its host-side notification wait
and releases the connection permit. A slow browser fills its bounded outbound
queue and backpressures the output task instead of accumulating messages.

## Browser lifecycle

An application-lifetime TypeScript workspace owns one controller per immutable
session. Each controller owns its WebSocket, replay cursor, xterm instance, and
status subscriptions. React effects attach or detach the controller's existing
DOM host. Only explicit session close disposes the controller.

The xterm renderer is loaded as an on-demand production chunk, so the locked
application shell does not pay its parse and initialization cost.

## Verification

- Real PTY WebSocket replay, input, response, and grant-reuse rejection.
- Event-driven output wake without a polling loop.
- Immediate host connection release when a waiting API peer disconnects.
- Invalid and unconfigured operator authentication fail closed.
- Browser duplicate suppression and sequence-gap rejection.
- Fresh grant and cursor resume after disconnect.
- Open-close loops cannot reset the reconnect budget indefinitely.
- React detach/reattach and two-session switching do not reconnect transports.
- Typecheck, browser unit tests, production build, Rust formatting, Clippy, and
  the complete Rust test suite.

### Live executable evidence

A local host, API, and production-equivalent Vite client were exercised with
two real Claude Code PTYs. Both sessions remained connected while the visible
worker was switched. After a full browser reload and memory-only token unlock,
the API returned the same two session IDs and the selected terminal rebuilt its
screen from sequenced host output, including Claude's untouched workspace-trust
prompt. Both test workers were then stopped explicitly and the temporary host,
API, and browser processes were verified absent.

## Remaining M1 work

- Canonical terminal parser and atomic snapshot-plus-delta attachment.
- Durable time-and-byte-bounded on-disk terminal history.
- Host upgrade descriptor handoff or explicit compatibility fallback.
- Packaged lifecycle and user-facing authentication beyond the development
  operator-token unlock.
- Longer browser-driven two-real-worker soak after canonical snapshots are
  available.
