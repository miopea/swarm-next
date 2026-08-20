# ADR 0008: Drain-compatible terminal-host updates

Status: **Accepted**

## Context

Swarm must update its backend without killing active workers. The API and
browser can already be replaced independently because the terminal host owns
PTY descriptors. Replacing the terminal host itself is different: transferring
a PTY master descriptor does not transfer normal child-process parentage,
waiting, or reaping. The current safe `portable-pty` abstraction also does not
offer a supported way to reconstruct its master/child objects from received
Unix file descriptors.

Building descriptor handoff now would therefore require new Unix-specific PTY
and process-supervision machinery at the most failure-sensitive boundary. The
accepted architecture already permits a declared fallback that preserves the
previous compatible host.

## Decision

M1 uses a drain-compatible update fallback:

1. API, browser, and application updates continue using the existing terminal
   host when its IPC protocol is compatible.
2. A terminal-host binary update atomically places the running host in drain
   mode.
3. Drain mode rejects new worker sessions but preserves all existing PTYs,
   input, output, browser attachment, and durable history.
4. The updater observes running-session count through same-user IPC. Exited
   sessions do not block replacement because their history is already durable.
5. When running-session count reaches zero, the service manager sends a
   graceful interrupt, waits for socket removal, and starts the new host.
6. The operator may cancel drain before replacement. A forced update that
   would stop active workers requires an explicit controlled-restart action; it
   is never the default update path.

Drain transition and session start share the registry lock, so no new PTY can
race into existence after the updater observes drain mode. Host status reports
the protocol version, binary version, drain state, running session count, and
retained in-process session count without terminal content.

Descriptor handoff remains an allowed future optimization. It requires a
separate ADR and must prove child supervision, signal routing, rollback,
duplicate-reader exclusion, and no missing/duplicate terminal bytes.

## Consequences

- Normal application updates preserve every worker immediately.
- A terminal-host change may remain pending until active workers finish, which
  is visible and cancelable rather than disruptive.
- The old compatible binary remains the authoritative PTY owner during drain;
  two hosts never concurrently own one terminal.
- Security fixes that cannot wait for drain require explicit operator choice
  and communicate worker impact before action.
- Packaging must retain the old host binary until drain completes and verify
  the replacement protocol before beginning.
- Graceful shutdown must remove the Unix socket so replacement does not require
  unsafe stale-socket deletion.

## Alternatives considered

- Transfer PTY descriptors with `SCM_RIGHTS`: incomplete without a proven child
  supervision/reaping model and unsupported by the current PTY abstraction.
- Kill and recreate workers automatically: violates the zero-worker-loss update
  target and can deliver input to an unintended replacement process.
- Never update the terminal host: avoids the immediate problem but prevents
  security and compatibility maintenance.
- Run old and new hosts against the same sessions: creates ambiguous ownership,
  duplicate readers, and split-brain input.

## Validation

- Once drain begins, concurrent and later starts deterministically fail with a
  drain-specific error.
- Existing sessions remain interactive throughout drain.
- Exited sessions do not block readiness and remain available in durable
  history.
- Canceling drain re-enables starts without affecting existing sessions.
- A graceful interrupt removes the socket and allows a replacement host to bind
  without manual cleanup.
- Status and protocol responses contain no terminal bytes, workspace paths, or
  credentials.
