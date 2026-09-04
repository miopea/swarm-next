# Capability inventory

Status: **M0 draft recommendation**

This inventory is intentionally organized around outcomes rather than legacy
modules. Decisions are provisional until reviewed with the primary operator.

Decision meanings:

- **Keep**: preserve the outcome with minimal product change.
- **Redesign**: preserve the value through a new model or interaction.
- **Merge**: absorb the outcome into a clearer capability.
- **Remove**: do not implement in Swarm.
- **Investigate**: usage or value needs more evidence.

## Product center

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Persistent agent terminals | Redesign | Core value. Server-owned terminal sessions with sequence-based attach, bounded history, explicit color capabilities, desktop paste, and private bounded image attachments. |
| Worker launch and lifecycle | Redesign | Core value. Immutable worker-session identity plus separate durable provider-conversation identity; durable named repository roster, bounded recursive path completion, direct canonical path entry, explicit ordering, lifecycle, and recovery. A managed sleeping Scout is pinned after Queen for deliberate cross-repository work while retaining ordinary worker authority; exact existing Project Root identities migrate in place without losing conversation or history. Preview-first Legacy migration imports selected repository workers as sleeping durable profiles, preserves name/path/description/provider/order, refuses Queen/Scout duplication, and can optionally resume the exact matching local Claude or Codex conversation on first wake. It never imports a running process, transcript or terminal history, credentials, identity files, groups, or approval rules. |
| Multi-worker control room | Redesign | Core value. React workspace with instant switching and stable retained sessions. |
| Task board | Redesign | Preserve work management while simplifying task types, transitions, and presentation. |
| Task assignment | Redesign | Preserve manual and assisted assignment; separate recommendations from execution policy. |
| Task history and audit | Keep | Implemented as bounded, durable per-task events plus a quiet operator Activity view with task search and progress, assignment, and change filters. It excludes terminal output and transport noise. |
| Direct terminal input | Keep | Essential, with explicit input ownership, stale-session protection, provider-prompt guards, and content-free actor/shape attribution at the terminal-host write boundary. Typed content and terminal output never enter that audit. |
| Groups and bulk worker actions | Investigate | Likely useful, but validate actual use and whether workspace selection replaces groups. |
| Worker routing descriptions | Redesign | Implemented as operator-reviewed Hive metadata for Queen routing, distinct from provider memory and project instructions. Swarm can draft locally from bounded README/manifest metadata or optionally improve that packet with one tool-free, non-persistent, budget-capped Claude turn; neither path saves without operator review. |

## Automation and orchestration

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Routine approval drones | Remove | Provider-native permissions own provider tool approval. Keep only typed Swarm-level operator decisions. |
| Worker activity and attention | Redesign | Implemented from the bounded host-owned terminal surface: Sleeping is unloaded, Resting is live and idle, Buzzing is active or conservatively unknown, and operator decisions are explicit. Provider classifiers and content-free state-change events keep the roster authoritative without browser polling. An open provider selection or confirmation prompt is also an input authority boundary: unrelated automation refuses without losing its intended message, while any authorized answer binds to the exact current prompt and requires read-back. |
| Crash/revival automation | Redesign | Becomes worker lifecycle recovery, not a drone. |
| Host pressure management | Redesign | Observation-first API and terminal-host memory evidence now has explicit thresholds and operator visibility; automated recovery waits for soak evidence and a safe target. |
| Context-pressure handling | Investigate | Reassess against current provider compaction and context-management capabilities. |
| Completion verification | Redesign | Review-to-Completed now requires concise durable verification evidence from the operator or Queen, including release or handoff evidence when shipping was part of done. Deterministic, task-specific verification policies remain a later orchestration layer before model review. |
| Queen task assignment | Redesign | Preserve assisted coordination but clarify recommendation, authorization, and execution. |
| Queen completion detection | Redesign | Prefer explicit agent/task protocol signals; retain inference only as a fallback. |
| Queen unattended conductor | Redesign | Implemented as an opt-in durable event-driven review marker. It defers to operator engagement and Steward takeover, coordinates only local work within the presence ceiling, requires an exact MCP completion signal, and fails uncertain rather than replaying after interrupted delivery. Night Watch is the primary higher-authority journey: Queen may consume a durable, narrowly scoped operator grant such as deployment authorization, but cannot create or widen one and never replaces Scout or repository workers as the implementation actor. The grant model and audit surface remain to build. |
| Queen interactive conversation | Investigate | Validate frequency and whether it belongs in the unified operator conversation surface. |
| Proposal approval system | Merge | One generalized operator-decision inbox rather than feature-specific proposal surfaces. |
| Approval-rule regex engine | Remove | Do not infer authorization from rendered terminal text or maintain a second permission authority. |
| Standing improvement loops | Investigate | High resource and safety implications; require demonstrated ongoing value and hard budgets. |
| Playbook synthesis | Investigate | Potentially valuable, but assess actual retrieval/use before carrying its machinery forward. |
| Speculative task preparation | Defer | Complexity must be justified by measured user benefit after the core task loop is stable. |

## Coordination

Ops Console ticket intake is an approved addition under ADR 0060, currently in
development. Explicit app scope and atomic draft/provenance persistence are
implemented in an isolated worktree with retry, restart, concurrency and rollback
tests. A separate scoped MCP endpoint and bounded progress/deployment projection
are implemented in isolation. Console outbox delivery and production provisioning
remain pending; no runtime integration is enabled yet.

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Agent-facing MCP coordination | Redesign | Implemented task and decision foundation: scoped per-worker credentials, role-specific discovery, shared application services, and a typed operator inbox. Add guarded Queen delivery rather than a broad legacy catalog. |
| Inter-worker messages | Redesign | Preserve findings, blockers, and directed handoffs; merge with decision/activity model where possible. |
| File ownership claims | Investigate | Modern agents and worktrees may reduce value; evaluate actual conflict prevention evidence. |
| Cross-project tasks | Investigate | Validate use before adding cross-workspace complexity. |
| Worker slash commands and injected skills | Redesign | Minimize; prefer stable MCP/application protocol where providers support it. |
| Provider-specific terminal scraping | Remove where possible | Replace with provider APIs, hooks, explicit events, or declared adapters. |

## Workflows and extensions

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Pipelines | Investigate | Potentially distinct product. Validate real use and whether task templates/automations cover the outcome. |
| Human/agent/automated pipeline steps | Investigate | Do not port engine before the pipeline product decision. |
| Shell-command service steps | Investigate | Security-sensitive; may belong in a constrained automation runner. |
| Webhook service steps | Investigate | Easy to reintroduce once a real journey requires them. |
| Headless-agent service steps | Merge | Likely part of orchestration rather than a generic service registry. |
| Testing harness | Redesign | Preserve reproducibility and controlled comparisons; separate product testing from runtime orchestration. |
| Tool-usage analytics | Redesign | Fold into unified observability rather than a special analysis subsystem. |
| Harness-improvement digest | Investigate | Preserve only if it drives regular operator decisions. |

## Integrations

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| GitHub | Keep/Redesign | Central coding workflow; use typed integration boundary and least-privilege credentials. |
| Jira | Redesign | Local dogfood slice implemented: operator OAuth, project pools, explicit workflow mapping and selective intake, linked task identity, safe reconciliation, and durable bounded outbound transitions with visible conflict/retry. Jira-backed Apiary groundwork now includes durable invitations, server-derived fail-closed join readiness, a Keeper-owned promoted-project catalog, an atomic private promotion command and Keeper UI, revision-bound operator acceptance, audited sole-Keeper collapse, durable Ed25519 node identities and signed connection cards, signed one-time invitation issuance/import with digest-verified project manifests, exact-revision acknowledgement with per-project preflight, and an authenticated Keeper endpoint that atomically consumes a signed independent-Hive submission and returns one retry-stable signed membership receipt plus bounded node credential. The invited Hive now completes that handshake through one explicit member-owned outbound request, verifies and atomically applies the receipt/credential as a durable Member, and can safely retry the same private submission after a temporary Keeper outage. Both sides expose a public-identity-only membership roster. A joined node automatically polls and verifies short-lived Keeper catalog snapshots plus a bounded ordered Swarm-task event feed, applies task snapshots and its cursor atomically, records content-free health, retries temporary outages with durable backoff, and halts authentication or protocol failures for operator action. Exact page retries are idempotent and gaps fail closed. Jira issue synchronization remains direct from each Hive to Jira; no Jira issue content or credential enters the Keeper task feed. Member-local convergence derives explicit catalog freshness, policy drift, Jira connection, project access, workflow readiness, and Keeper-task projection evidence. Keeper serializes shared Jira issue claims across authenticated member Hives with bounded reservations, idempotent confirmation after Jira acknowledges assignment, explicit pre-confirmation release, expiry recovery, durable confirmed home-Hive ownership, and a low-noise read-only ownership rollup that excludes expired attempts and private node material. Member Hives now journal each shared Jira claim before side effects, then durably reserve at Keeper, assign through their own Jira identity, confirm at Keeper, and import locally; exact retries are idempotent, temporary outages back off without Jira writes, and conflicts require attention. Confirmed Jira-claim handoff and governed Keeper-canonical Swarm task creation, Hive routing, Member claim/change submission, and idempotent private-worker materialization are implemented. Ordered local-to-Keeper lifecycle mirroring now advances linked private tasks through one durable desired state and one canonical revision at a time. The broader Native Apiary distributed task backend remains staged work. |
| Outlook/email import | Redesign | Frequent closed-loop daily-driver intake. One linked account exposes a bounded Inbox picker; explicit import atomically creates one idempotent local task from one message or up to 20 related messages while preserving every readable body, embedded image, supported attachment, source identity, and original-thread link. After Completed plus recorded deployment, the operator reviews one non-technical resolution and explicitly sends it through a durable per-thread outbox. Every source has independent delivery, retry, and uncertainty evidence, so retrying one ambiguous Outlook result cannot duplicate already confirmed replies. Credentials stay adapter-private; intermediate task state never mutates the mailbox. |
| Cloudflare Tunnel | Redesign | Remote access is valuable; treat tunnel choice as deployment adapter, not core domain. |
| Browser notifications | Keep | Implemented as a bounded, durable Web Push adapter for Needs you: presence-gated, generic encrypted payloads, explicit opt-in, configurable policy, and an eagerly refreshed current brand icon. |
| PWA installation, push, and mobile terminal | Redesign | Android Chrome/Edge is a first-class dogfood surface. Push-only service worker and install manifest are implemented; preserve terminal commands alongside long-form voice input and continue rendered mobile acceptance. |
| In-app feedback | Keep/Redesign | Critical to dogfooding; capture correlated diagnostics, bounded content-free browser failure markers, and privacy-safe context. Chat remains the richer path for screenshots until outbound attachment transport exists. |

Stewardship checkpoint: Keeper can atomically create, replace, list, and
audit-preservingly revoke an explicit grant over selected Member Hives and
capabilities. Each Member now polls a separate credential-bound snapshot of
only her own authority, atomically replaces its local projection, and treats an
empty snapshot as explicit revocation. The responsive Keeper and Member UIs
expose only public member identity; **My Stewardship** is visually distinct.
The first guarded remote action is implemented: a Steward with explicit
**Assign** authority can queue one Keeper-canonical Swarm task for a managed
Hive. The Member journals the command before network I/O; Keeper authenticates
the exact node and operator, rechecks the current grant and Hive scope, creates
the task atomically, stores a retry-stable receipt, and audits rejections. The
target Hive privately chooses its worker and repository. **Observe** now adds a
Keeper-derived, content-free shared-work pulse for each managed Hive: counts of
Ready, Active, Blocked, Review, and active Jira ownership plus the last shared
change. It never reports worker, repository, terminal, transcript, local-task,
or credential data. **Assist** now provides a separate durable request/response
loop: the Steward offers bounded help, Keeper rechecks scope, the target Hive
polls it outward, and its operator accepts or declines without any terminal
injection or engagement interruption. The Steward sees the resulting status on
her next poll. Takeover is now specified as an outbound-only, two-phase,
exclusive lease over the target Hive's Queen; it remains unavailable until the
lease, live relay, owner reclaim, automation pause, audit, and responsive
visibility ship together. Member and project actions remain staged until each
receives its own bounded command and conflict rules.

Keeper control-room checkpoint: a federated Keeper receives a first-class,
read-only Apiary surface outside Settings. It summarizes registered membership,
promoted Jira projects, active reservations or durable home-Hive ownership, and
Steward scopes from the existing private contracts. Routine worker activity
remains inside each Hive; invitations and configuration stay in Settings.

Member control-room checkpoint: a joined Hive receives the same first-class
Apiary navigation without being shown Keeper administration. Its read-only
surface identifies the Keeper, local Hive and operator, catalog convergence,
per-project readiness, synchronization health, blockers, and only shared work
whose durable home is this Hive. A synchronized Steward additionally sees only
her own managed Hives and granted capability names. With **Assign**, she can
route an outcome through Keeper and see queued or rejected delivery state
without seeing remote workers, repositories, terminals, or credentials.
Keeper sees bounded recent accepted and declined Steward routing, and an
accepted shared task names its public routing Steward. This audit view still
contains no remote worker, repository, terminal, provider session, or node
credential.
Browser acceptance uses route-local public fixture data so desktop and Android
layouts can be proved without changing the dogfood Keeper, Jira, membership,
or federation credentials.

Member departure checkpoint: a joined Hive can explicitly return to Personal
Hive mode only after Member-local outboxes and Keeper-owned shared authority are
clear. The Member freezes new shared mutations before its one outbound request;
the Keeper atomically rechecks ownership, stores a signed retry-stable receipt,
and ends the exact membership. A lost response leaves a visible, reload-safe
paused state and retries the same operation. Private workers, repositories,
provider conversations, settings, tasks, and Hive integrations remain local;
Apiary Jira bindings become Hive-owned and shared projections are removed only
after receipt verification.

## Platform and administration

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| SQLite persistence | Keep/Redesign | Embedded source of truth with one owner, transactional migrations, backups, and integrity checks. The latest migration and declared schema ceiling now share one named version, and a structural immediately-previous-schema test prevents an unreachable compiled step. |
| Legacy migration | Redesign | Preview-first, versioned migration packages keep Legacy read-only during import. Open Legacy tasks exclude Jira and closed work; selected repository workers import sleeping with an explicit choice to resume exact provider conversations or start fresh. A second opt-in can replace the conversation on an already-configured matching worker, but only while she is sleeping; the prior Next conversation is retained for untouched rollback. The commit is atomic and provenanced, starts no workers, and remains reversible only while the batch is untouched. Exact Claude resume stages the matching local provider transcript into Swarm's isolated Claude profile without parsing or exposing its content; first wake repairs earlier imports and fails closed rather than silently starting fresh. A separate backed-up receipt finalization leaves transferred Legacy tasks visible but read-only rather than completed; no dual write is allowed. |
| Configuration UI | Redesign | Human-oriented settings grouped by outcome; durable workers can be created, renamed, assigned an always-active policy, and ordered without path entry. Local Hive names and Keeper-owned Apiary names are editable public labels without changing durable identity, ownership, membership, or signing keys. The control room shows the current Hive plus Personal, Keeper, or Member context from that same private identity snapshot on desktop and mobile. No competing YAML/DB precedence after import. |
| CLI | Redesign | Installation, service, diagnostics, import/export, and automation only; normal operation remains web-first. |
| Self-update and restart | Redesign | Atomic update, compatibility check, worker preservation, health verification, and rollback. Development Settings refreshes working-copy detection while it remains open, so a newly pulled App/API revision becomes actionable without restarting the page; activation remains an explicit worker-preserving action. |
| Health/readiness/resource diagnostics | Keep/Redesign | First-class subsystem health and correlated traces from day one. |
| Authentication/password/passkeys | Redesign | Threat-model local, tunnel, and remote modes separately; least privilege by default. |
| OAuth provider for MCP | Investigate | Implement only for confirmed remote MCP journeys. |
| Global search | Keep/Redesign | Likely valuable across tasks, workers, messages, and decisions; validate scope. |
| OpenAPI documentation | Keep | Generate from accepted Rust contracts; also generate the React client. |

## Delivery tiers

The implementation sequence is:

1. Walking skeleton: terminal continuity, worker lifecycle, minimal task loop,
   local auth, persistence, diagnostics, update/recovery proof.
2. First core expansion: second provider, small coordination protocol, search,
   feedback, resource policy, and GitHub integration.
3. Deferred evaluation: Queen, groups, integrations, mobile/remote operation,
   pipelines, playbooks, and experimental automation.

No `Investigate`, `Defer`, or compound decision becomes an implementation
requirement until dogfooding resolves it. See `10-m0-evidence-review.md` for the
evidence and recommended cut.
