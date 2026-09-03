# ADR 0060: Browser-owned bounded performance evidence

Status: Accepted for the operator-approved maturity program, 2026-09-03.

The server cannot explain browser main-thread/render latency. Browser evidence
must not itself become a source of unbounded work or collect terminal contents.

One application-owned recorder aggregates allowlisted timing metrics into ten
second buckets, capped at 360 buckets and one hour. Automatic incident captures
retain at most five windows (two minutes before, one after), expiring after 24
hours. Capture is evidence, not an operator alert. No timer drives collection;
native performance observations and existing lifecycle events provide samples.

Only numeric durations, counts, metric identifiers, and timestamps enter the
recorder. No session IDs, arbitrary strings, paths, DOM targets, event names,
URLs, input, or terminal bytes are retained. Observers disconnect on application
teardown; unsupported performance entry types report unavailable. Page-hide saves
a bounded content-free snapshot in session storage for before/after reload
comparison. Storage failure is nonfatal. This is not long-term Dogfood storage.

Reports expose historical incidents separately from current health. A past slow
sample cannot keep Needs You active. Timing evidence is not a claim of Edge CPU
utilization, database integrity, or provider delivery acknowledgment.

Tests must prove count/age bounds, invalid input rejection, privacy projection,
incident coalescing, storage-failure recovery, and observer disposal before the
slice is accepted. Subsequent server correlation and Dogfood views extend this
owner rather than create competing browser recorders.
