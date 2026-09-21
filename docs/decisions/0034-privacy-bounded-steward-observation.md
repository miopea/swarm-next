# ADR 0034: Privacy-bounded Steward observation

Status: **Accepted**

## Context

Stewardship grants have always required **Observe**, but the synchronized scope
previously exposed only the names of managed Hives and capability labels. That
was insufficient for a Steward to know whether help was useful, while copying
remote worker state, terminal output, or repository details into Keeper would
violate the one-operator Hive boundary and recreate fleet noise.

## Decision

Keeper derives one bounded shared-work pulse for each Hive in the authenticated
Steward's exact managed scope. It contains only counts of Keeper-canonical Swarm
tasks in Ready, Active, Blocked, and Review; active Keeper-known Jira claims;
and the timestamp of the latest shared-work change. Names and task content are
not duplicated into the pulse. Member UI joins the Hive identity from the
existing public roster.

The pulse is an additive field in the existing credential-bound Stewardship
snapshot. Old Members ignore it and new Members accept an omitted field during
rolling updates. A non-empty pulse must contain exactly one unique record for
every managed Hive, all counts and timestamps are bounded, and a snapshot with
no current Stewardship cannot carry observations. The Member persists the pulse
with the same atomic projection as the authority that permits viewing it.

This is not live presence. It does not contain worker state, repositories,
local tasks, terminal or transcript data, provider sessions, Jira issue
content, integration configuration, or credentials. Assist and Take Over keep
their own future authorization, engagement, delivery, and audit contracts.

## Consequences

- A Steward can scan shared workload and blockers without opening another Hive.
- Keeper remains the only source; Members do not contact each other.
- Private execution stays local, so ordinary worker activity creates no Apiary
  traffic or Keeper memory pressure.
- A rolling deployment can temporarily show no pulse without breaking the
  existing Steward grant or guarded Assign action.

## Validation

API integration proves an authenticated Steward initially receives one empty
managed-Hive pulse and sees Ready increment after a guarded routing command.
Serialization and local projection validation reject foreign, duplicate,
oversized, future-dated, or authority-free observations. Member UI tests prove
the shared-work counts while asserting that private worker and terminal details
remain absent. Desktop and Android browser acceptance are required before
deployment.

## ⚠️ Amendment 2026-09-21: the pulse is no longer the boundary

**The counts-only bound above is SUPERSEDED, and so is the reason given for
it.** The Context argues that "copying remote worker state, terminal output, or
repository details into Keeper would violate the one-operator Hive boundary".
The operator has changed what is wanted: Keeper gets "literally everything,
basically a window into that hive, either watching or being able to do a
takeover". Read this section before acting on anything above it.

**Depth and breadth are now different things.** The grant decides WHICH Hives
you can see, never HOW MUCH. A Steward sees each Hive in their exact scope
exactly as Keeper would; Keeper additionally sees every other Hive. The
operator's own framing: Moleek stewarding Paul's Hive "would have line of site".

This ADR's Context already conceded the pulse was "insufficient for a Steward to
know whether help was useful". That is the complaint this amendment answers.

The **Observe** grant is upgraded in place rather than a new Watch grant being
added. ⚠️ THE STATED PREMISE — "there is only one apiary out there, mine" — IS
ONLY MOSTLY TRUE. Checked against the live store on 2026-09-21: one Apiary, but
one live stewardship carrying four capability grants across eight memberships.
The upgrade therefore widens exactly one Steward's authority, and whoever holds
it gains terminal visibility without having agreed to it. That Steward should be
told rather than discover it.

### Four properties keep this safe enough to state

Without all four this is standing surveillance of every member's machine.

1. **Live only, never stored.** Keeper relays frames and does not persist them
   or build an Apiary transcript — the rule ADR 0036 already sets for takeover
   relay. A compromised Keeper then exposes what is on one screen now, rather
   than months of every member's terminal history including whatever they typed.
   The member Hive keeps its own private bounded history.
2. **Always visible while watching.** The watched operator sees that it is
   happening, and sees WHO is watching. A reason is not required — which is
   deliberately asymmetric with takeover, so the audit can answer "who looked
   and when" and can never answer "why".
3. **Instant reclaim, including against Keeper.** Unchanged from ADR 0036 and
   extended to Keeper: the local operator reclaims immediately from any
   authenticated local surface, and Keeper may simply take over again.
4. **Credentials, filesystem roots and provider permissions stay OUT.** Doc 98
   excludes these and that exclusion is NOT superseded. Nothing in the interview
   asked for it. Do not widen by inference.

### What survives unchanged

Keeper remains the only route; Members still do not contact each other. Assist
and Take Over keep their own authorization, engagement, delivery and audit
contracts. The pulse itself is not deleted — it remains a cheap summary, and a
window is not a substitute for a scannable count across many Hives.

Design record: `docs/specs/apiary-controls-scope.md`.
