# ADR 0030: Member-polled Stewardship projection

Status: **Accepted**

## Context

Keeper already persists explicit Steward grants, but a Member Hive cannot act
on or present authority it has never received. Adding the grant to the signed
project catalog would change that catalog's signature contract and make rolling
upgrades brittle. Leaving a revoked grant cached locally would be worse: stale
authority must never survive a successful reconciliation.

## Decision

Each joined Member polls one separate, authenticated Stewardship endpoint after
it accepts the Keeper catalog. The response is bound to the calling node and
operator and contains only that operator's current grant: Apiary identity,
managed Hive identities, capability names, protocol versions, and generation
time. It contains no Jira data, workers, repositories, terminals, tasks,
credentials, or provider sessions.

The Member validates the exact Apiary, node, and operator binding and replaces
one local projection atomically. A response with no grant is an explicit
revocation and replaces any older projection. Exact retries are idempotent.
Invalid, oversized, foreign, or unsupported responses halt federation
reconciliation as incompatible; temporary network failures use the existing
durable retry state. Mixed-version operation therefore fails closed instead of
silently retaining old authority.

The Member control room presents a distinct **My Stewardship** panel only when
a confirmed grant exists. It names the Hives and capabilities in scope, while
stating that remote actions remain unavailable until each guarded command is
implemented. Presentation never implies authorization by itself.

## Consequences

- Keeper remains the single source of Steward authority.
- All traffic remains Member initiated; Keeper needs no route into a Member.
- Revocation converges through the same bounded poll as granting.
- The project catalog signature stays compatible with its existing contract.
- Future remote actions must authorize against the synchronized grant and add
  their own deterministic command, audit, and conflict rules.

## Validation

Persistence tests cover grant, exact retry, identity tampering, and revocation.
Transport and API tests cover credential-bound delivery and content exclusion.
Member UI and browser acceptance cover distinct desktop and Android
presentation without horizontal overflow.
