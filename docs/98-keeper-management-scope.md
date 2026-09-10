# Keeper management and timely Hive coordination

Status: **Keeper management direction approved; capability details in design**

## Requested outcome

Once a Hive joins, Keeper should manage substantially more of its shared setup
and coordination. Changes should reach members promptly without repeatedly
walking developers through settings. This extends, rather than replaces,
ADR0097 enrollment work. It is not approval for unrestricted remote control.

## Proposed authority model

| Area | Recommended default on joining | Explicitly outside this default |
|---|---|---|
| Shared configuration | Apiary catalog, shared workflow configuration, agreed policy and shared defaults | Private Hive overrides, filesystem roots, credentials, provider permissions |
| Work coordination | Eligible assignments, shared task priorities, structured progress and blockers | Access to projects the member cannot access in Jira |
| Recovery | Inspect typed failures and request a supported recovery | Interrupt active work, restart workers, run arbitrary machine commands |
| Visibility | Shared readiness, connection freshness, pending changes and acknowledgements | Private transcripts, tokens, private tasks or repository contents |
| Terminal intervention | Existing explicitly scoped takeover paths only | Membership itself granting terminal control |

The operator approved joining as consent to Apiary-wide Keeper management,
while retaining the local task system. Swarm shared tasks are the minimum setup;
Jira is optional, including no Jira projects at all. The table above remains a
capability-design proposal, not an unlimited shell/terminal authority grant.
Existing explicit grants remain authoritative until capability changes are
specified, implemented, audited and tested.
Subsequent questions should address concrete operations rather than an undefined
"manage everything" switch, one question at a time with a recommendation.

## Connection recommendation

Use one outbound, authenticated, persistent notification connection per Hive
when supported. It should announce durable changes, not carry arbitrary remote
commands or establish authority. Reuse the existing outbound reconciliation
owner to fetch and apply the referenced changes. Do not add one connection per
worker or per browser. The member needs no inbound port.

Transport selection (SSE, WebSocket, or long polling) remains an implementation
decision pending an assessment of existing runtime ownership and deployment
proxies. Prefer the simplest server-to-member notification transport if no
bidirectional stream is needed. Measure actual update latency and resource use
before choosing a more complex transport. There is no persistent transport
implemented or accepted by this proposal alone.

Notification loss is recoverable through a durable cursor and bounded snapshot
resynchronization. Queue ownership, caps, timeouts, retry backoff and shutdown
must be explicit. A slow member cannot hold Keeper resources indefinitely.
Use revisioned desired state for replaceable settings, not an unbounded history
of obsolete commands. Mutating actions retain exact idempotency keys and final
receipts. Reconnecting never blindly executes expired or superseded work.

## Operator-facing management

Keeper sees each Hive's applied configuration revision and exceptions, not just
"online". Distinguish queued, received, applied, declined, incompatible, and
failed. A heartbeat or successful send is not proof of application.

Hive operators see which settings are managed, by whom, their effective values,
and any local action required. Keep technical receipts collapsed. Ordinary
successful changes should not become Needs You items. Actionable gaps identify
the responsible person and a concise next step. A project added after joining
must neither invalidate unrelated access nor require rejoining.

Policy or permission expansion requires renewed scoped consent. Removing a
grant prevents future execution, including queued actions; an action already
executed remains audited. Departure stops shared management without deleting
private settings or work. Stale commands from an old membership cannot apply
after rejoining, key rotation, or a change of Apiary.

## Acceptance matrix

- Two independent Hives, including overlapping and disjoint Jira access.
- New shared project/configuration arrives without manual recreation or rejoin.
- Offline member continues private work and converges once reachable.
- Dropped notification and lost acknowledgement do not duplicate side effects.
- Superseded settings converge to the current revision, not every intermediate one.
- Unsupported member version shows a precise compatibility action and preserves
  permitted local work; no silent downgrade of authority checks.
- Revoked grant, departed Hive, expired credential and replay all fail closed.
- Active terminal engagement survives routine shared configuration changes.
- Keeper sees actual applied/failed results; members can explain managed settings.
- Bounded resource use with many idle Hives and a slow/offline member.

## Delivery boundary

Continue already-approved enrollment and project-readiness work while scoping
this extension. Do not block that work on persistent transport or turn transport
refactoring into its prerequisite. No BFG Admin coordination is needed or
authorized here. No release is authorized by this document.
