# ADR 0031: Retry-safe Apiary departure

Status: **Accepted**

## Context

A Member Hive must leave one Apiary before it can join another. The action
crosses two independently operated installations and may lose its HTTP response
after the Keeper commits. A naive sequence can either abandon shared work or
leave the Member uncertain whether it is still authorized. Replaying a broad
membership credential after departure would also keep unrelated federation
operations available longer than intended.

Private Hive state must not be coupled to the Apiary lifecycle. Workers,
repositories, provider conversations, private tasks, settings, and Hive-owned
integrations belong to the Hive. Shared ownership, Steward authority, and
Keeper-canonical task projections belong to the Apiary.

## Decision

Departure is one explicit, Member-initiated, outbound-only protocol:

1. The Member checks local durable blockers, then atomically marks its
   membership `departing`. This freezes new shared mutations but does not remove
   any private or membership state.
2. The Member sends the same credential-bound departure request to the
   reachable Keeper. The Keeper rechecks active Jira claims, open
   Keeper-canonical tasks, and Stewardships in the transaction that marks the
   membership departed.
3. The Keeper stores and returns one signed departure receipt. An exact retry
   after a lost response returns the same receipt. A departed credential is
   accepted only by this exact departure endpoint; normal federation reads and
   commands reject it.
4. The Member verifies the Keeper signature and every identity in the receipt,
   stores its own audit copy, converts Apiary-scoped Jira bindings to Hive scope,
   removes only shared projections and the membership credential, then returns
   to Personal Hive mode atomically.

An authoritative Keeper conflict returns the local membership to `active`
because known blockers remain. A timeout or unavailable Keeper leaves the local
state `departing`; the UI says that no partial departure occurred and offers an
exact retry. Status remains readable after a browser or application restart.

The operator must review what remains local, clear every named blocker, and
type the Apiary name before the first request. The Keeper never reaches inbound
to the Member Hive.

## Consequences

- A lost success response cannot create two departures or strand a half-removed
  local membership.
- New shared writes stop while the outcome is uncertain, but private workers and
  already-local work remain available.
- A Member cannot silently abandon confirmed shared ownership or Steward
  responsibility.
- Departure does not migrate tasks or authority to another Apiary. Joining
  elsewhere still requires a fresh invitation and full readiness checks.
- Jira-linked history remains readable after leaving because the local bindings
  become Hive-owned; Jira itself remains authoritative for issue history.
- Keeper and Member retain independent, signed audit evidence without retaining
  a generally usable departed credential.

## Alternatives considered

- Delete local membership first: rejected because Keeper failure would strand
  shared ownership without a valid Member identity.
- Let the Keeper remove a Hive asynchronously: rejected because federation is
  outbound-only and the Hive operator must explicitly control departure.
- Treat a timeout as failure and reactivate immediately: rejected because the
  Keeper may already have committed, allowing unsafe new shared mutations.
- Force the Keeper to preserve all normal credential access for retries:
  rejected because only the idempotent departure operation needs post-departure
  replay.
