# 0036: Steward takeover is an outbound Queen lease

## Status

Accepted for staged implementation. The capability remains unavailable until
the control lease, relay, owner reclaim, and visible audit ship together.

Implementation checkpoint (2026-08-16): the internal two-phase lease store and
terminal-host authority primitive are implemented and tested. No operator or
federation route exposes them yet. Outbound relay, restart reconciliation,
automation recovery, audit presentation, and desktop/mobile control remain the
release gate.

Checkpoint (2026-09-22): Keeper may now take over on its own authority, and the
relay carries both directions. The reclaim rule below was amended after a proven
defect.

The relay is the SAME bounded, memory-only fan-out ADR 0107 built for watching,
generalised over its key rather than copied — the "frames pass through and are
never kept" property has one home, because two copies would be two places for it
to stop being true and only one of them would get the next fix. The input path
is the whole difference between takeover and watching, and it runs on a separate
channel from the screen so that delivering a keystroke to a watcher, or a screen
frame to a keyboard, is unrepresentable rather than merely unlikely.

One Hive-to-Hive relay endpoint is registered and is INERT: it demands an active
acknowledged lease, and no operator surface exists to create one. The gate is
about the capability being usable, not about whether a federation endpoint
answers 403.

Restart reconciliation now runs at boot. The asymmetry it exists for: Queen
automation is held down by the DURABLE lease row, while the authority that makes
a takeover real lives in the terminal host's MEMORY. Restart the host and the
row outlives the authority, leaving the Hive paused on behalf of a takeover that
is not happening — visible only as a Hive that has stopped working, which is the
hardest failure here to diagnose. So a lease survives a restart only if a Queen
session is still running to be controlled through it; anything else is ended as
`Expired`, which is what releases automation.

Automation recovery is now a real gate rather than a sentence. Measured first:
automation resumed the instant the lease row closed, so a Steward could leave a
half-typed command in the terminal, release, and have Queen inject into it on
the next tick. The row closing WAS the resume, and "only after local
reconciliation" was implemented by nothing.

A takeover that ends now records a durable debt, derived from the closed lease
rows rather than hooked into each closing path — a takeover ends by release,
reclaim, expiry, revocation, departure or restart, and a hook on each is six
places to forget one. Queen automation stays paused while a debt stands. The
debt is durable because the process is not: a crash must not be a way to skip
the reconciliation.

⚠️ THE DEBT HAS TO BE PAYABLE OR IT IS ITSELF A WEDGE. It is settled by clearing
the terminal host's authority, at boot and on every federation pass, and ONLY
when the host answers — released, or nothing there to release. An unreachable
host leaves the debt standing and the next pass retries, because marking it paid
on a failed call would quietly restore the gap this closes.

Audit presentation is built: a Keeper-side reader and a panel showing who held
which Hive, on whose authority, why it started and why it ended. Both reasons
are carried because they are different claims — the lease's reason is why a
takeover began, the reclaim reason is the local operator's account of taking
their machine back, and the second is what matters when the question is whether
the first should have happened. Keeper's own authority reads as Keeper rather
than as a blank stewardship.

The audit carries exactly what this ADR permits — Apiary, source Hive, target
Hive, actor, reason, state, revision, timestamps — plus the reclaim reason,
which is the operator's own words about their own machine. It says WHO held a
Hive and never what they did while holding it; that second part is why frames
are not stored at all.

Outstanding, and the refusal to ship a partial takeover behind a flag stands:
desktop/mobile control, which is also the only remaining reason the capability
is unreachable — no operator surface can start a takeover.

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
- **Reclaim is not fenced by the lease revision, on either side.** Renewal bumps
  the revision, and a Steward actively working renews constantly, so a fence
  means the busier the remote actor is the more reliably the person at the
  keyboard is refused — at exactly the moment they reached for it. The local
  projection also only advances when the target next polls Keeper, so the
  revision on screen is already stale immediately after acknowledgement.
  Measured 2026-09-21: one ordinary renewal between projection and reclaim was
  enough to reject the local operator. Everything else about reclaim is
  unchanged — the actor must still be the target Hive and the lease must still
  be active. Amended from the 2026-09-21 interview, which offered an
  unreclaimable Keeper lease and a Keeper-settable lock and had both declined.
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
