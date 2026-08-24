# ADR 0051: A worker reloading its own Hive

## Status

Accepted, 2026-08-24. Asked for by the operator on 2026-08-23: "add an ability
for you to restart swarm api/app on your own so you don't need me for fixes."
The presence rule below is an operator ruling from the same session.

## The cost being paid

Roughly twenty commits went into this Hive in one session and every one of them
sat unverified until the operator pressed **Build and release**. Two were
regressions nobody could see until the operator hit them — a notification
handler that stopped opening the app, and a geometry claim that made two devices
fight — and both cost a full round trip through a human whose only role was
pressing a button. A worker could not close its own loop, so "fixed" always
meant "fixed, pending someone else".

The mechanism already existed: `swarm-development-reload.path` watches for
`~/.local/state/swarm/development-reload.request`, and the control room writes
that file. What was missing was a way for an agent to ask.

## Decisions

### Only the worker whose workspace is the checkout

A worker in another repository has no business restarting this Hive — that is
restarting somebody else's control room to fix your own bug. The tool is not
offered at all unless the caller's workspace canonicalises to the configured
development checkout, so an unauthorised caller never sees it in `tools/list`
rather than discovering it and being refused.

Queen does not get it either. She coordinates; she does not build.

### Refused while the operator is at the Hive, not queued

Considered and rejected: hold the request and fire it once the operator leaves.
Rejected on the operator's ruling, and it is the right call — a queued reload
arrives at a moment nobody chose, possibly into a control room that has since
been reopened, which is exactly the surprise restart the guard exists to
prevent. Being told "not now" is a worse experience for the worker and a better
one for the operator, and the worker can simply ask again.

`status` stays readable while the operator is present. It changes nothing, and
refusing it would break the loop this whole decision exists to close.

### Two actions, because the API cannot answer a call that restarts the API

The obvious shape — one call that reloads and reports the new version — cannot
exist. The process serving the call is the process being replaced.

So `action=request` starts the build and returns the revision to expect, and
`action=status` reports the running build afterwards. This is not a workaround:
it is the only version that proves anything, because `status` reads what is
actually running after the swap rather than predicting it before. It is the
same discipline `/ship` requires of a human, and it means a worker must compare
`running_revision` to `expect_revision` before claiming a fix is live.

Workers keep running across the reload — the terminal host is a separate
service — which is what makes any of this feasible.

### Rate limiting comes free

The existing endpoint already refuses when the checkout has no product changes
to reload and when a build is in flight. A worker that tries to reload after
every commit is therefore refused by the mechanism it shares with the control
room button, with no separate interval to tune.

Sharing that path is deliberate: `start_development_reload` is called by both
the button and the tool, so a guard added to one cannot go missing from the
other.

## What this does not do

It does not let a worker release. Building and swapping a development checkout
is not cutting a release, and the release path still runs through the operator.

It does not change what "deployed" means. A worker that reloads this Hive has
verified its fix on this machine; the shipping vocabulary is unchanged, and
`swarm_record_deployment` still wants its evidence.
