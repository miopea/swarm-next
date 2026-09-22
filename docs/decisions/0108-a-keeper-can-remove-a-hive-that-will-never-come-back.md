# 0108 — A Keeper can remove a Hive that will never come back

- Status: Accepted
- Date: 2026-09-22

## Context

An operator reinstalled a machine and could not add the new Hive cleanly,
because the old one was still on the Apiary roster and nothing could remove it.
Their words: "I still cannot delete a hive from an apiary as I need to add this
wsl one as it is a new install."

Departure is built and is good. `depart_federation_member` ends exactly one
membership, detaches the remote Hive, and returns a Keeper-signed receipt the
member applies to itself — private workers, tasks, repositories and credentials
stay put, and only shared projections leave. `keeper_departure_readiness`
refuses while the member still holds confirmed Jira claims, open shared tasks or
live stewardships, so leaving cannot strand shared work.

Every one of those paths is reached through the MEMBER'S OWN CREDENTIAL:

- `departure_member_context` looks the membership up by `credential_digest`
- and requires `credential_expires_at > now`.

That follows ADR 0034's outbound-only rule, where a Keeper never reaches into a
member. The consequence is that departure is something a member DOES, never
something that can be done ABOUT it. So:

- A Hive that has been reinstalled, decommissioned or lost cannot depart,
  because nothing is left to make the call.
- A Hive whose federation credential has lapsed cannot depart EITHER, even if it
  returns, because the lookup requires an unexpired credential.

The roster then keeps a member nobody can remove, and the Apiary's counts,
version standing and raises describe a machine that no longer exists. The
operator sees a Hive they cannot act on and cannot delete.

## Decision

A Keeper may remove a member from its own Apiary, keyed on the member's HIVE ID
rather than on a credential it cannot produce.

The removal performs the identical transaction the member-initiated departure
performs — the same readiness check, the same signed departure receipt written
to `apiary_federation_departures`, the same membership moved to `departed`, the
same `hives.apiary_id` cleared — and differs only in how the membership is
found.

This does NOT weaken outbound-only. Nothing is pushed to the member and no
connection is opened to it. The Keeper changes only the state it already owns:
its own roster and its own shared projections. A member that never comes back
costs the Apiary nothing.

**A member must be able to honour a removal it did not ask for.** Applying a
departure receipt required the member's own membership to be `departing`, which
only `begin_federation_departure` sets — so a Keeper-initiated receipt was one
the member could never accept, and the two would disagree about membership
permanently. A test written for this ADR is what established that; it was
assumed to work and it did not. `apply_federation_departure` now accepts an
`active` membership as well, with the signature check unchanged: the receipt is
still verified against the Keeper public key pinned at invitation and against
that exact membership. A Keeper that could mint this receipt could already
refuse the member service entirely, so honouring it concedes nothing the Keeper
did not already hold — and the member keeps every private worker, task and
repository either way.

Readiness is NOT bypassed. A Keeper removing a member with confirmed claims,
open shared tasks or live stewardships is refused with the same blockers, for
the same reason: removal must not strand shared work. Clearing that work first
is the Keeper's job and it is visible on the board.

## Consequences

- The roster stops describing machines that no longer exist, so version standing
  and raises mean what they say.
- A reinstalled Hive is an ordinary new member rather than a second row beside a
  dead one.
- Removal is Keeper-only and refuses on outstanding shared work, so the blast
  radius is the same as a member choosing to leave.
- A removed member that returns is told by its own receipt, not by a surprise.
  Its private work was never touched.
- The expired-credential lock is now only on the member's own path, where it
  belongs: a member proving who it is needs a live credential, and a Keeper
  acting on its own roster does not.

## Alternatives rejected

**Let the member's credential be renewed so it can depart itself.** Requires the
machine to exist. The case that matters is the one where it does not.

**Delete the membership row.** Loses the receipt, so a returning member would
have no evidence it was removed and would look like an intruder. The departed
state and its receipt are what make the two agree afterwards.

**Let removal ignore readiness.** Turns one operator's tidy-up into stranded
shared work on other Hives. The blockers are already computed and already
explain themselves; refusing with reasons is better than a surprise.
