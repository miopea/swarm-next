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

## Boundary

This slice deliberately implements preview and copy, not an outbound submission
transport. GitHub, Jira, or another destination must be chosen explicitly
before Swarm sends a report off the machine. That later transport will submit
exactly the operator-reviewed payload rather than recollecting hidden context.

## Verification

Frontend tests exercise healthy subsystem evidence, terminal/history requests,
content-free recent transitions, and the privacy exclusions above. The full
frontend typecheck, test suite, and production build remain required before the
checkpoint is committed.