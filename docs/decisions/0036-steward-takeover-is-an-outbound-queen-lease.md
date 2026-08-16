# 0036: Steward takeover is an outbound Queen lease

## Status

Accepted for staged implementation. The capability remains unavailable until
the control lease, relay, owner reclaim, and visible audit ship together.

Implementation checkpoint (2026-08-16): the internal two-phase lease store and
terminal-host authority primitive are implemented and tested. No operator or
federation route exposes them yet. Outbound relay, restart reconciliation,
automation recovery, audit presentation, and desktop/mobile control remain the
release gate.

## Context

A Steward may need to step into a managed Hive when its operator is unavailable
or a cross-Hive decision is blocked. Member computers are not required to
accept inbound connections, Keeper does not own their workers, and ordinary
worker or terminal data remains private to each Hive. A label that merely says
“takeover” without a real exclusive control boundary would be unsafe and
misleading.

## Decision

Takeover controls the managed Hive's always-active **Queen**, not an arbitrary
private worker. The Queen is the Hive-level coordination surface and can route
work to her own workers without exposing their identities or repositories to a
Steward.

Every connection remains outbound to the reachable Keeper:

1. The Steward's Hive journals a reasoned takeover command before network I/O.
2. Keeper authenticates that Member, rechecks the exact unrevoked **Take over**
   grant and managed Hive, and creates one bounded requested lease.
3. The target Hive retrieves the request on its existing outbound connection,
   installs the exclusive local Queen lease, pauses competing automation, and
   acknowledges the exact lease revision.
4. Only after that acknowledgement does Keeper mark the lease active and relay
   bounded Queen terminal frames between the two outbound Member connections.

Keeper is the authority and relay, but it does not persist terminal frames or
turn them into an Apiary transcript. The target Hive keeps its ordinary private,
bounded terminal history. Public audit data contains only Apiary, source Hive,
target Hive, actor, reason, state, revision, and timestamps.

## Lease and reclaim rules

- At most one takeover lease exists for a target Hive. A concurrent request
  conflicts rather than silently replacing another Steward.
- The active lease lasts at most five minutes and renews only while the Steward
  supplies authenticated input. Lost connections expire safely without a
  background truth owner.
- Activation visibly replaces the local operator engagement lease for Queen.
  It does not start Queen, select a private worker, or inject a queued message.
- The target operator can reclaim Queen immediately from any authenticated
  local input surface. Reclaim closes the relay, records the reason, and gives
  the local operator a fresh engagement lease.
- Keeper revocation, Stewardship revocation, membership departure, target
  restart without a matching durable lease, expiry, or credential failure ends
  takeover and resumes normal automation only after local reconciliation.
- A requested lease that the target has not acknowledged grants no terminal
  visibility or input authority.

## Rolling compatibility

Takeover fails closed unless Keeper, Steward Hive, target Hive, and terminal
host all advertise the takeover relay protocol. Older nodes may continue all
other Apiary work. The existing capability name in a Stewardship is authority
to attempt the future command, not evidence that a control channel exists.

## Consequences

The design preserves one-operator Hives and the outbound-only network model.
It also makes the implementation materially larger than Observe or Assist: the
control-plane lease, target acknowledgement, memory-bounded live relay, owner
reclaim, automation pause, audit, and desktop/mobile visibility are one safety
unit and must not be released as disconnected partial features.
