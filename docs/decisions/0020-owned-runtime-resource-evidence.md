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

### Separate memory from combined machine pressure (September 9, 2026)

The runtime machine verdict combines memory and compute evidence for automatic
start admission. It is not a memory verdict. Memory and memory-stall verdicts are
now separately classified in the domain using the established 85/95-percent use
and 2/10-percent memory-PSI thresholds. The resource response exposes both; older
responses lacking them remain unclassified in the UI. The memory footprint of
the API/worker tree is judged against memory pressure, never CPU-only contention.
The combined machine guard still defers automatic starts under compute pressure.
No worker is stopped, no new sampler is introduced, and CPU admission thresholds
are unchanged. The headline names resource pressure; each row carries its own
evidence rather than inheriting the worst machine color.

September10 presentation follow-through: capacity is stated separately without
a second aggregate pressure verdict. The existing Performance evidence assessment
owns the summary across fresh machine, CPU-wait and process measurements. This
avoids a machine aggregate all-clear competing with CPU/process pressure from the
same sample. Detailed checks retain their own evidence and classifications; no
admission policy, measurement threshold or polling owner changes.

This repairs live evidence from an eight-CPU Hive: CPU wait and load were high
while memory use was about 43 percent and memory PSI effectively zero, yet memory
rows and the worker footprint were marked pressured. Both UI and API regressions
failed before correction. Tests cover independent classifications, invalid and
missing evidence, CPU-only admission deferral, older responses and quiet recovery.

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
