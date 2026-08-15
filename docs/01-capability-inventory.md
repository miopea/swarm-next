# Capability inventory

Status: **M0 draft recommendation**

This inventory is intentionally organized around outcomes rather than legacy
modules. Decisions are provisional until reviewed with the primary operator.

Decision meanings:

- **Keep**: preserve the outcome with minimal product change.
- **Redesign**: preserve the value through a new model or interaction.
- **Merge**: absorb the outcome into a clearer capability.
- **Remove**: do not implement in Swarm Next.
- **Investigate**: usage or value needs more evidence.

## Product center

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Persistent agent terminals | Redesign | Core value. Server-owned terminal sessions with sequence-based attach, bounded history, explicit color capabilities, desktop paste, and private bounded image attachments. |
| Worker launch and lifecycle | Redesign | Core value. Immutable worker-session identity plus separate durable provider-conversation identity; durable named repository roster, bounded recursive path completion, direct canonical path entry, explicit ordering, lifecycle, and recovery. |
| Multi-worker control room | Redesign | Core value. React workspace with instant switching and stable retained sessions. |
| Task board | Redesign | Preserve work management while simplifying task types, transitions, and presentation. |
| Task assignment | Redesign | Preserve manual and assisted assignment; separate recommendations from execution policy. |
| Task history and audit | Keep | Implemented as bounded, durable per-task events plus a quiet operator Activity view with task search and progress, assignment, and change filters. It excludes terminal output and transport noise. |
| Direct terminal input | Keep | Essential, with explicit input ownership and stale-session protection. |
| Groups and bulk worker actions | Investigate | Likely useful, but validate actual use and whether workspace selection replaces groups. |
| Worker memory/context notes | Investigate | Preserve only if distinct from provider memory and project instructions. |

## Automation and orchestration

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Routine approval drones | Remove | Provider-native permissions own provider tool approval. Keep only typed Swarm-level operator decisions. |
| Worker activity and attention | Redesign | Implemented from the bounded host-owned terminal surface: Sleeping is unloaded, Resting is live and idle, Buzzing is active or conservatively unknown, and operator decisions are explicit. Provider classifiers and content-free state-change events keep the roster authoritative without browser polling. |
| Crash/revival automation | Redesign | Becomes worker lifecycle recovery, not a drone. |
| Host pressure management | Redesign | Observation-first API and terminal-host memory evidence now has explicit thresholds and operator visibility; automated recovery waits for soak evidence and a safe target. |
| Context-pressure handling | Investigate | Reassess against current provider compaction and context-management capabilities. |
| Completion verification | Redesign | Preserve as an optional verification policy; deterministic checks before model review. |
| Queen task assignment | Redesign | Preserve assisted coordination but clarify recommendation, authorization, and execution. |
| Queen completion detection | Redesign | Prefer explicit agent/task protocol signals; retain inference only as a fallback. |
| Queen interactive conversation | Investigate | Validate frequency and whether it belongs in the unified operator conversation surface. |
| Proposal approval system | Merge | One generalized operator-decision inbox rather than feature-specific proposal surfaces. |
| Approval-rule regex engine | Remove | Do not infer authorization from rendered terminal text or maintain a second permission authority. |
| Standing improvement loops | Investigate | High resource and safety implications; require demonstrated ongoing value and hard budgets. |
| Playbook synthesis | Investigate | Potentially valuable, but assess actual retrieval/use before carrying its machinery forward. |
| Speculative task preparation | Defer | Complexity must be justified by measured user benefit after the core task loop is stable. |

## Coordination

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
| Jira | Redesign | Local dogfood slice implemented: operator OAuth, project pools, explicit workflow mapping and selective intake, linked task identity, safe reconciliation, and durable bounded outbound transitions with visible conflict/retry. Jira-backed Apiary groundwork now includes durable invitations, server-derived fail-closed join readiness, a Keeper-owned promoted-project catalog, an atomic private promotion command and Keeper UI, revision-bound operator acceptance, audited sole-Keeper collapse, durable Ed25519 node identities and signed connection cards, signed one-time invitation issuance/import with digest-verified project manifests, exact-revision acknowledgement with per-project preflight, and an authenticated Keeper endpoint that atomically consumes a signed independent-Hive submission and returns one retry-stable signed membership receipt plus bounded node credential. The invited Hive now completes that handshake through one explicit member-owned outbound request, verifies and atomically applies the receipt/credential as a durable Member, and can safely retry the same private submission after a temporary Keeper outage. Both sides expose a public-identity-only membership roster. A joined node automatically polls and verifies short-lived Keeper catalog snapshots plus a bounded ordered Swarm-task event feed, applies task snapshots and its cursor atomically, records content-free health, retries temporary outages with durable backoff, and halts authentication or protocol failures for operator action. Exact page retries are idempotent and gaps fail closed. Jira issue synchronization remains direct from each Hive to Jira; no Jira issue content or credential enters the Keeper task feed. Member-local convergence derives explicit catalog freshness, policy drift, Jira connection, project access, workflow readiness, and Keeper-task projection evidence. Keeper serializes shared Jira issue claims across authenticated member Hives with bounded reservations, idempotent confirmation after Jira acknowledges assignment, explicit pre-confirmation release, expiry recovery, durable confirmed home-Hive ownership, and a low-noise read-only ownership rollup that excludes expired attempts and private node material. Member-side Jira claim orchestration, governed Swarm-task ownership/change submission, offline outbound queues, and confirmed-claim handoff remain staged work. |
| Outlook/email import | Redesign | Frequent closed-loop daily-driver intake. One linked account exposes a bounded Inbox picker; explicit import atomically creates one idempotent local task from one message or up to 20 related messages while preserving every readable body, embedded image, supported attachment, source identity, and original-thread link. After Completed plus recorded deployment, the operator reviews one non-technical resolution and explicitly sends it through a durable per-thread outbox. Every source has independent delivery, retry, and uncertainty evidence, so retrying one ambiguous Outlook result cannot duplicate already confirmed replies. Credentials stay adapter-private; intermediate task state never mutates the mailbox. |
| Cloudflare Tunnel | Redesign | Remote access is valuable; treat tunnel choice as deployment adapter, not core domain. |
| Browser notifications | Keep | Implemented as a bounded, durable Web Push adapter for Needs you: presence-gated, generic encrypted payloads, explicit opt-in, configurable policy, and an eagerly refreshed current brand icon. |
| PWA installation, push, and mobile terminal | Redesign | Android Chrome/Edge is a first-class dogfood surface. Push-only service worker and install manifest are implemented; preserve terminal commands alongside long-form voice input and continue rendered mobile acceptance. |
| In-app feedback | Keep/Redesign | Critical to dogfooding; capture correlated diagnostics, bounded content-free browser failure markers, and privacy-safe context. Chat remains the richer path for screenshots until outbound attachment transport exists. |

Stewardship checkpoint: Keeper can atomically create, replace, list, and
audit-preservingly revoke an explicit grant over selected Member Hives and
capabilities. The responsive Keeper UI exposes only public member identity and
states that remote delivery still depends on federation synchronization.
Enforcement of Steward-scoped remote actions remains staged work.

Keeper control-room checkpoint: a federated Keeper receives a first-class,
read-only Apiary surface outside Settings. It summarizes registered membership,
promoted Jira projects, active reservations or durable home-Hive ownership, and
Steward scopes from the existing private contracts. Routine worker activity
remains inside each Hive; invitations and configuration stay in Settings.

Member control-room checkpoint: a joined Hive receives the same first-class
Apiary navigation without being shown Keeper administration. Its read-only
surface identifies the Keeper, local Hive and operator, catalog convergence,
per-project readiness, synchronization health, blockers, and only shared work
whose durable home is this Hive. Browser acceptance uses route-local public
fixture data so desktop and Android layouts can be proved without changing the
dogfood Keeper, Jira, membership, or federation credentials.

## Platform and administration

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| SQLite persistence | Keep/Redesign | Embedded source of truth with one owner, transactional migrations, backups, and integrity checks. |
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
