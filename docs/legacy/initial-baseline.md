# Legacy Swarm initial baseline

Captured: 2026-08-10

This document records measured context, not replacement requirements.

## Scale

- Approximately 81,000 Python source lines across 297 modules.
- Approximately 14,000 JavaScript lines, dominated by one dashboard IIFE.
- Approximately 9,700 HTML lines.
- Approximately 97,000 test lines across 294 test files.
- Approximately 268 registered HTTP routes.
- 42 MCP tools across worker and Queen surfaces.
- SQLite migrations through version 19.
- More than 20 persistent background loops in the daemon.

## Relevant architecture

- A Python PTY holder owns PTY masters and child workers over a JSON-lines Unix
  socket protocol.
- The holder supports raw output buffering, client backpressure, source-version
  detection, and in-place descriptor handoff through `execv`.
- The Python daemon owns application orchestration, database access, routes,
  MCP, integrations, WebSockets, and many background loops.
- The browser caches xterm instances and independently manages reconnection,
  replay, fitting, resize, focus, scroll, input readiness, and stale detection.

## Observed terminal pressure

Legacy terminal recovery contains multiple compensating mechanisms:

- raw replay snapshots plus live output subscription;
- xterm reset on reconnect to avoid mixed stale frames;
- hard reconnect after certain layout transitions;
- repeated immediate, animation-frame, and delayed resize attempts;
- first-payload and stale-output watchdogs;
- cached terminals detached and reattached to changing containers.

These mechanisms demonstrate hard-earned operational knowledge, but also show
that terminal truth is distributed across holder, daemon, socket, xterm, and
browser layout lifecycles.

## Product evolution evidence

Legacy documentation emphasizes background approval drones. Current Claude and
Codex automatic-approval capabilities have reduced the importance of that
original outcome. This is the first confirmed example of a feature family that
must be reassessed rather than ported.

