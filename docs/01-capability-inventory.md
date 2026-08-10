# Capability inventory

Status: **Proposed initial assessment**

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
| Persistent agent terminals | Redesign | Core value. Server-owned terminal sessions with sequence-based attach and bounded history. |
| Worker launch and lifecycle | Redesign | Core value. Immutable worker-session identity; explicit lifecycle and recovery. |
| Multi-worker control room | Redesign | Core value. React workspace with instant switching and stable retained sessions. |
| Task board | Redesign | Preserve work management while simplifying task types, transitions, and presentation. |
| Task assignment | Redesign | Preserve manual and assisted assignment; separate recommendations from execution policy. |
| Task history and audit | Keep | Essential for trust, recovery, and diagnosis; implement as first-class domain events. |
| Direct terminal input | Keep | Essential, with explicit input ownership and stale-session protection. |
| Groups and bulk worker actions | Investigate | Likely useful, but validate actual use and whether workspace selection replaces groups. |
| Worker memory/context notes | Investigate | Preserve only if distinct from provider memory and project instructions. |

## Automation and orchestration

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Routine approval drones | Remove/Merge | Provider-native automatic approval now covers much of the original value. Retain only policy or audit outcomes not supplied natively. |
| Idle detection and nudging | Redesign | Valuable only where provider session status is authoritative enough; avoid screen-scraping heuristics where possible. |
| Crash/revival automation | Redesign | Becomes worker lifecycle recovery, not a drone. |
| Host pressure management | Redesign | Becomes platform resource policy with explicit thresholds, quotas, and operator visibility. |
| Context-pressure handling | Investigate | Reassess against current provider compaction and context-management capabilities. |
| Completion verification | Redesign | Preserve as an optional verification policy; deterministic checks before model review. |
| Queen task assignment | Redesign | Preserve assisted coordination but clarify recommendation, authorization, and execution. |
| Queen completion detection | Redesign | Prefer explicit agent/task protocol signals; retain inference only as a fallback. |
| Queen interactive conversation | Investigate | Validate frequency and whether it belongs in the unified operator conversation surface. |
| Proposal approval system | Merge | One generalized operator-decision inbox rather than feature-specific proposal surfaces. |
| Approval-rule regex engine | Remove/Investigate | Likely obsolete for provider prompts; retain only if a current non-provider policy use remains. |
| Standing improvement loops | Investigate | High resource and safety implications; require demonstrated ongoing value and hard budgets. |
| Playbook synthesis | Investigate | Potentially valuable, but assess actual retrieval/use before carrying its machinery forward. |
| Speculative task preparation | Remove/Investigate | Complexity must be justified by measured user benefit. |

## Coordination

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| Agent-facing MCP coordination | Redesign | Keep a small, coherent tool surface based on current agent-native coordination abilities. |
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
| Jira | Investigate | Preserve if used; simplify around links and explicit import/export rather than broad synchronization by default. |
| Outlook/email import | Investigate | Validate frequency and which parts—task creation, attachment parsing, draft replies—remain valuable. |
| Cloudflare Tunnel | Redesign | Remote access is valuable; treat tunnel choice as deployment adapter, not core domain. |
| Browser notifications | Keep | Useful attention channel; unify with in-app decisions and notification policy. |
| PWA installation and share target | Investigate | Evaluate mobile usage and platform reliability before committing. |
| In-app feedback | Keep/Redesign | Critical to dogfooding; capture correlated diagnostics and privacy-safe context. |

## Platform and administration

| Capability | Decision | Rationale and intended direction |
|---|---|---|
| SQLite persistence | Keep/Redesign | Embedded source of truth with one owner, transactional migrations, backups, and integrity checks. |
| Configuration UI | Redesign | Human-oriented settings grouped by outcome; no competing YAML/DB precedence after import. |
| CLI | Redesign | Installation, service, diagnostics, import/export, and automation only; normal operation remains web-first. |
| Self-update and restart | Redesign | Atomic update, compatibility check, worker preservation, health verification, and rollback. |
| Health/readiness/resource diagnostics | Keep/Redesign | First-class subsystem health and correlated traces from day one. |
| Authentication/password/passkeys | Redesign | Threat-model local, tunnel, and remote modes separately; least privilege by default. |
| OAuth provider for MCP | Investigate | Implement only for confirmed remote MCP journeys. |
| Global search | Keep/Redesign | Likely valuable across tasks, workers, messages, and decisions; validate scope. |
| OpenAPI documentation | Keep | Generate from accepted Rust contracts; also generate the React client. |

## Required review

The primary operator should review this inventory in several short passes:

1. Daily-driver essentials.
2. Automation and Queen behavior.
3. Coordination and MCP.
4. Integrations and remote/mobile use.
5. Experimental or rarely used features.

No `Investigate`, `Remove/Investigate`, or `Keep/Redesign` entry becomes an
implementation requirement until its decision is resolved.

