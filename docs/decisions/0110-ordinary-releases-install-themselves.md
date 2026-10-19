# 0110 — Ordinary releases install themselves while the operator is away

- Status: Accepted
- Date: 2026-09-27
- Supersedes: ADR 0050 §3 ("installing is consented to every time") for ordinary releases
- Amended: 2026-10-19, §5 (which Hives the Keeper asks about, and what it offers)

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
  twice. (§5 narrows "a different set" to "a Hive not yet asked about".)
- Schema is judged only against a Keeper running a release. A development
  build's schema is not any release's schema and says nothing about members.
- A member whose release is behind gets the same grace window whether its schema
  is behind or not, so an auto-installing member has time to catch up before
  anyone is asked. A member on the expected release with an older schema is
  still raised at once: that is a migration that did not run.

### 5. Ask only about Hives that can be helped, and offer only what Swarm does

Amended 2026-10-19, on the operator's answers to two questions: "Only online
Hives" and "Tell its operator / Leave it".

What was measured. Simply Bee-rilliant was asked about twice for 1.17.4, and
the operator answered both times. Then Honeysuckle caught up, which made the
set of Hives behind a smaller one, and a third ask was due. Every 1.17.4 raise
named only Hives that had been silent for days. "Update them" was answered
eleven times, but the Keeper cannot install a release on a member, so nothing
could act on it. Queen turned the answer into a worker task, and the only
thing that task could do was read state.

- **A quiet Hive is not asked about.** A Hive unheard for ten minutes reads
  Quiet on the roster. The 2026-09-30 ruling warns about silence there and
  pushes nothing. The fleet panel still lists the Hive as behind. An open card
  is withdrawn once no Hive it names is still behind and in touch.
- **A Hive's grace runs from its return.** Grace starts when the release
  appeared, or when the Hive came back into touch after that, whichever is
  later. Its time in touch is read from the Keeper's day of contact slots
  (ADR 0114). The spell breaks only where the slots are more than ten minutes
  apart. A Hive that stays in touch keeps its spell, so checking in does not
  restart the clock. Honeysuckle came back at 14:05 and was current by 14:15,
  with nobody involved.
- **An answer holds for each Hive it named.** A Hive answered about for a
  release is not asked about again for that release. A card names only Hives
  nobody has been asked about, and at most sixteen. The rest are asked about
  once that card is answered.
- **The card offers what can happen.** It offers "Tell its operator" and
  "Leave it for now" (plural when it names several Hives). Telling the
  operator is marked as the operator's own action, as it is on a Hive-fault
  card. The card says that the Keeper cannot install on a member, and that its
  operator can, from the Updates card.
- **The card fits its bounds.** A summary over 400 bytes is refused, and the
  raise is then lost with only a log line to say so. The old wording, one line
  per Hive, went over at four Hives. The summary names Hives as fit ("and N
  more"), and the evidence carries each Hive in full.

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

A Hive that is never in touch for six hours in a row while behind is never
asked about. A machine that runs a few hours a day, with its operator at it the
whole time, can stay behind with nobody told. Its own Updates card still offers
the install, and the fleet panel still shows it behind (§5).
