# Apiary and Hive product contract

Status: **Accepted foundation; staged delivery**

This contract records the product decisions approved during the first
daily-driver design review. It is an implementation boundary, not a promise to
ship every listed capability in the first dogfood build.

## Product hierarchy

- **Apiary**: optional organization-level federation and policy boundary.
- **Keeper**: Apiary owner with organization-wide visibility and authority.
- **Stewardship**: optional delegated scope over selected Hives, Jira projects,
  and capabilities.
- **Hive**: one operator's independently managed Swarm environment.
- **Queen**: the operator's primary coordinator inside one Hive.
- **Scout**: privileged built-in cross-repository provisioner used by Queen.
- **Worker**: durable private execution identity bound to its own repository.

Departments are labels and reporting groups, not another execution or task
ownership layer. A Hive can report directly to Keeper or through one primary
Steward. Backup observation may overlap, but there is one active escalation
route.

## Membership and shared-work authority

A Hive belongs to zero or one Apiary. It must leave its current Apiary before a
Keeper can invite it to another. Joining always runs the destination Apiary's
identity, integration, project-access, policy, and protocol readiness checks.
No tasks or authority transfer automatically between Apiaries.

A personal Hive may found one Apiary atomically. Its operator becomes Keeper,
the local Hive becomes the first member, and the selected shared-work backend
is immutable. A Hive that is already federated cannot found another Apiary;
changing organizations remains leave-then-join rather than conversion.

The executable invitation foundation is durable and fail-closed. A Keeper may
issue one bounded pending invitation per Apiary/Hive pair only while the target
Hive is personal. Acceptance uses one atomic transaction: the matching current
invitation is accepted, the Hive joins that Apiary, and competing invitations
are revoked. Readiness is a sealed domain result covering identity,
integration, promoted-project access, policy acceptance, protocol compatibility,
exclusive membership, and expiry. No browser or agent payload may assert its
own readiness. Distribution transport and operator UI remain separate later
slices; Native Apiary joining stays unavailable until its real distributed
backend can satisfy the same contract.

Policy acceptance is revision-bound and belongs to the invited Hive operator.
Each invitation records the Apiary policy revision required when it is issued;
the operator must explicitly accept that exact revision before the sealed join
readiness can become ready. If the Apiary policy revision changes, prior
acceptance becomes stale and joining fails closed until a new invitation and
acceptance are recorded. Keeper, browser, and agent payloads cannot manufacture
or override this evidence.

The application command surface now owns founding an Apiary, listing local
invitations, accepting the exact policy revision, and joining. Join always
re-derives integration and promoted-project readiness at command time; a stale
browser snapshot is display evidence only and cannot authorize membership.

Separate installations bootstrap trust with a durable signing identity rather
than a shared browser token. An operator may download a one-day signed Hive
connection card containing only public identity and version evidence. The card
expires and grants no membership or access. An active Keeper may import it to
verify and pin the exact node, Hive, operator, and public-key tuple as a
candidate. Re-import may refresh display metadata and validity evidence but can
never replace a pinned key. Candidates remain visibly separate from members and
invitations. The full signed-envelope and one-time-secret transport is specified
by ADR 0025; candidate pinning does not pretend that distributed invitation
delivery already exists. Keeper-side invitation issuance is now executable: the
operator explicitly chooses a pinned candidate and downloads one signed,
24-hour invitation bundle. It contains the exact Apiary/backend/policy/catalog
and endpoint facts plus a bearer secret shown only once; Keeper storage retains
only its digest. A pending bundle is visible beside that Hive, blocks duplicate
issuance, and blocks collapsing the Apiary. Downloading the bundle still grants
no membership or execution authority. On the exact invited personal Hive, the
operator can now review the Apiary, Keeper Hive/operator, backend, policy
revision, promoted Jira project manifest, and expiry before explicitly pinning
the Keeper key and saving the invitation. The manifest is bounded and its
canonical digest is part of the signed envelope, so removal, substitution, and
renaming fail closed. Both signatures, every Keeper and invited identity field,
expiry, HTTPS endpoint, supported backend, manifest digest, and the 256-bit
secret are revalidated by the private application command. The secret and
complete envelope remain private to the Hive database; browser reads expose
only a sanitized pending-join summary and public project identities.
Import does not join, accept policy, share work, or grant terminal access.
The invited Hive can separately acknowledge only the exact signed policy
revision. That local transition grants no membership and performs no network
request. Browser reads then include server-derived preflight evidence: current
Jira connection state and, for every signed project identity, whether this Hive
has a matching access-verified binding and completed workflow map. Local Jira
matching uses immutable project ID rather than mutable key or display name.
Only policy acknowledgement plus ready Jira and project evidence produces
`Ready to contact Keeper`. A locally derived ready state can now be sealed into
one retry-stable signed submission. The Keeper's public federation endpoint
authenticates it with both the pinned Hive signature and one-time bearer secret,
rechecks current policy and catalog identity, then atomically creates exactly
one remote membership and returns a Keeper-signed receipt plus bounded node
credential. Identical lost-response retries return the same durable result;
altered replay fails closed. The invited Hive now verifies that result against
its pinned Keeper and exact invitation, then atomically stores the private
credential, mirrors public Apiary identity, becomes a Member, and revokes
competing invitations. The bounded outbound HTTPS adapter remains the next
handshake slice.

The invited operator can now prepare that signed submission through a private
browser command once the server-derived preflight is clear. The response
returns only the refreshed invitation overview: the bearer secret, signature,
and signed submission remain host-private. The UI labels this as prepared and
durable, never delivered or joined. Repeating the command reuses the exact
stored submission; no network request is made by this slice.

Both Keeper and Member Hives can now read a deliberately narrow Apiary roster:
Hive name and ID, operator name and ID, Keeper/member role, and which row is
local. The roster describes durable registration rather than live presence and
never includes node credentials, signed receipts, repositories, tasks, or
terminal state. Keeper-pinned candidates remain visibly separate until the
authenticated handshake creates membership.

A joined Hive's bounded node credential can now authenticate a read-only
Keeper catalog request. The Keeper returns a five-minute Ed25519-signed
snapshot bound to that exact member node, current policy revision, and the
canonical digest of the ordered promoted-project identities. The response is
`no-store` and contains no node credential, Jira credential, access evidence,
workflow map, issue content, task, repository, or terminal data. Invalid and
expired credentials fail as authentication failures. This is a pull contract,
not delivery: Member-side acknowledgement, offline reconciliation, and
outbound polling remain later slices, and this endpoint performs no Jira or
membership mutation.

The joined Member can now verify and durably acknowledge one of those signed
snapshots through a private local command. Verification is bound to every
Keeper, Apiary, and member identity in the stored membership receipt, requires
an unexpired membership credential, validates the canonical project digest,
and rejects altered, stale, or rollback snapshots. Exact retries are
idempotent. The acknowledgement records the digest, policy revision, project
count, and timestamps but deliberately does not claim local Jira access,
workflow readiness, or policy acceptance. Automatic polling and the per-project
readiness/reconciliation loop remain separate adapter work.

For Jira-backed Apiaries, promoted projects now have a separate durable catalog
owned by the Apiary. Only its Keeper may promote a Jira project, Native Apiaries
reject Jira promotion, and a joining Hive passes project readiness only when
every promoted project has a matching access-verified, fully mapped local
Apiary binding. An empty catalog is valid; a partially received or partially
mapped catalog fails closed. This is distinct from the Hive-owned Jira projects
that remain private to one operator.

Promotion is now one private application command rather than a UI convention.
It atomically adds the project to the Apiary catalog and converts the Keeper
Hive's existing ready binding to Apiary scope without replacing its workflow
mapping or issue links. The command rejects member Hives, Native Apiaries,
foreign bindings, and bindings without verified access and a completed workflow
map. Keeper UI shows both the promoted catalog and which local Hive projects are
actually ready to share. Invitation distribution now carries the exact
signed-digest project manifest into the invited Hive. Per-Hive Jira access and
workflow mapping are derived from private local bindings and presented per
project; manifest presence alone never claims that a remote Hive is ready.

Every Apiary permanently chooses one canonical shared-work backend:

- **Jira-backed Apiary**: Jira is canonical and every active Hive connects its
  operator Jira identity and receives all promoted Apiary projects.
- **Native Apiary**: Swarm is canonical and supplies first-class distributed
  task synchronization, ownership, event propagation, offline queues,
  reconciliation, and conflict handling.

Mixed mode and backend conversion are not supported. Native is a substantial
later capability, not a free fallback. Private Hive tasks and Hive-owned Jira
projects remain available in either mode because they are outside Apiary shared
work.

A sole Keeper Hive may explicitly collapse an Apiary after automatic safety
validation. Native tasks become local while preserving identity and history;
Jira-backed Apiary projects become Hive-owned bindings. Outstanding invitations,
Stewardships, handoffs, contributions, or departed nodes block the collapse.
Collapse readiness is derived from durable state and rechecked inside the same
transaction as detachment. The Apiary record and append-only lifecycle event
remain as audit history; collapse never deletes the federation identity. An
inactive Apiary cannot accept Hives, create invitations, or promote projects.

## Daily interaction

Operators continue to spend most of their time with their personal Queen. A
Queen assigns work only to workers in her Hive and respects operator engagement
leases. Operators may directly steer workers for ad hoc work without creating
a task.

A Steward uses the same personal Queen. The UI separates **My Hive** from
**My Stewardship**, and the Queen receives a visible Steward treatment plus
deterministic tools restricted to the granted scope.

Keeper receives structured milestones, blockers, capacity, policy exceptions,
and requested help. Routine terminal output and ordinary Queen conversations do
not enter the Keeper attention stream.

## Engagement and takeover

Viewing a terminal does not reserve it. The first operator input or a provider
waiting for operator input creates an engagement lease. While engaged, Queen,
Keeper, Steward, and automation queue requests rather than injecting them.

Observe, Assist, and Take Over are distinct operations. Takeover requires a
reason, is immediately visible, replaces the exclusive lease, pauses competing
automation, and is audited. Keeper grants Steward takeover capability once per
scope; an in-scope takeover does not require another approval.

## Tasks

The target canonical lifecycle is:

`Inbox -> Ready -> Active -> Review -> Delivery -> Done`

`Blocked` is available from active work and `Archived` removes work without
erasure. Assignment is independent from lifecycle status. A task is not done
until its configured definition of done, including shipping when approved, is
satisfied.

A shared task has exactly one home Hive. Cross-Hive work is either:

- a **handoff**, transferring home-Hive ownership; or
- a **contribution**, leaving the parent in its home Hive while creating a
  linked deliverable in the contributing Hive.

Keeper routes cross-Hive work to the target Queen, never directly to a private
worker. The target Queen may accept, negotiate, or decline ordinary requests.
An explicitly marked owner directive is distinct and audited.

A Hive leaving an Apiary must complete, hand off, or release active shared work.
The Apiary keeps authoritative shared history. The departing Hive retains only
an audited departure receipt and its private records.

## Jira-backed Apiaries

Jira identity maps one-to-one with the operator who owns a Hive. Jira remains
authoritative for external issue identity, mapped workflow state, and human
assignee. Swarm owns home-Hive assignment, worker assignment, execution state,
local notes, evidence, and terminal history.

A Jira project is either Hive-owned or Apiary-owned:

- Hive-owned projects synchronize and operate locally.
- Keeper may promote a project after every active Hive passes Jira access and
  workflow readiness checks.
- Promotion automatically distributes the project binding to every Hive.
- Each Hive synchronizes using its own Jira identity and permissions.
- Unclaimed Apiary issues are visible to eligible Hives.
- Operator or Queen may claim within policy; a claim atomically sets the home
  Hive and normally assigns the Jira issue to that Hive's operator.
- Keeper can offer or assign work without approving routine Hive claims.

Temporary network loss is not an authorization failure. Already-owned work
continues locally and outbound updates queue with explicit bounds. New shared
claims require Apiary coordination. Confirmed credential or permission loss
enters a visible degraded state and blocks affected claims and writes.

The integration boundary reports these cases separately: not connected,
temporary network loss, invalid credentials, denied project access, and missing
workflow mapping. A temporary outage permits work already owned by the Hive but
never permits a new shared claim. Provider credentials remain adapter-private;
Queen consumes typed readiness and commands rather than tokens or browser state.

The first executable adapter slice is deliberately read-only. When all three
operator-owned settings (`SWARM_JIRA_BASE_URL`, `SWARM_JIRA_EMAIL`, and
`SWARM_JIRA_API_TOKEN`) are present, the API performs a five-second Jira Cloud
`/rest/api/3/myself` identity probe and returns only typed readiness plus the
account display name. Remote transport must be HTTPS, credentials embedded in
URLs are rejected, partial configuration fails closed, the endpoint is
operator-authenticated and `no-store`, and the UI keeps local work available
for every degraded state. Project bindings, workflow mapping, issue sync, and
writes remain outside this slice and require separate tested contracts.

## Presence, attention, and mobile

Presence modes are **At the Hive**, **Away**, and **Night Watch**. On supported
desktop browsers, operating-system lock detection participates in presence.
Authenticated heartbeat, real interaction, visibility, timeout, manual
override, and schedule provide layered fallback. Presence tunes notification
routing and escalation; it never expands authorization.

Away delivers actionable phone notifications. Night Watch continues
pre-authorized work and sends only blocking, failed, security-sensitive, or
approval-required alerts. Lock-screen detail defaults to private and can be
configured to include task/worker or full question content.

The Android Chrome/Edge installed PWA is a first-class client of the same React
application. It includes Queen, attention, tasks, workers, settings, approvals,
and terminals. The terminal retains direct input for slash commands and TUIs,
plus an integrated long-form voice composer. Mobile controls include a
collapsible D-pad, Enter, Escape, Tab, Ctrl, interrupt, and a provider-aware
permission-mode selector. Controls are customizable per device class.

The initial service worker is limited to push and notification navigation. It
does not cache the application shell. Registration, update, memory, and revoke
paths require browser tests because Legacy experienced a service-worker-related
memory regression.

## Queen, Scout, and unattended work

Queen remains an always-active terminal and may coordinate pre-authorized work
while the operator is away. Queen uses a policy matrix by repository,
environment, and action; a generic confidence score alone never authorizes a
deployment or external side effect.

When parallel work would relieve a real repository bottleneck, Queen may ask
Scout to provision managed worktrees under policy. Scout validates the base,
creates bounded worktrees and temporary workers, registers them, verifies
readiness, and reports to Queen. Queen assigns the prepared work. Repository
ownership remains with the primary worker, and cleanup is explicit.

## Configuration and recovery

Settings use progressive disclosure with synchronized account preferences,
separate desktop/mobile presentation, per-device overrides, workspace defaults,
and non-overridable organization policy. Provider controls are capability
driven. Sections support live preview and targeted reset.

Versioned configuration export and encrypted full backup/restore are designed
but deferred beyond initial dogfood. Repositories are never included; external
environment provisioning restores them. Full backup later requires integrity
checks, disposable restore verification, pre-upgrade creation, and rollback to
prevent backup rot.
