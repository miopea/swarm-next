# Privacy-safe diagnostics

Status: **Implemented foundation**

Settings now identifies browser, API, database, terminal-host, provider, and
integration health separately. A terminal-host failure degrades that subsystem
without hiding evidence from the healthy API or browser layers.

## Feedback preview

The operator can generate and inspect a structured report before copying it.
The report includes:

- a locally generated correlation ID and report schema version;
- browser connectivity, visibility, and live-update state;
- API and terminal-host versions and terminal-host drain state;
- the local Hive ID, worker/session counts, session IDs, and provider failure
  count;
- bounded terminal-history counters; and
- the last 16 content-free control-room transition sequence IDs, kinds, and
  timestamps observed by this browser.

The report excludes operator credentials, terminal output, task text, worker
names, workspace paths, and raw backend error messages. A browser clipboard
failure leaves the preview selectable for manual copy.

## Private Hive queue

The feedback dialog can now save the exact previewed bundle to the local Hive.
An optional pasted screenshot is stored through the same content-addressed,
signature-checked, mode-0600 attachment boundary used by terminal image paste;
the report records only that opaque filename. Reports are authenticated,
`no-store`, included in the Hive database backup, and capped at the newest 50.
The UI keeps notes and the image in place if either write fails.
After a successful save, the action stays disabled until the operator visibly
edits the notes or attachment, preventing accidental duplicate reports.

This makes dogfooding asynchronous: the operator can save evidence at the
moment of failure and a trusted developer can later read it through
`GET /api/v1/feedback/reports`. No hidden data is recollected after submission.
Settings also shows the newest reports as collapsed summaries and copies the
already-reviewed bundle on demand; diagnostic payloads are not expanded by
default.

A trusted operator can download a retained screenshot from its saved report.
The authenticated endpoint serves only opaque attachment names that are still
referenced by a retained report, uses `no-store` and `nosniff`, and forces a
download rather than rendering untrusted image content inline. Screenshot files
expire after seven days even when the report remains; Settings then explains
that the screenshot is no longer available instead of silently failing.

## Boundary

This is private local retention, not an outbound submission transport. GitHub,
Jira, or another destination must be chosen explicitly before Swarm sends a
report off the machine. Any later transport will submit exactly the
operator-reviewed retained payload rather than recollecting hidden context.

## Verification

Persistence, API, and frontend tests exercise bounds, authentication, image
signature validation, report saving, content-free recent transitions, and the
privacy exclusions above. The full frontend typecheck, test suite, strict Rust
lint, database migration, and production build remain required before the
checkpoint is committed.
