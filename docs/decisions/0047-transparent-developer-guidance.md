# ADR 0047: Transparent developer guidance

Status: **Accepted product contract; persistence and UI intentionally staged**

## Context

Legacy Swarm experimented with learning miners and Dreamer-style behavior so a
Hive could stop repeating the same operator corrections. The useful outcome is
personal adaptation. The unsafe outcome is hidden policy: an inferred rule can
quietly change Queen's behavior, expand authority, leak one repository's
conventions into another, or spread one developer's preference across an
Apiary.

Ring 1 review retained the outcome but rejected implicit learning. Queen may
notice repetition and propose guidance; only the operator may make it active.
The complete active ruleset must be visible and manageable in Settings.

## Decision

Swarm Next will implement **Developer guidance** as a private, revisioned Hive
record with an explicit lifecycle:

1. **Proposed** — Queen has suggested the narrowest useful rule and cited the
   bounded evidence that motivated it. It has no behavioral effect.
2. **Active** — the operator reviewed and enabled the exact revision. Queen may
   use it as guidance within its recorded scope.
3. **Disabled** — the record and provenance remain visible, but Queen must not
   use it.
4. **Retired** — removed from the everyday ruleset while an audit tombstone
   prevents the same rejected revision from silently returning.

Every record contains:

- one local Hive owner;
- a scope of Hive, worker, or repository;
- the concise guidance text;
- content-bounded provenance that identifies the correction or durable work
  evidence without copying terminal transcripts;
- the proposing Queen identity;
- an immutable revision history; and
- creation, review, activation, disablement, and retirement times as applicable.

The first implementation will not mine raw terminal text. Queen can propose a
rule only from facts already visible through typed Swarm tools, such as repeated
task outcomes, operator decisions, or explicit corrections. A proposal must say
what would change and why its chosen scope is the narrowest safe scope.

## Authority boundary

Developer guidance is advice, not permission.

- It cannot increase the current Queen autonomy ceiling.
- It cannot authorize Jira changes, email, deployment, purchases, credentials,
  Apiary actions, or any other external effect.
- It cannot override repository ownership, task lifecycle invariants, provider
  permissions, operator engagement, or a more specific durable decision.
- It remains private to one developer's Hive and is never promoted to Apiary
  policy merely because another Hive behaves similarly.
- Queen may read only Active guidance. Proposed, Disabled, and Retired records
  are visible to the operator but inert.

The agent bridge will expose one narrow proposal command to Queen and a bounded
read of Active guidance. Operator APIs own editing and lifecycle changes. There
is no worker command for creating or activating guidance.

## Routines remain separate

A repeated multi-step journey may eventually become a Queen-proposed Routine,
but a Routine is not a larger guidance string. It needs typed steps, triggers,
approval points, external-effect declarations, cancellation, retry limits, and
failure behavior. Both use the same transparent proposal principle; they do not
share an executable free-form rule engine.

## Delivery sequence

1. Add the private revisioned persistence model and validation without changing
   Queen behavior.
2. Add Settings review, edit, enable, disable, and retire controls with an empty
   state that does not imply learning is already active.
3. Add Queen proposal and Active-guidance read tools, with tool-level proof that
   neither can grant authority.
4. Dogfood proposals manually before considering any automatic repetition
   detector.
5. Design the typed Routine vocabulary only after real proposals demonstrate
   repeated journeys that tasks plus guidance cannot express.

## Consequences

- Developers can teach their Hive without trusting an invisible personalization
  layer.
- Incorrect learning is reversible and inspectable.
- The first useful slice is intentionally less magical: no rule takes effect
  until the operator approves it.
- This adds a durable schema and worker-engine compatibility boundary, so it
  should ship in an explicitly scheduled engine release rather than hitchhiking
  on an unrelated App/API polish deployment.

