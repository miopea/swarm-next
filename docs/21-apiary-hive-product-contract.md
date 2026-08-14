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
