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
