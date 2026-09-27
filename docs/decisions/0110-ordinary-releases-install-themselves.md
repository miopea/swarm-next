# 0110 — Ordinary releases install themselves while the operator is away

- Status: Accepted
- Date: 2026-09-27
- Supersedes: ADR 0050 §3 ("installing is consented to every time") for ordinary releases

## Context

Every Hive checks for a release every four hours, but nothing installs one until
somebody presses Install. In an Apiary that means every member lags until its
operator happens to open the Updates card, and the Keeper raises "Hives are
behind the current release" in Needs you. The operator asked, on 2026-09-27:
"shouldn't most hives update automatically?" — and then "yes, build
auto-install for ordinary releases", adding "the needs you keeps asking me about
updating hives".

The asking was measured, not guessed: eleven "Hives are behind" decisions for
one member between 2026-09-23 and 2026-09-27. A raise was suppressed only while
an earlier one was still pending, so answering it — even "Leave them behind for
now" — let the next check ask again. A Keeper running a development build also
judged members' database schema against its own unreleased schema, with no
grace window, so a member on the newest published release was raised as
`schema_behind` minutes after the Keeper was rebuilt.

## Decision

### 1. An ordinary release installs itself, once, while the operator is away

A release is ordinary when all of these hold:

- this Hive is running a release, not a development build (ADR 0050 §4 stands);
- the offer is newer than what is running;
- installing it does NOT change the terminal-host protocol — known, not assumed:
  a Hive that cannot tell whether the protocol changes does not install;
- no install is already requested or running.

It installs only while the operator is not at the Hive — effective presence is
away or Night Watch. Installing restarts the API: workers keep running, but open
pages reconnect, and that should not happen under someone's hands.

A schema migration does not make a release extraordinary. The installer backs
the database up before any release can migrate it, and restores it on failure.

A protocol change stays manual, because it stops every worker session.

### 2. One automatic attempt per release

If the automatic install of a release fails or is refused, the Hive does not try
that release again by itself. The failure stays on the Updates card, in the
installer's own words, and the operator retries from there. A newer release is a
new attempt. Without this, a release that cannot install would be retried on
every pass for as long as the operator stayed away.

### 3. On by default, and one switch turns it off

The setting is per Hive and defaults to on, which is what "most Hives update
automatically" means. Existing Hives take the default when they upgrade to the
release that carries this. Checking stays a separate setting (ADR 0050 §3): a
Hive that does not check has nothing to install.

### 4. The Keeper stops asking about what the operator already answered

- An answer to "Hives are behind" — any answer — holds until something new
  happens: a newer release, or a different set of Hives behind. The raise
  records what it was raised for, and a raise for the same thing is not asked
  twice.
- Schema is judged only against a Keeper running a release. A development
  build's schema is not any release's schema and says nothing about members.
- A member whose release is behind gets the same grace window whether its schema
  is behind or not, so an auto-installing member has time to catch up before
  anyone is asked. A member on the expected release with an older schema is
  still raised at once: that is a migration that did not run.

## Consequences

Members catch up without their operators, and Needs you asks about a lagging
Hive once per release rather than every check. The operator loses the moment of
consent for ordinary releases; the Updates card says installs are automatic and
offers the switch.

Only Hives running a build that carries this decision can install themselves, so
each Hive needs one manual install to join.

A Hive whose operator never leaves it installs nothing by itself. That is
deliberate: presence is the only evidence that restarting the API will not land
under somebody's hands.
