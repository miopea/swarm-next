# ADR 0100: Repeatable Apiary membership

Status: Accepted implementation direction for the operator-requested rejoin fix.

A clean departure ends authority, not identity or history. A fresh approved
invitation may enroll the same pinned node/Hive/operator again. An existing
active membership still conflicts. Rejoin must not generate replacement IDs,
overwrite receipts, reactivate old credentials or remove private work.

Keeper membership records are membership **epochs**, identified by their signed
receipt and invitation. Preserve departed rows and departure references. Enforce
one active membership per Hive, operator and Apiary/node with partial unique
indexes rather than forbidding historical epochs. Keep immutable receipt fields.
The persistence migration copies all rows transactionally, preserving foreign
keys and validating integrity through the existing migration boundary.

Reuse public operator/Hive rows only when the exact pinned node/Hive/operator
has a departed epoch and the Hive is currently unassigned. Arbitrary collisions
remain conflicts. Active roster queries exclude historical epochs.

An old invitation replay may return its historical receipt for idempotency but
cannot confer new authority. Old credentials remain departed; replaying an old
departure must not detach the newly joined epoch. Member receipt application
must reject an old departed epoch rather than install it as a fresh membership.

Acceptance: two-Hive leave/rejoin, stable identity/private work, distinct new
receipt, single active roster row, old credential rejection, old departure and
acceptance replay, active-identity collision, migration preserving historical
receipt references and immutable fields. No manual live database repair.

## Invitation completion and cancellation

The operator requires Keeper cancellation until actual joining, not merely until
delivery. The persistence transaction revokes the bootstrap link and its pending
signed invitation together. Consumption and revocation are mutually exclusive;
once consumed, cancellation refuses without affecting membership or its receipt.
The public link projection adds a default-false `membership_confirmed` field,
derived from that exact invitation's consumption, not another membership for the
same Hive. Older observations without the field remain unconfirmed. Keeper UI
keeps unconfirmed invitations actionable and moves confirmed invitations to
collapsed history. This is not the separately requested member-removal feature.

Member cancellation of a recorded enrollment retires its matching unsubmitted
imported invitation in the same transaction as removing the bootstrap journal.
Legacy bootstrap cleanup without an enrollment must preserve its invitation.
If joining is already in progress or the signed request is submitted, refuse
and roll back: local cancellation cannot prove a remote membership never formed.
Such a conflict is not a missing-link 404. Receipt reconciliation remains required
before removing an uncertain submitted join.
