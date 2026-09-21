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

## ⚠️ Amendment 2026-09-21: the authority table's visibility row is superseded

The **Visibility** row above puts "private transcripts, tokens, private tasks or
repository contents" outside the Keeper default, and the **Terminal
intervention** row puts "membership itself granting terminal control" outside
it. **Both are SUPERSEDED.** The rest of the table stands.

Operator decision: Keeper gets "literally everything, basically a window into
that hive, either watching or being able to do a takeover". A Steward gets the
same window over the Hives in their exact granted scope, because the grant
decides WHICH Hives, never HOW MUCH.

### Revised rows

| Area | Now | Still outside |
|---|---|---|
| Visibility | A full live window into any Hive in granted scope — tasks, terminal, state | Stored transcripts; credentials and tokens |
| Terminal intervention | Watching within scope; takeover per ADR 0036, extended to Keeper | Holding a Hive against its operator; arbitrary machine commands |

### The four properties that make this safe enough to state

Stated here in full rather than by reference, because this table is what an
implementer reads:

1. **Live only, never stored.** Keeper relays frames and never persists them or
   builds an Apiary transcript — the rule ADR 0036 already sets for takeover.
2. **Always visible while watching**, naming who is watching. No reason is
   required, so the audit answers who and when but never why.
3. **Instant reclaim, including against Keeper**, from any authenticated local
   surface. Keeper may take over again; the operator at the machine is the only
   one who knows whether they are mid-deploy or mid-incident.
4. **Credentials, filesystem roots and provider permissions stay out**, exactly
   as the Shared configuration row already says.

### ⚠️ This amendment breaks one of this document's own rules, knowingly

This document states: **"Policy or permission expansion requires renewed scoped
consent."** Upgrading the existing **Observe** grant in place is exactly a
permission expansion without renewed consent — whoever holds Observe today gains
terminal visibility having agreed only to four counts.

The operator's premise was "there is only one apiary out there, mine, so we
don't need to worry about this happening in place". Checked against the live
store on 2026-09-21, that is only mostly true: one Apiary, but ONE live
stewardship carrying FOUR capability grants across EIGHT memberships. So the
expansion is not zero-impact — it widens exactly one Steward's authority.

The decision stands; it was made with this in front of the operator. **The
affected Steward should be told rather than discover it.** If a second Apiary or
an outside member ever exists, the renewed-consent rule above applies again in
full and this exception does not carry.

### What is NOT superseded, and must not be widened by inference

- **Credentials, filesystem roots and provider permissions.** Still outside,
  exactly as the Shared configuration row says. Watching a Hive is one thing;
  writing its secrets or rewriting its execution permissions is another, and
  nothing in the interview asked for it.
- **Work coordination and Recovery rows.** Unchanged. Keeper still cannot
  interrupt active work, restart workers, or run arbitrary machine commands.
- **Removing a grant still prevents future execution**, including queued
  actions, and an executed action remains audited.
- **Departure still stops shared management** without deleting private settings
  or work.

### Transport, also superseded

This document instructs measuring latency and resource use before choosing a
more complex transport, and says no persistent transport is accepted by the
proposal alone. The operator has chosen **one outbound WebSocket per Hive** now,
on the grounds that takeover relay needs bidirectional frames regardless. That
is a deliberate override: if polling would have sufficed, this is where the
extra complexity entered.

The constraint it does NOT override: the connection "should announce durable
changes, not carry arbitrary remote commands or establish authority". Durable
state keeps arriving through the existing polled, cursored feed; the socket is a
doorbell. Live terminal frames are the one exception that must ride the stream.

Design record: `docs/specs/apiary-controls-scope.md`.
