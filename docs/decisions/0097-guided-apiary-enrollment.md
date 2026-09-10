# ADR 0097: Guided enrollment and project-scoped Apiary readiness

Status: **Accepted product direction; implementation pending**

## Context

The operator approved reducing Apiary joining friction on September 10, 2026.
Keeper supplies shared configuration; members should not reconstruct projects
and policies manually. Adding IT exposed that departments do not necessarily
share Jira permissions. Existing membership must recover in place.

This supersedes ADR 0010's requirement for access to every promoted project.
It does not weaken ADR 0025's signed identity, exact policy acceptance, replay
protection, or local credential boundaries.

## Decision

Treat connection, membership, and eligibility for a project's work as distinct
states. Present one resumable enrollment journey, not settings scattered across
pages. Keeper approval is performed once for the exact Hive identity. The member
reviews the Apiary and policy once; readiness checks run as part of that journey,
not as a separate approval ritual. Membership completion needs no second Keeper
finalization. A failed transport resumes the existing operation.

The connected Hive receives authenticated shared configuration through its
existing outbound Keeper connection. "Push settings" describes the user outcome,
not inbound access to a developer's machine. Distribute shared Jira project
identities and supported shared policy/configuration; do not distribute Keeper
credentials, overwrite private Hive configuration, or silently change filesystem
roots, provider permissions, or terminal access.

Jira authentication remains personal. Reuse an existing ready connection and
verified identity; otherwise put its connect action in the enrollment journey.
Keeper-supplied configuration is not proof of Jira access. Verify access using
the member's identity before enabling a project. Reuse verified compatible
configuration where supported; request only missing or conflicting local input.
Never infer workflow mappings merely from similarly named statuses.

For a nonempty shared Jira catalog, one eligible project is sufficient for
project participation; lacking another department's access is not a Hive-wide
failure. A connected or enrolled Hive with no eligible projects must clearly
show setup remaining and receive no project work. Preserve existing empty-catalog
coordination behavior; do not manufacture an inaccessible project requirement.
Implementation must explicitly distinguish these states rather than reporting
unqualified readiness or silently relaxing old signed readiness assertions.

Later catalog additions and access revocation affect only the corresponding
project. Keep membership, identity, existing permitted project configuration,
private work, and workers intact. Reconcile by immutable project identity.
Assignment and claims must verify the relevant project scope, including stale
evidence and permission loss. A project with no eligible recipient has an
actionable ownership/setup gap, not an assignment to an arbitrary Hive.

Shared policy updates are received automatically. Updates that require renewed
consent show the exact revision and scope; receiving a policy is not accepting
new authority. No recurring acceptance for an unchanged revision.

## Delivery and validation

1. Trace the existing signed enrollment, catalog reconciliation, Jira binding,
   and assignment contracts before changing membership gates.
2. Implement project-scoped readiness and corresponding assignment enforcement,
   with explicit older-runtime compatibility. Do not deploy a predicate-only fix.
3. Consolidate setup into the resumable enrollment and member setup surface;
   show ready projects separately from optional/inaccessible projects and show
   the next required action without exposing implementation detail by default.
4. Prove independent Hive flows: overlap/disjoint departmental access, IT added
   after joining, revoked access, no eligible project, policy revision change,
   interrupted join, expired credentials, and retry without duplicate membership.
5. Verify desktop/mobile UI and existing members upgrading without rejoining.

No runtime implementation or live acceptance is claimed by this ADR.
