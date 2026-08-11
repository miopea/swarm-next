# Program charter

Status: **Proposed**

## Mission

Build the persistent agent workspace Swarm would have been if its current
product knowledge, provider capabilities, operating experience, and failure
history had been available on day one.

This is not a requirement to preserve every feature or behavior. It is a
requirement to preserve or improve the valuable user outcomes.

## Why this program exists

Legacy Swarm grew rapidly through direct operator-driven development. That
produced substantial value and unusually rich operational knowledge, but also
created overlapping state ownership, large mutable frontend lifecycles,
compatibility paths, polling systems, and automation that provider products
have since made less important.

The immediate symptom is terminal redraw and reconnect flakiness. The broader
concern is that continued feature work is increasingly constrained by the
current Python daemon and page-level JavaScript architecture.

## Desired end state

The operator experiences a single application in which:

- workers and their terminal sessions are persistent resources;
- browsers are disposable views over those resources;
- reload and update never threaten running work;
- task and worker states are unambiguous and auditable;
- provider-native capabilities are preferred over duplicated automation;
- performance and memory use remain bounded during multi-day operation;
- failures identify their subsystem and recover without ritual restarts;
- installation, update, rollback, and diagnostics are one coherent workflow.
- one-operator Hives can remain private or join an Apiary without moving their
  workers, repositories, credentials, or normal Queen workflow;
- Keeper and optional scoped Stewards receive useful oversight without routine
  developer activity becoming management noise.

## Program principles

1. **Outcomes over parity.** Legacy behavior is evidence, not authority.
2. **One change axis at a time.** Keep legacy Swarm operational while the new
   product grows through independently testable vertical slices.
3. **One owner per fact.** Terminal state, task state, configuration, and
   integration state each have one authoritative owner.
4. **No invisible infinity.** Buffers, queues, retries, histories, and tasks
   have explicit limits and overflow behavior.
5. **Recovery is a primary path.** Reload, disconnect, sleep, update, and crash
   are designed and tested, not treated as exceptions.
6. **Provider-native first.** Do not reproduce behavior now safely supplied by
   Claude, Codex, or other providers.
7. **AI accelerates implementation, not proof.** Generated code is held to the
   same architecture, review, testing, and soak requirements.
8. **Modular monolith first.** Avoid microservices until independent scaling or
   security requirements justify them.
9. **Dogfood continuously.** The primary operator drives priorities with live
   evidence from real work.

## Scope of M0

- Assess the complete legacy product at capability and journey level.
- Decide keep, redesign, merge, or remove for each capability.
- Approve the target domain and architectural boundaries.
- Specify the persistent terminal model.
- Define a walking skeleton and objective acceptance targets.
- Establish safe side-by-side testing and eventual cutover.

M0 deliberately excludes production runtime implementation.

## Program risks

- Reproducing legacy structure under new syntax.
- Mistaking obscure behavior for required behavior.
- Removing a low-visibility feature that carries important operational value.
- Allowing Rust and React work to advance faster than behavioral verification.
- Running legacy and next-generation instances against the same workers or
  database during dogfooding.
- Letting temporary compatibility layers become permanent architecture.

## Authority

The primary operator approves product decisions and serves as the principal
dogfooder. The architecture documents and accepted ADRs are the implementation
authority for AI and human contributors.

