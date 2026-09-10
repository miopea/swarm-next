# ADR 0096: Authenticated public Hive directory

Status: **Accepted** — implements the operator-approved September 10 onboarding
and shared-identity outcomes. Implementation is not yet complete.

## Context

Keeper currently stores joined Hives, but members only receive Keeper and self
identity during joining. The signed project catalog contains no roster. A local
Hive rename changes only its own database. Thus members cannot see a consistent
Apiary directory, and successful local renames do not propagate.

Appending fields to existing signed version-1 catalog payloads is unsafe during
mixed-version operation: older readers reconstruct a different canonical payload
and fail signature verification. Display identities must not become authority.

## Decision

Add a separately versioned, authenticated public-directory exchange. Keep the
existing project catalog wire format unchanged. The member initiates outbound
exchange with its existing active node credential; no public member endpoint is
required. The node additionally signs its own profile update, binding Apiary,
node, Hive and operator IDs, a monotonic profile revision and payload version.
Keeper verifies the pinned key and exact active membership before accepting it.

The public profile contains Hive name, operator display name and optional
operator-provided contact email. Email is contact information, not verified
authentication or a grant. Never infer email from another person's identity or
silently publish unrelated integration credentials. The joining UI previews the
shared profile. Existing installations can supply missing information explicitly.
Only the untouched default My Hive is renamed to <firstname>'s Hive during the
explicit join workflow; custom names remain unchanged.

Keeper issues a signed, complete directory snapshot bound to the requesting
member node and Apiary, with a monotonic directory revision, expiry, and at most
256 entries within the existing 1 MiB transport limit. Oversized directories
fail explicitly rather than truncating membership. Entries carry stable node,
Hive and operator IDs plus public labels and Keeper/member role. No private
worker, repository, task, session, credential or terminal data is included.

As with the signed catalog, verification permits at most five minutes of clock
skew for issue time. Expiry must remain after issue time, within the five-minute
snapshot lifetime, and later than the recipient's current time. This accommodates
independent machines without accepting indefinitely fresh or expired snapshots.

The recipient verifies the pinned Keeper signature and scope, rejects duplicate
or colliding identities, and replaces a dedicated directory projection in one
persistence transaction. It does not overwrite its local owner profile from a
stale Keeper echo, alter local_hive_identity, or create permission grants.
Snapshot revisions prevent rollback; the same revision is idempotent only when
its content matches. Removed entries disappear from the directory projection,
not from private operational records. Keeper alone determines membership/roles.

Profile changes commit locally with one coalesced latest-revision pending update.
Acknowledgement clears only the exact acknowledged revision, preserving edits
made during delivery. Directory refresh is part of the owned federation cycle,
not browser lifetime. Temporary failures retain the last verified directory and
use existing bounded backoff; signature/authentication failures fail closed.
The UI distinguishes saved locally, pending sharing, and synchronized identity.

Mixed-version peers lacking this endpoint retain existing shared-work behavior
and show directory synchronization unavailable. Never describe a partial roster
as complete or let absence of this display capability stop project/task sync.
Compatibility owner: Apiary application service. Remove this fallback once the
minimum supported federation build includes the directory endpoint, with explicit
release compatibility evidence; do not silently retire it during dogfooding.

An authenticated operator may explicitly request a synchronization retry after
repairing connectivity or updating an incompatible peer. This queues the existing
owned runner, retains failure/success history, and never marks the connection
healthy before verification. The request is atomic with its runtime event and
coalesces while queued. It neither replaces membership/credentials nor bypasses
signature, expiry, or authorization checks. Repeated invalid credentials halt
again normally; recovery does not require removing and rejoining the Hive.

## Required acceptance evidence

- Independent Keeper and two member databases converge on all three profiles.
- A member rename reaches Keeper and the other member; concurrent local edits
  survive older acknowledgements and snapshots.
- Default-name replacement preserves custom names and requires a usable name.
- Missing email is shown honestly; profile edits cannot change IDs or authority.
- Forgery, wrong membership, revoked credential, duplicate identities, replay,
  rollback, malformed fields and oversized snapshots cause no partial writes.
- Departure removes the directory entry without deleting private work.
- Offline/reconnect and old/new peer combinations preserve ordinary shared work.
- Browser verification covers invite, approval, profile preview, join, directory
  and rename recovery on Keeper/member layouts. Real memberships remain untouched
  during intrusive tests; use fictional identities in isolated instances.
