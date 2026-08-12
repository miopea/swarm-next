# ADR 0020: Owned, observation-first runtime resource evidence

Status: **Accepted**

## Context

Swarm must remain stable for multi-day work and must identify memory growth in
the process that owns it. Browser memory, the replaceable API, the terminal
host, and provider processes are different resource owners. Combining them
into one number would recreate the ambiguity that made the legacy browser
incident difficult to diagnose.

Automatic worker termination under pressure would be particularly dangerous:
one sample cannot distinguish a leak from productive provider work, and an API
restart cannot repair terminal-host or browser pressure.

## Decision

Each long-lived Rust process samples and reports only its own resident memory.
The terminal host includes an optional content-free sample in its existing
status response. The API samples itself and exposes one private, no-store
resource status containing:

- the API resident-memory sample;
- the terminal-host resident-memory sample when the connected sidecar supports
  it;
- explicit advisory and critical thresholds; and
- one classified state per process: normal, advisory, critical, or unavailable.

The initial thresholds are 256 MiB advisory and 512 MiB critical. They are
evidence thresholds, not process limits. The initial policy is
`observe_only`: it never kills, restarts, pauses, or drains a worker.

Sampling is request-driven. It creates no timer, history, queue, retry loop, or
background owner. Linux reads the kernel-owned `VmRSS` field. Unsupported
platforms and older compatible sidecars report unavailable rather than
inventing a value.

Provider memory remains separately attributable in the soak harness and is not
folded into terminal-host resident memory. Browser memory remains browser-owned
and cannot be inferred from the server.

## Consequences

- Diagnostics can identify API and terminal-host memory pressure separately.
- A normal API update can expose API memory immediately while honestly showing
  an older sidecar as unavailable until a zero-session reconciliation.
- Diagnostic reports gain byte counts and classifications, never process
  arguments, paths, terminal content, worker names, or provider output.
- Automated pressure actions require a later decision supported by soak
  evidence, hysteresis, recovery semantics, and an explicit safe target.

## Validation

- Linux parsing accepts only `VmRSS` in KiB and handles missing/malformed
  evidence as unavailable.
- The host status field is optional so an updated API can communicate with the
  previous compatible sidecar.
- The private resource endpoint rejects unauthenticated requests and remains
  useful if the terminal host is unavailable.
- Component tests cover readable normal/advisory/critical labels and sanitized
  report inclusion.
- Full Rust, strict lint, frontend, build, packaged-runtime, browser, and soak
  gates remain required before promotion.
