# 0117 — A membership renews itself before its credential lapses

- Status: Accepted
- Date: 2026-10-14
- Decided by: the operator, on 2026-10-14, in the Swarm Next worker's terminal:
  - "Build it now (Recommended)", asked whether to build renewal now;
  - "Renews itself, 60-day grace (Recommended)", asked how a Hive whose
    membership already expired gets back in.
- Amends: ADR 0025, whose node credential was bounded and never renewed. Leaves
  ADR 0108 standing: removal is still something the Keeper does, and it now
  also ends a member's grace period.
- Task 01a13c1a. The design note is on that task.

## Context

The operator asked why their Keeper's roster showed Hives "not reporting" when
they believed every one was on 1.17.2.

When a member joins, the Keeper issues it a node credential and a signed
membership receipt that expire thirty days later
(`FEDERATION_NODE_CREDENTIAL_LIFETIME_SECONDS`). Nothing renewed them, and both
sides refuse an expired one:
- The Keeper's credential lookups require `credential_expires_at > now`.
- The member stops its sync pass at its own expiry check.
- `verify_federation_membership_receipt` bounds the lifetime from `joined_at`.

So every member fell out of its Apiary a month after joining. The roster blamed
the Hive: "This Hive hasn't reported to the Keeper yet".

On the real Keeper on 2026-10-14:
- Honeysuckle had expired on 10-10, and Angie's, Moleek's and Paul's on 10-11.
- Simply Bee-rilliant was due to expire that night, and WSL on 10-22.
- Angie's, Moleek's and Paul's had never contacted the Keeper at all, even while
  their credentials were valid. That is a second problem, with a cause on their
  side; renewal does not explain or fix it.

## Decision

### 1. A member renews on its own sync pass

From fifteen days before its credential expires, each sync pass calls
`POST api/v1/federation/membership/renewal` with the current credential.
- The call comes before the member's own expiry check, so an expired credential
  can still renew.
- A failure leaves the credential as it was, and the next pass tries again.

The Keeper:
- mints a new credential;
- sets its expiry to thirty days from now;
- re-signs the same receipt, with the same `receipt_id` and `joined_at`, a new
  `credential_expires_at` and a new `credential_issued_at`.

The member accepts the renewal only when all of these hold:
- the receipt verifies against the Keeper key pinned at join;
- it names the same membership: receipt, invitation, Apiary, Keeper and member
  identities, and `joined_at`;
- it lasts longer than the credential the member holds now.

### 2. A lost response does not lock a member out

The Keeper keeps the digest of the credential a renewal replaced
(`previous_credential_digest`). That credential can do exactly one thing: ask
again, and get the same renewal back. Every other call needs the new one.

### 3. Sixty days of grace, ended by Remove

An expired credential may renew for sixty days after it expires, and do nothing
else. The membership must still be active, so a Keeper that removed the Hive
(ADR 0108) has ended it. Past the grace period, only a new invitation brings a
Hive back.

### 4. The receipt's lifetime runs from when its credential was issued

`credential_issued_at` is an optional last field, skipped when absent. A receipt
issued at join therefore serialises to exactly the bytes it was signed as, and
its signature still verifies. The thirty-day bound is measured from
`credential_issued_at`, or from `joined_at` when it is absent.

Releases from before this change never call renewal, so they never see the new
field.

### 5. Storage

Schema 202:
- adds `previous_credential_digest` to the Keeper's memberships;
- relaxes both immutability triggers, so the credential, receipt and expiry can
  change. What the membership is (receipt, invitation, Apiary, member, Keeper
  and `joined_at`) stays frozen.
- adds a trigger on each side so the database itself accepts a new receipt only
  when it carries the same receipt id and join date, and on the Keeper the same
  member.

### 6. The roster names the cause

The Keeper's member summary carries `credential_expires_at`.
- An expired member shows "Membership expired <date>", and says it reconnects
  once it updates, until the grace period ends. That line replaces its policy
  gaps, since the expiry is what causes them.
- Past the grace period, it says to remove the Hive and invite it again.
- A member with fourteen days or fewer left has not been renewing, so the
  roster warns about it.

## Consequences

- Each member keeps itself in its Apiary for as long as it syncs at least once
  every forty-five days.
- Renewal reaches a Hive only through a release. Members expire on schedule
  until they install it. Within the grace period they then renew by
  themselves, with nothing for either operator to do.
- A leaked credential stays useful for its thirty days, as before. Expiry now
  bounds a credential, not a membership.
- Angie's, Moleek's and Paul's Hives need someone to find out why they never
  reached this Keeper. Renewal gives them a way back once they do.
