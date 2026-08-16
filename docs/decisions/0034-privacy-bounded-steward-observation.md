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
