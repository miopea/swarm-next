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

Diagnostics compares the recorder's last 30 seconds with the latest server
resource sample. Samples older than 30 seconds, absent readings, and server
timestamps more than five seconds ahead are explicitly stale/unknown, not healthy.
Concurrent browser delays and server pressure are observations, never proof of
causality or a measurement of browser CPU. The same content-free assessment is
included in copied reports. It adds no collector, timer, or operator escalation;
old incident windows and pre-reload evidence cannot keep a current delay active.

Reports expose historical incidents separately from current health. A past slow
sample cannot keep Needs You active. Timing evidence is not a claim of Edge CPU
utilization, database integrity, or provider delivery acknowledgment.

Within one App window, the App owns the server resource sample used by both
runtime pressure status and Diagnostics. Its visible-page, single-flight owner
samples every thirty seconds normally and every ten while Diagnostics is mounted.
Manual diagnostic refresh joins that same owner. Diagnostics retains its own
host/history reads but does not duplicate the resource endpoint. Standalone
diagnostic views without an App owner retain a bounded local resource read.
This shares evidence within a window; it is not a cross-device cache or proof of
server health. Failed samples remain unavailable in both consumers.

Tests must prove count/age bounds, invalid input rejection, privacy projection,
incident coalescing, storage-failure recovery, and observer disposal before the
slice is accepted. Subsequent server correlation and Dogfood views extend this
owner rather than create competing browser recorders.

Terminal grant diagnostics also retain at most 200 successful request observations
for one hour, collected in the existing attachment completion path without an
observer or timer. A same-origin Resource Timing entry is paired only when exactly
one valid fetch entry fits the measured request interval. Missing, restricted,
stale or ambiguous entries remain unknown, never zero. Only numeric durations and
the observation timestamp survive collection; URLs, session IDs, headers and
response data are not retained. This local detail is not added to hourly exports.

The report distinguishes full client-observed fetch duration, resource duration,
request-to-first-byte, body transfer and time from response end to client completion.
Request-to-first-byte includes network/proxy/server time; response-to-client includes
browser scheduling, body consumption and JSON parsing. Neither is a server CPU
measurement or proof of a particular bottleneck. Cold reload measurements on the
development Hive motivated this attribution; deployment and live comparison are
still required before drawing a performance conclusion.

Successful authenticated terminal grant responses additionally carry one standard
Server-Timing metric, `swarm_grant`, measured with the API's monotonic clock from
handler entry through authorization, engine validation, grant issuance and response
encoding. It is numeric elapsed wall time, not CPU time. It excludes request time
before handler entry and later proxy/network delivery. Failed authorization does
not expose this success-only metric. No new log, observer, retry or background task
is created, and neither the grant nor session identity enters the timing header.

The browser accepts only one bounded numeric Swarm metric from at most 1024 header
characters and retains only its duration in the existing 200-sample/hour detail.
Missing, malformed, duplicate or implausible durations remain unknown. This can
be compared with the same request's resource timing without assuming a difference
is exclusively network latency: API scheduling before handler entry is excluded.
Old servers and proxies that omit the header continue to work normally.
