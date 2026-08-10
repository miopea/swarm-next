# ADR 0005: Independent terminal-host process

Status: **Accepted**

## Context

The walking skeleton requires provider processes and PTYs to survive browser
reload and API restart. If the HTTP process owns PTY descriptors, ordinary API
replacement necessarily risks worker loss. Moving only terminal rendering into
the browser does not solve descriptor or process ownership.

## Decision

Ship a dedicated Rust terminal-host process as part of the single Swarm product
package. It owns PTY masters, provider child processes, bounded terminal
journals, and session lifecycle. The Rust API communicates through a versioned
local protocol over a same-user Unix socket.

The host accepts typed provider operations and validated workspace roots. It
does not accept arbitrary executable commands from HTTP or the browser. The API
owns operator authentication and external contracts but never inherits PTY
handles.

## Consequences

- API replacement cannot implicitly close worker PTYs.
- Terminal-host failure is isolated and diagnosable as its own subsystem.
- Local IPC framing, permissions, compatibility, and resource limits become
  explicit proof obligations.
- Packaging supervises two cooperating processes but exposes one installed
  application to the operator.
- Terminal-host updates require descriptor handoff or a declared fallback that
  preserves the previous compatible host.

## Rejected alternatives

- API-owned PTYs: cannot satisfy API restart survival reliably.
- Browser-owned sessions: loses work on reload and gives UI lifetime authority
  over processes.
- Permanent Node control plane plus Rust holder: adds another backend runtime
  without improving the ownership boundary.
