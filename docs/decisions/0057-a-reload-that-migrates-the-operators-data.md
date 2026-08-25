# ADR 0057: A reload that migrates the operator's data

## Status

Accepted, 2026-08-25. Answers a question Queen raised and the operator passed
back — "We just reloaded. Ask Swarm." — which is consistent with the ruling this
extends rather than a refusal to rule.

## The question

ADR 0055 lets the worker whose workspace is the development checkout reload this
Hive on its own judgment. The operator's own words, read from
`resolution_answers` on decision `01a0396a-9e92-7d50-9701-e14836fe623c`:

> "Swarm next is approved to reload the app since it is the one dong active
> develpoment in the app itself. It knows how far it can go and when it is safe
> better than any worker. This does NOT apply to other workers."

That was given about swapping a binary. On 2026-08-25 a reload carried a schema
migration for the first time — 90 to 91, adding `task_review_holds`. Queen
flagged, correctly, that the permission predated the case.

## Decision

**A migrating reload is NOT a separate ask.** Requiring one would reinstate
exactly the wait-for-a-human dependency the whole heartbeat line of work exists
to remove, and it would bite hardest at night, when nobody is there to grant it
and the schema is no more dangerous than it was at noon. The operator has twice
now declined to make this worker ask, and answering "ask again" would be
answering a question they did not have.

**It is a separate PRECAUTION, and the precaution is enforced rather than
remembered.** Before a reload whose build carries a schema change, Swarm takes a
full backup of the database at its current version and refuses the reload if the
backup fails.

## Why the asymmetry justifies a rule at all

A code reload is symmetric. If the new build is wrong, reload again with a fix;
the cost is a minute and a page refresh, which the operator has explicitly called
acceptable ("that's just development").

A migration is not symmetric, for three reasons that compound:

- It changes the operator's DATA, not the code reading it. A worse build is
  replaceable; worse data is not.
- Migrations here are forward-only. `RECENT_SCHEMA_STEPS` carries `undo_sql`
  for *modelling* a database one version short in tests, not for reversing a
  live one.
- The remedy is therefore a RESTORE, not a retry — and a restore is only
  available if somebody took a backup BEFORE, which is the one moment the
  person best placed to take it is busy doing something else.

## Why enforced rather than remembered

On the day this was written the precautions were taken voluntarily: a dry-run of
the DDL against a copy of the live v90 database, and a full backup before
requesting. Both were cheap — seconds — and both were entirely dependent on the
worker deciding to bother.

That is the shape this fleet keeps rediscovering and keeps deciding against.
`CLAUDE.md` already lists four rules that are hooks rather than reminders,
precisely because a rule that depends on remembering is a rule that fails on the
day someone is busy. A habit that protects the operator's data should not be one
of the things that has to be remembered.

The dry-run is not made mandatory. It is good practice and it caught nothing
this time; the backup is what converts an unrecoverable class of mistake into a
recoverable one, and it is the one worth spending a guard on.

## What this does not change

It does not narrow ADR 0055. The reload still needs no operator round trip, the
requester's own Active work is still the only refusal, and the operator being at
the Hive is still not one.

It does not make the worker judge whether a migration is *safe* — that judgement
is exactly what the operator said to leave to it. It makes the failure
recoverable regardless of whether the judgement was right.

## Where the behaviour lives

`back_up_before_migrating_reload` in `crates/swarm-api/src/reload_backup.rs`,
called from `start_development_reload` in `crates/swarm-api/src/maintenance.rs`
— the one function both the control room's button and the agent tool go
through, so the guard cannot be present on one route and missing from the other.
It runs before the build is requested, so a refusal leaves the running Hive
exactly as it was.

"Carries a migration" is decided by reading `CURRENT_SCHEMA_VERSION` from the
*checkout* and comparing it against the live `PRAGMA user_version`. Not against
this binary's own compiled-in constant: the running binary migrated the database
to its own version at startup, so that comparison would report "no migration"
for every reload, including the ones that carry one.

Two details that are easy to get wrong, both settled here so nobody re-derives
them:

- **The constant is an alias**, not a literal — `const CURRENT_SCHEMA_VERSION:
  i64 = REPLY_ALLOWS_APPROVED_EXEMPTION_SCHEMA_VERSION;` — so resolving it takes
  two reads. A parser handling only a literal would read every real release as
  carrying no migration, and skip the backup on exactly the reloads that need
  one. A test reads the real file, so a future rename fails there rather than
  silently disarming the guard.
- **A version that cannot be read counts as a migration.** The two failure
  directions are not symmetric: an unnecessary backup costs megabytes, a skipped
  one costs data that forward-only migrations cannot give back.

The copy is taken with SQLite's own backup API rather than a file copy, because
the database runs in WAL mode and copying the file alone would produce a backup
that opens cleanly and is quietly missing the most recent work. It is then
verified — opened without migrating it, checked for the version it claims and
for integrity — because writing bytes is not the same as having a backup.

The reasoning that produced it is worth keeping even after the guard lands: the
question was not "may I", it was "what makes it safe to say yes".
