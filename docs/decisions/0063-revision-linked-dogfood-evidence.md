# ADR 0063: Revision-linked, bounded Dogfood evidence

Status: Accepted implementation design within the approved maturity program.

Developer Dogfood needs cross-session evidence; the local one-hour recorder is
not a durable history. Start with browser timing aggregates, not terminal content
or a second event log. Ordinary server aggregates remain a separate 30-day scope.

One capture represents one browser collector's UTC hour and immutable running
build. Its random capture UUID is a retry identity, never a worker/session/device
identifier. A strictly increasing revision replaces that capture's cumulative
counts, totals, and maxima. Replaying the same or older revision does not add
samples. A capture ID cannot change build or hour. Metrics are validated
cumulatively: counts, totals, and maxima cannot decrease, and added
duration must be possible for the number of appended samples. A same-revision
retry with different contents is a conflict, not an overwrite.
Fixed allowlisted metrics are
long tasks, interaction, navigation, terminal paint, and terminal reconnect.
The September 5 phase extension adds terminal grant acquisition (request through
validated response), socket opening (construction through open), and initial
state (open through applied canonical state). These use the same bounded numeric
aggregates and owner. Completed phase attempts need not have a completed overall
connection, and initial state overlaps terminal apply latency; means must not be
summed or presented as matched end-to-end traces. Visibility loss or view
suspension discards in-flight phase and total-connection timings. No session IDs,
grant values, transport paths, or contents enter these measurements.

The evidence reader defaults absent phase aggregates to zero **samples** for
retained pre-extension records and older clients, not a measured zero duration.
It owns this additive compatibility for the 90-day retention / supported-client
window; remove defaults only after both have aged out. Frontend pending-capture
restore likewise accepts missing phases for its 24-hour queue window. No new
database table, timer, retention budget, or external export is introduced.
No arbitrary labels, prompts, paths, input, errors, screenshots, or timestamps for
individual actions are accepted. Zero samples differ from a zero duration.

The persistence boundary owns retention: at most 4,096 hourly captures, each
serialized to at most 4,096 UTF-8 bytes (16 MiB payload ceiling), for at most 90
days. Index/SQLite overhead is additional and not misrepresented as payload size.
Oldest hours are evicted first under capacity pressure. Writes and reads prune;
no cleanup timer or background queue is introduced. Reads are capped at 100
captures. Incoming hours must be aligned, no older than 24 hours, and no later
than the current server hour. Clock mismatch/offline loss must remain visible
when collection is wired; missing evidence never establishes health.

Schema 126 adds an isolated evidence table. It does not mutate worker, task,
conversation, or operator-policy data. Rollback to schema-125 code needs the
normal database compatibility/restore procedure, not a binary-only rollback.

Subsequent application/API wiring must authenticate access, enforce existing
development-mode detection on automatic collection, bound request bodies and
in-flight uploads, and report eviction/collection gaps. It must not automatically
publish data to GitHub or a third party. The browser will retain only bounded
unsent hourly aggregates and will not replay uncertain terminal input.

This foundation is not a completed telemetry feature. Comparison UI must show
sample counts, coverage, build identity, and collection limits; means/maxima are
not p95. Capture measurements and instrumentation overhead must be validated
before using them as release gates. Server/orchestration/recovery metrics remain
in the approved scope and must not be replaced by these browser-only summaries.

## Event entries versus interactions

The historical `interaction` wire field contains individual native Event Timing
entries, not unique actions. Preserve that meaning for retained and incoming
hourly records; label it Event-entry latency and explain the reporting threshold.
Never silently overwrite it with grouped data or claim it is INP.

Local diagnostics additionally owns at most 200 positive interaction IDs seen in
the past minute, grouping repeated entries by ID and retaining the slowest entry
with its input-delay, processing, and estimated-presentation phases. Expiry runs
on observation/read, without timers. Eviction is least recently observed, and
capture installation resets the map. IDs, targets and event names are never
exported or persisted. Only aggregate count and the slowest entry's numeric phases
enter the local report. This is thresholded recent evidence, not all interactions,
whole-page INP or a long-term percentile. Quantized duration may make the phase
remainder slightly negative, so presentation is clamped to zero and labeled an
estimate. Malformed/unidentified entries do not become guessed interactions.
Reference: https://www.w3.org/TR/2026/WD-event-timing-20260223/
