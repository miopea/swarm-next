# ADR 0089: Terminal registry observation isolation

Status: Accepted implementation under the approved performance and update-safety
scope. Isolated candidate validation passed; live engine acceptance remains open.

## Evidence and outcome

A live demo reconnect measured1105ms in engine session validation, compared with
30ms for capability negotiation. Source inspection found that fleet resource
listing holds the registry membership mutex while scanning Linux processes once
per worker. Session lookup for terminal control needs that same registry mutex.
This is a concrete contention path, not proof that every slow request uses it.

## Decision

Diagnostic session listings and activity census take a bounded vector of strong
session references under the registry lock, release that lock, then read per-session
facts. Membership is the set captured at observation start; running state and other
facts are observations made afterward. A concurrently removed session can appear
in that snapshot but cannot become a current worker binding or write permission.
References last only for the call and are bounded by configured registry capacity.

One resource listing captures Linux process relationships and RSS once and reuses
that content-free snapshot for each root. There is no cross-request cache, timer,
command-line collection or new background task. Scans exceeding65536 entries return
unavailable rather than truncated totals. Missing roots are unknown, not zero.
Non-Linux resource capability remains explicitly unavailable.

Session creation/registration, stop, drain and maintenance membership exclusion
retain their existing guards. Per-session input/control/resize guards are unchanged.
The activity census remains diagnostic and never becomes atomic restart authority
(ADR0085). Releasing a read lock is not approval to stop or replace an engine.

## Verification

Use synchronized observation barriers to prove a paused fleet read does not block
registry lookup and typed control status. Failed reads must release references and
locks without changing the live session or authority. Test shared-tree accounting,
missing roots, cycles, existing handoff/maintenance exclusions and process recovery.
Run the bounded Linux scan comparison as evidence, not a flaky timing threshold.
Verify actual worker preservation/conversation return when activating an engine
build, then compare fresh demo reconnects and sustained real-workload responsiveness.
Unit tests alone cannot close the terminal-jumping or fleet-performance gates.
