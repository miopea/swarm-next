# ADR 0091: Bounded history of confirmed Queen run finishes

Status: Accepted under DOG-01; implemented and live-verified September 9, 2026.

The current `queen_automation` row owns the live run and is overwritten by the
next run. Browser history cannot answer how often Queen finishes without action,
how often coverage is incomplete, or how long delivery and finishing took.

Record each successful exact-run finish in the same persistence transaction as
the existing lifecycle change. Store the requested outcome separately from the
accepted outcome after existing coverage/decision checks. Retain only run ID,
trigger, requested/delivered/finished timestamps, attempt count, initial actionable
count, and the API build at finish. No terminal, prompt, task title, statement,
worker path, evidence body, or customer content belongs in this history.

The API build at finish is not proof that the entire run used that build. Recorded
`completed` means a Queen run finished with that accepted label, not that tasks
completed or that the Queen's judgment was correct. Do not present these outcomes
as a verified productivity percentage. Missing/clock-inconsistent timing is
unavailable, never zero. Unfinished, abandoned, and pre-feature runs are outside
this explicit-finish series; the UI must state that coverage limitation.

Retain at most 4,096 fixed-shape rows for 30 days. Prune on admission and reads;
reads return at most 100 newest rows and disclose the retained count so a partial
view cannot masquerade as full history. Exact run IDs are unique. A repeated or
stale finish cannot add another observation or change the original record.
History is private to the Hive and uses existing operator authentication.

This introduces no timer, terminal read, new Queen prompt, routing policy,
permission, public publishing, or worker restart. Domain types describe facts;
persistence owns SQL/retention; the HTTP adapter only authorizes and projects.
Schema migration uses the existing database-backup/update boundary. Downgrade
requires a compatible database backup. Missing/corrupt history is unavailable,
not a healthy zero-run report.

Verify normalized outcomes, duplicate/stale finish, transaction rollback,
retention/count bounds, migration/restart, unknown timings, private API access,
UI failure/recovery, and live normal Queen finishes before accepting this slice.
Broader server aggregates, verified task-review yield, recovery metrics, and the
full DOG-01 requirement remain separate work; this history does not close them.

Live acceptance: commit 202519ad, schema-153 backup before activation, all twelve
running session/conversation pairs preserved, public unauthenticated read refused.
An ordinary Queen finish retained requested completed versus accepted incomplete;
the authenticated API and rendered Developer Dogfood panel agree on the record
and 275-second delivery wait / 50-second delivered-to-finish interval. The next
queued run leaves it intact. Full persistence suite: 689 passed. See docs/47.
