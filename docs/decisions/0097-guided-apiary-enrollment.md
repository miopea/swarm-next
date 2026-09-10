# ADR 0097: Guided enrollment and project-scoped Apiary readiness

Status: **Accepted product direction; implementation pending**

## Final enrollment interaction: submit once, Keeper approves

The operator explicitly requires three steps: Keeper generates a link; the
member pastes it, sees the management implications, and submits; Keeper approves
and membership completes automatically. There is no post-approval member
acceptance, readiness approval, or Keeper finalization. Jira is not a gate.

Before submission the member must see authenticated Keeper identity and the
exact management-policy revision. Submission durably records consent bound to
the link, Apiary, Keeper node, local node/Hive/operator, and policy revision.
The approved invitation must match that consent before its existing signed
join can proceed. A changed policy needs renewed consent; transport retries
must never manufacture it. Existing legacy invitations without that recorded
consent retain explicit acceptance rather than being silently auto-accepted.

The application owns bounded enrollment reconciliation, including while the
browser is closed. Persist each phase before its next external effect; retain
the signed submission and receipt for idempotent recovery. Closing the page,
restarting the API, or losing a response must not require another approval or
create duplicate membership. Cancellation must stop automatic progression.
Completion opens Apiary, where optional setup is shown separately.

New Keeper links carry a signed disclosure using a domain-separated signature
and schema1/management-terms1, independent of the later invitation schema.
The offer binds the exact link, Apiary, endpoint, Keeper connection card, policy
revision and expiry. Older links without it cannot opt into automatic acceptance.
Federation application owns this legacy fallback until supported invitations
have all expired or upgraded; never infer consent when migrating old links.

The member journal is bounded to 32 saved enrollments. It records immutable
consent and compare-and-swap phases: awaiting approval, joining, complete,
cancelled, or attention. Cancellation wins against a stale pre-join attempt.
Once a join request may have been sent, reconcile its receipt before offering
departure; never claim cancellation undid an uncertain remote membership.
Legacy saved links are migrated without invented consent. Removing a saved
link also removes its enrollment record; completed signed membership receipts
remain owned by the existing federation receipt store.

Acceptance requires independent-Hive tests of the exact three-step flow,
browser-closed completion, restart and response-loss recovery, policy/identity
substitution, cancellation, expiry, and existing-member upgrade without rejoin.
The deployed post-approval acceptance flow does not meet this requirement.

## Superseding operator decision: Swarm first, Jira optional

The operator clarified that joining accepts Apiary-wide Keeper management while
retaining the Hive's local task system. Swarm shared tasks are the baseline;
Jira is optional and neither a Jira connection nor one project is required for
membership. This supersedes the at-least-one-project enrollment discussion below
and ADR0010's Jira-only baseline. Existing Jira assignments retain Jira as their
source of truth; this does not authorize rewriting or converting existing work.

The Apiary tab is the member setup home: show what works now, missing setup with
direct actions and benefits, and things waiting on Keeper. Jira is an optional
enhancement, not a membership failure. Resolved setup prompts disappear.
Managed settings show Keeper ownership. Private credentials remain local;
machine/terminal capabilities still require explicit deterministic boundaries,
not an unrestricted remote-command channel implied by management consent.

Membership-only submissions use the unpublished schema2 described below even
when no Jira connection exists. Keep the older assertion only when its original
Jira readiness conditions hold. Older-runtime rejection must preserve invitation
and local work. Do not label basic shared work usable until its independent
task-feed and assignment paths pass no-Jira end-to-end tests.

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

### Join compatibility boundary

Project-scoped join assertions use submission schema2, with the existing signed
identity/policy/catalog fields and receipt format unchanged. All-project-ready
members may continue issuing schema1 assertions because the older assertion
remains true. New Keepers accept both; older Keepers reject schema2 before
consuming the invitation. Never silently retry a schema2 assertion as schema1.
The stored signed submission stays byte-stable across retries. This is not a
per-project access grant; project execution/claim checks remain independent.
The federation application owns schema1 compatibility until the supported
minimum member version implements project-scoped joining; removal requires
explicit migration of retained pending submissions, not their deletion.

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
