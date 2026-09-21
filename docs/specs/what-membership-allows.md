# What being part of an Apiary allows

Status: Accepted direction from the operator interview of 2026-09-21. Product
shape only; no ADR yet and nothing implemented.

The documented Apiary is almost entirely enrollment mechanics and Keeper reach.
A member accepts policy, publishes readiness, polls a catalog and receives
Keeper's task feed — and gains, in the documentation, nothing. This records what
membership BUYS.

## The answer: cross-Hive work — depend, hand off, unblock

A member's task can wait on work happening in another Hive and be told when it
clears. Work that belongs to a repository this Hive does not own is routed to a
Hive that does, rather than filed as a local draft only this Hive can see.

⚠️ THAT LAST FAILURE IS NOT HYPOTHETICAL. On 2026-09-20 a worker filed
`bfg-watchfaces` work with `swarm_create_task`, correctly naming the foreign
workspace — and it landed as a draft on the FILING Hive's board. The Hive that
owns that repository never learned of it. Cross-repo filing today records the
right facts in the wrong place.

## Topology: hub through Keeper

Every federation connection originates Hive to Keeper, and members have no
inbound address by design. A hub is therefore the only shape available without
asking members to expose a port, and Keeper already owns the ordering,
idempotency and conflict machinery this needs. No mesh, now or later, until
some other contract changes.

## Each Hive reports what it can do

A Hive publishes its repositories and its workers, INCLUDING SLEEPING ONES, so
Keeper knows what the fleet is capable of rather than what it is currently
doing. Availability is a separate and later question; capability is durable and
is what routing needs.

A Hive may pick up any apiary task whose work it has a worker for.

**Repository identity is the remote URL, never the local path.** This is an
engineering consequence rather than a product choice, and it is flagged for
review: absolute workspace paths differ per machine, so they cannot identify the
same repository across Hives, and publishing them leaks each operator's
filesystem layout for no gain.

## Contention: reserve at Keeper, first to reserve wins

Two Hives reporting a worker for the same repository is the expected case, not
an edge one. Reservation reuses the machinery already serializing shared Jira
claims: bounded reservations, idempotent confirmation, explicit release, and
EXPIRY RECOVERY when a Hive sleeps or dies mid-work. The last of those is what
stops a reservation becoming a permanent hold, which is the same stuck-work
shape this whole area keeps producing.

## One shared board, fully synced

Every member syncs EVERY apiary task. Keeper owns the ticket; members keep it
synced. Three things follow:

- Any Hive can pick up work the moment it gains a matching worker, with no
  round trip to discover what exists.
- A cross-Hive prerequisite resolves locally, because the upstream task is
  already present.
- Nothing leaks. These tasks were authored to be shared, so ADR 0034's
  privacy bound — which exists to keep terminal observations, worker identities
  and operator quotes inside a Hive — applies to a Hive's PRIVATE work and not
  to these.

Apiary tasks and local tasks share ONE board, with apiary work clearly marked.
Queen routes across both, so a Hive with a free worker takes shared work without
the operator switching views, and the marking keeps the ownership and closure
difference honest.

⚠️ THE COST, NAMED: on a busy Apiary, shared work can bury local work in the
same list. If that happens the answer is ordering or filtering within the one
board, NOT a second board — work nobody is looking at is how every invisible
backlog in this system has started.

## Cross-Hive prerequisites point only at Keeper-canonical tasks

A task may depend on a shared apiary task and on nothing else. Never another
Hive's private work.

⚠️ CORRECTION, ESTABLISHED AFTER THE INTERVIEW: this is not a new design. The
table `apiary_task_prerequisites` already exists, keyed apiary task to apiary
task with a required reason, and currently holds zero rows. The decision above
therefore RATIFIES the shape already built rather than choosing between open
options, and the remaining work is the routing and unblock behaviour around it,
not the link itself.

This settles privacy, authority and readability at once: no private task is
exposed, Keeper already owns ordering for these, both sides can legitimately
read title and state, and the link is a real edge the board can compute with
rather than prose. The cost is real and accepted: to be depended on, work must
first be promoted to shared.

## An orphaned link unblocks rather than waits

When the upstream vanishes — its Hive leaves the Apiary, or the task is
abandoned — every task blocked on it STOPS being blocked and becomes a Queen
obligation carrying why.

⚠️ THIS IS THE WHOLE POINT AND IT IS DELIBERATE. A gate that can never open is
the failure this system produces most often and detects least well. Measured on
this Hive 2026-09-20: two tasks in states that had stopped matching reality were
the only two live recovery obligations, and they forced EVERY Queen run to
Incomplete for hours. Cross-Hive dependencies would otherwise add a new and
better-hidden way to manufacture exactly that. Queen decides whether the work
still makes sense; nothing resumes silently and nothing waits forever.

## Closure: the member closes, Keeper mirrors

The Hive that did the work closes the apiary task, and Keeper mirrors the
closure. This was chosen over Keeper-settles.

**It is not closure by assertion, and the spec depends on that.** A member
closing locally still passes its own evidence gates: commits reported, and a
deployment or an approved no-deployment claim recorded. So "the member closes
it" already means "closed with evidence". THE EVIDENCE MUST MIRROR UPSTREAM WITH
THE CLOSURE, or a dependent Hive resumes on a conclusion it cannot inspect.

⚠️ THE RESIDUAL RISK, NAMED RATHER THAN DESIGNED AWAY: one Hive's closure
unblocks dependents in other Hives with no second check. A no-deployment claim
that is wrong therefore propagates. Mirroring the evidence is what makes that
recoverable rather than invisible; it does not prevent it.

## Settled 2026-09-21, second interview round

**Availability needs no signal.** A sleeping Hive simply never reserves, so
unreserved work stays available to whoever can take it, and expiry recovery
returns anything a Hive reserved and then slept on. Liveness is the
fastest-rotting fact in the system: routing on it means routing to a Hive that
was awake a minute ago, and "awake" never meant "free" anyway.

**Full sync is achieved by PAGING**, using the ordered event feed and its
atomically applied cursor that already exists, is already idempotent on exact
retry, and already fails closed on gaps. No arbitrary cap, no new mechanism, and
no member left holding a truncated board it cannot pick work from.

**Departure releases reservations immediately.** A departing Hive's held work
returns to the board at once rather than waiting out an expiry window.
Departure is a known ordered event and ADR 0031 already makes it retry-safe, so
there is no reason to let work sit unavailable when we know for certain it is
never coming back. Expiry recovery remains for the cases we do not know about.

**The capability report withholds nothing** — every repo, every worker name.
Keeper already gets a full window into every Hive, so hiding a repository NAME
while its terminal is watchable would be a boundary that protects nothing. This
holds while the Apiary is one organisation; revisit if one ever spans more.

## Not settled here

⚠️ THE THREE ITEMS THAT WERE LISTED HERE — availability, sync scale, and whether
worker names are sensitive — WERE ALL SETTLED in the second interview round
above. They are removed rather than left standing, because a stale open-questions
list is worse than none: it invites someone to re-decide something already
decided, differently.

What actually remains:

- **Reservation bounds.** How long a reservation holds before expiry recovery
  reclaims it. The Jira-claim machinery already has an answer; reuse it rather
  than choose a second number.
**Settled 2026-09-21: unreserved shared work is READ ONLY.** Reserve first to
change anything — title, description, state, prerequisites, notes. Reservation
is the moment a Hive takes responsibility, so it is the natural line, and it
stops two Hives editing one shared ticket with no serialization. Reserving is
cheap: the reservation machinery with expiry recovery already exists. The cost
is real and accepted — noticing a typo means reserving to fix it.
