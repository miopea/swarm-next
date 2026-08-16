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

The local operator may rename her Hive as a bounded public-label change. A
Keeper may likewise rename the active Apiary; Members can see that name but
cannot change it. Renaming never changes durable Hive, operator, Apiary, node,
or signing identities; membership, backend, policy revision, projects, tasks,
repositories, credentials, and ownership remain intact. Apiary renames are
append-only lifecycle events. Previously issued signed cards and invitations
remain immutable snapshots, while newly issued public identity material uses
the current label.

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
The normal operator experience wraps this exact protocol in a short-lived
Keeper invitation link. Opening it presents a content-free handoff page: the
recipient can name her personal Hive URL or confirm that she is already there,
then Swarm transfers the unchanged secret-bearing fragment in browser memory,
clears it from history, opens Settings -> Apiary, and prefills the connection
control. No relay receives the capability, and no membership change occurs
until the personal Hive introduces its signed identity. Manual paste and
connection-card or bundle files remain fallback paths rather than the primary
workflow. A QR code may later carry the same link without changing this trust
boundary. The invited Hive initiates the connection, presents its signed public
identity, becomes the exact bound candidate, and then previews and accepts the
resulting invitation.
Keeper's reachable HTTPS URL may be public or available through a trusted
LAN/VPN/mesh, but cannot be `localhost` for a multi-machine Apiary. Member Hives
need no inbound public address because federation connections originate from
the Hive toward Keeper. When Keeper is offline, local Hive work continues and
bounded federation events wait for ordered reconciliation; live cross-Hive
coordination is visibly unavailable until the route returns.
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
competing invitations.

The invited operator completes that handshake with one explicit private
browser command once the server-derived preflight is clear. The member API
seals or reuses the retry-stable submission, sends it outbound to the bounded
Keeper endpoint, verifies the signed acceptance, and applies membership
atomically. The response returns only the public local Apiary context: the
bearer secret, signatures, signed submission, and node credential remain
host-private. A temporary Keeper outage leaves the exact submission durable so
the operator can retry without creating a second membership. Jira continues to
sync directly from this Hive; the Keeper connection carries only federation
state and later Native Apiary work.

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
not delivery: the Member initiates every request, and this endpoint performs no
Jira or membership mutation.

The joined Member can now verify and durably acknowledge one of those signed
snapshots through a private local command. Verification is bound to every
Keeper, Apiary, and member identity in the stored membership receipt, requires
an unexpired membership credential, validates the canonical project digest,
and rejects altered, stale, or rollback snapshots. Exact retries are
idempotent. The acknowledgement records the digest, policy revision, project
count, and timestamps but deliberately does not claim local Jira access,
workflow readiness, or policy acceptance. The API now performs this fetch and
acknowledgement automatically on a bounded Member-owned loop; the per-project
Jira readiness remains local evidence rather than Keeper data.

The Member can also inspect a server-derived convergence view for the latest
acknowledged snapshot. It compares immutable Jira project IDs with the Hive's
private bindings and reports access and workflow readiness per project, plus
explicit blockers for a missing or stale catalog, unavailable Jira identity,
policy-revision drift, and incomplete project access. A fresh signed catalog
therefore never silently becomes usable shared work merely because it arrived.

The Keeper now exposes an authenticated claim coordinator for promoted Jira
issues. A Member reserves an issue by immutable project and issue ID before it
attempts Jira's human-assignee write. One active reservation or confirmed claim
exists per issue across the Apiary; an exact retry by the same member returns
the same record and another member receives a conflict. Reservations expire
after two minutes and may be explicitly released if Jira assignment fails.
Only the reserving member can confirm after Jira acknowledges assignment, and
confirmation makes home-Hive ownership durable. A confirmed claim cannot be
released through the reservation recovery endpoint; it moves through the
explicit handoff workflow described below. All claim endpoints require the
bounded member node credential, disable caching, and never receive or expose
Jira credentials. The Member-side claim and handoff sagas journal intent before
side effects, reconcile in the background, and preserve ambiguous outcomes for
retry instead of replaying Jira writes.

Keeper can now inspect a bounded, operator-authenticated ownership rollup built
from that same authoritative claim ledger. It includes only unexpired
reservations and confirmed home-Hive ownership, enriched with public Hive,
operator, and promoted-project names. Released and expired attempts, node
credentials, receipts, Jira credentials, task content, repositories, terminals,
and routine worker activity are omitted. This is an on-demand control-room read,
not presence or a background polling claim.

The outbound adapter foundation is also bounded independently of any browser:
remote endpoints require HTTPS and may not embed credentials, queries, or
fragments; redirects are rejected; each request has a five-second connection
and total deadline; and response bodies stop at one mebibyte before JSON
decoding. Network loss, authentication rejection, claim conflict, remote
rejection, oversized data, and invalid protocol content remain distinct typed
results. Loopback HTTP exists only for local development and isolated tests.
The client now powers both the explicit one-time join and the automatic
post-membership catalog pull. Secrets remain host-private and every connection
still originates from the Member toward Keeper.

Member Hives now also retain a content-free reconciliation health record with
an explicit condition, last attempt/success timestamps, consecutive failure
count, and next eligible attempt. Temporary outages use a deterministic
5/15/30/60/120/300-second bounded backoff; authentication and protocol
incompatibility halt until operator action. The private Member UI combines this
health with catalog/Jira readiness without exposing endpoints, credentials,
receipts, issue content, or response bodies. The API runner evaluates the
durable next-attempt boundary every 15 seconds, fetches at most once per minute
while healthy, and resumes temporary failures according to the stored backoff.
Personal and Keeper Hives perform no Member polling.

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

Every Apiary permanently chooses one canonical provider-work backend:

- **Jira-backed Apiary**: Jira is canonical for Jira issues and every active Hive connects its
  operator Jira identity and receives all promoted Apiary projects. Each Hive
  reads and writes Jira directly; issue bodies, comments, statuses, and
  assignees do not relay through Keeper. Keeper distributes only the bounded
  project catalog, policy, membership, and cross-Hive coordination facts.
- **Native Apiary**: Swarm is canonical for all shared work and supplies first-class distributed
  task synchronization, ownership, event propagation, offline queues,
  reconciliation, and conflict handling. Member Hives retrieve that shared
  work by polling Keeper and submit ordered changes over the same outbound
  route.

Mixed provider backends and backend conversion are not supported. Native is a
substantial later capability, not a free fallback. Swarm-generated Apiary tasks
are a platform coordination source in every Apiary: Keeper is canonical for
them and member Hives retrieve them by polling Keeper. They never contain or
proxy Jira issue content. Private Hive tasks and Hive-owned Jira projects remain
available because they are outside Apiary shared work.

All Swarm-to-Swarm federation traffic is member initiated. A member Hive polls
the reachable Keeper HTTPS endpoint for invitations, policy/catalog changes,
Steward authority, coordination, and Swarm-generated Apiary task state. Keeper
never requires an inbound route to a member machine. This is
separate from Jira-backed issue synchronization, where every Hive talks to
Jira directly using its own operator identity.

Steward authority travels in a separate credential-bound snapshot after the
project catalog is accepted. Keeper returns only the authenticated operator's
current scope. The Member replaces that projection atomically; an empty scope
explicitly revokes prior authority. Invalid identity, protocol, or shape halts
reconciliation for operator attention, so a rolling-version mismatch cannot
leave stale authority active. No Jira issue, worker, repository, terminal,
task, credential, or provider-session content enters this snapshot.

Member changes to Swarm-generated Apiary tasks use a durable outbound command
queue. Every command has a unique identity, the last observed task revision,
and one bounded claim or lifecycle transition. The Keeper persists the exact
command and receipt atomically with the canonical task event. Retrying an
identical command returns the same receipt; changing a command under an old
identity fails closed. A stale revision becomes an operator-visible conflict,
not an implicit overwrite. Members record an attempt before network I/O, flush
queued commands before polling newer task events, and retain commands across
offline periods and process restarts. Routine success stays quiet; conflicts
and rejections remain visible for review. None of this path reads or mutates
Jira.

Keeper now has a first-class operator flow for creating those canonical Swarm
tasks. Creation records a focused outcome, optional context, and priority. The
Keeper may leave the work unassigned for a Member Hive to claim or route it to
one active Member Hive. Routing identifies only the public Hive; it never
selects or exposes that Hive's private worker, repository, terminal, or provider
session. Member Hives receive the task through their existing outbound poll.
Keeper Queen receives bounded public-Hive list/create authority through her
private agent tools. A Member Queen receives bounded list/claim/lifecycle
authority, with every mutation entering that Member's durable outbound command
queue. Neither role can address another Hive's private workers, repositories,
terminals, or provider sessions, and ordinary workers receive no Apiary-level
tools. This keeps unattended coordination inside the existing federation and
conflict rules rather than creating a privileged side channel.

After routed work reaches its home Member Hive, that Hive's Queen may select
one reviewed local repository worker and materialize one durable private task.
The bridge is idempotent: retries reuse the same local task and worker rather
than duplicating work or silently reassigning it. The local task owns the
worker, repository path, provider conversation, dispatch, and evidence; none of
those fields enter Keeper's task or federation feed. Keeper continues to see
only the public home Hive and canonical shared lifecycle. Ordered lifecycle
mirroring remains a separate durable-outbox capability because local worker
progress may advance while Keeper is offline and must never be collapsed into
best-effort writes.

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
deterministic tools restricted to the granted scope. The synchronized scope is
visible before those tools are enabled; presentation alone never authorizes a
remote action.

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

Departure is Member-initiated and outbound-only. Before contacting Keeper, the
Member atomically freezes new shared commands. Keeper rechecks active shared
claims, Keeper-canonical tasks, and Stewardships in the same transaction that
ends membership and creates a signed receipt. Exact retries return that receipt;
ordinary federation operations reject the departed credential. A known Keeper
conflict reactivates the Member so blockers can be cleared. An uncertain network
outcome remains visibly frozen and reload-safe until the same request succeeds.
Only after verifying the receipt does the Member remove shared projections and
its credential. Private workers, repositories, provider conversations, tasks,
settings, and Hive integrations remain; Apiary Jira bindings become Hive-owned.

A confirmed Jira home moves through a Keeper-authoritative offer rather than a
direct reassignment. The current home offers one active destination Hive; the
destination durably accepts, assigns Jira through her own integration, then
confirms at Keeper. Keeper changes the home Hive only in that final atomic
confirmation. Exact retries return the same result, accepted handoffs never
expire silently, and the destination Queen waits for confirmed Hive ownership
before assigning a private worker.

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
