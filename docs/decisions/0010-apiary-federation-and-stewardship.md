# ADR 0010: Federated Apiaries and scoped stewardship

Status: **Accepted**

## Context

Swarm began with one local operator because that is the shortest path to
reliable dogfooding. The intended product must also let independently managed
developer environments cooperate without sharing Linux accounts, credentials,
repositories, terminals, or conversational context.

A single shared Queen would interrupt the personal Queen workflow and mix
unrelated context. A central execution server would concentrate memory,
credentials, and process ownership. Retrofitting identity and ownership after
tasks, integrations, and notifications assume a singleton operator would be
expensive and unsafe.

Shared task authority cannot be ambiguous. A Native Apiary is not a no-Jira
fallback: making Swarm canonical requires distributed synchronization, atomic
claims, offline queues, reconciliation, conflict handling, event propagation,
and durable ownership. Mixing Jira-backed and Native shared work inside one
Apiary would create inconsistent guarantees between Hives.

## Decision

The independently managed unit is a **Hive**. A Hive has exactly one operator,
one personal Queen, private workers, execution nodes, tasks, settings, and
integration identities. A Hive is fully useful without joining an Apiary.

An **Apiary** is an optional federation of Hives. It owns shared membership,
project catalog, atomic task claims, policy, audit, and cross-Hive routing. It
does not own Hive terminals, repositories, provider credentials, or routine
work.

Every Apiary selects one immutable canonical shared-work backend:

- **Jira-backed**: Jira is canonical. All active Hives connect their operator
  Jira identity and pass access checks for every promoted Apiary project.
- **Native**: Swarm is canonical and supplies the full distributed task
  protocol, durable event propagation, offline operation, and reconciliation.

The first implementation is Jira-backed. Native is a substantial later
capability with its own acceptance and failure testing. Hives may keep private
local tasks or Hive-owned Jira projects in either Apiary mode; these are not
part of the Apiary shared-work backend.

A Hive belongs to zero or one Apiary. Changing Apiaries is an explicit leave
followed by a fresh invitation and readiness check. Shared tasks never migrate
between Apiaries. The Apiary backend never changes. A sole Keeper Hive may
explicitly collapse its Apiary boundary after safety validation: Native shared
tasks become local tasks and Jira-backed Apiary projects become Hive-owned
bindings while identity and audit history are preserved.

The Apiary owner is the **Keeper**. A Keeper can delegate a **Stewardship** over
explicit Hives, Jira projects, and capabilities. A Steward remains the operator
of their own Hive and uses their existing Queen; the Queen receives additional
scoped tools and a distinct Steward presentation. Stewardship is optional. A
Hive without a primary Steward escalates directly to Keeper.

Authority is evaluated by deterministic application services. Queen and Keeper
recommendations are not authorization. A durable scope grant pre-authorizes
ordinary Steward actions, including an explicitly granted takeover capability,
without per-action Keeper approval. Assist and takeover remain visible,
exclusive, reasoned, and audited.

Personal workers are private by default. Authorized users can observe structured
status. Terminal or transcript drill-down is explicit and audited. Direct
control requires one exclusive engagement lease; owner or Steward takeover
replaces that lease visibly rather than injecting into an active session.

For a Jira-backed Apiary, each Jira project has one synchronization scope:
Hive-owned or Apiary-owned. Every Hive uses its operator Jira identity.
Promoting a project is a Keeper decision that distributes the binding to every
Hive after access and workflow readiness checks. Each Hive synchronizes through
its own identity; the Apiary coordinates configuration, atomic claims,
home-Hive ownership, and cross-Hive handoffs without routing routine
synchronization through the Keeper agent.

## Membership lifecycle

A Hive leaving an Apiary retains private tasks, workers, repositories, settings,
and Hive-owned integrations. Active shared work must first be completed, handed
off, or released. The Apiary retains authoritative shared history; the Hive
retains an audited departure receipt and its private records.

A Hive joins another Apiary only through a fresh Keeper invitation and the new
Apiary's normal identity, integration, access, policy, and version checks. No
task or authority is carried between Apiaries automatically.

Collapsing an Apiary into its sole remaining Keeper Hive requires no outstanding
invitations, Stewardships, cross-Hive handoffs, contributions, or departed
execution nodes. The action is explicit and audited; it never happens merely
because another Hive leaves.

## Consequences

- Single-operator dogfooding uses the same identity and ownership model as a
  future team deployment.
- Personal Queen interaction remains the primary developer workflow.
- Team execution scales across nodes instead of one privileged host.
- Apiary metadata and authorization are centralized while Hive execution and
  credentials remain isolated.
- Every material action carries an actor, Hive, scope, and audit record.
- A shared task has one home Hive. Cross-Hive work is an explicit handoff or a
  linked contribution, never two implicit owners.
- Multi-user UI, Jira synchronization, invitations, and distributed scheduling
  can ship after the local dogfood slice without a schema redesign.
- Apiary availability must not stop a Hive from continuing already-owned local
  work. New shared claims require an authoritative shared backend connection.
- Native and Jira-backed Apiaries share product concepts but use different
  provider-work adapters and cannot be converted. Swarm-generated coordination
  tasks remain Keeper-canonical in either backend and are polled outbound by
  members; Jira issue synchronization remains direct from each Hive to Jira.

## Alternatives considered

- One operator forever: rejected because it prevents the approved management
  and cross-team product without a later ownership retrofit.
- One shared Queen: rejected because it breaks the personal Queen workflow,
  creates concurrent-input contention, and mixes unrelated context.
- Central shared execution: rejected because it combines credentials,
  repositories, terminal control, and memory pressure on one machine.
- One private Queen plus one additional supervisory agent per manager: rejected
  because scoped tools on the existing Queen preserve the workflow with less
  noise and state.
- Keeper-owned Jira synchronization for all work: rejected because it creates
  an avoidable operational bottleneck and weakens per-user Jira authorization.
- Mixed Jira/Native shared work or backend conversion: rejected because claims,
  offline behavior, conflict resolution, and ownership would have inconsistent
  guarantees and require risky migration machinery.

## Validation

The foundation is valid when:

1. a fresh local installation creates one operator and one Hive without
   requiring Apiary setup;
2. durable workers and tasks belong to that Hive;
3. authorization types represent Keeper, optional scoped Stewardship, and
   ordinary operator access without a special singleton path;
4. an Apiary has one immutable canonical shared-work backend selection;
5. a second Hive can be introduced without moving the first Hive's workers,
   tasks, repositories, or provider sessions;
6. out-of-scope Steward actions and concurrent task claims fail closed;
