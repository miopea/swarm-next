# Legacy Swarm evolution atlas

Status: **Active evidence review**

## Purpose

Legacy Swarm is evidence, not a specification. Its history records which
operator problems were real enough to solve repeatedly, which solutions held,
which fixes caused regressions, and which constraints have since disappeared.
Swarm Next uses that evidence to avoid both amnesia and accidental porting.

This atlas is reviewing every reachable legacy commit from the repository root
to the final legacy release. Each capability is classified as **keep the outcome**,
**redesign**, **remove**, **defer**, or **investigate**. A commit count is not a
priority score; repeated fixes usually indicate a difficult boundary, not a
feature that should be copied.

## Repository baseline

- Reachable history: 1,431 commits from 2026-02-07 through 2026-08-10.
- Release commits: 362.
- Conventional-change subjects: 303 fixes, 187 features, 38 refactors,
  14 tests, 9 performance changes, and 3 security changes, plus earlier
  unconventionally named work.
- Subject-level evidence clusters include 131 terminal/PTY changes, 118 task or
  assignment changes, 83 Queen changes, 77 drone/polling changes, 74 settings
  changes, 44 worker-state changes, 36 mobile/PWA changes, 34 security/auth
  changes, 25 messaging changes, 23 recovery changes, 21 memory/resource
  changes, and 19 email changes. These sets overlap.

The root commit already contains adaptive polling and circuit breakers, so this
repository begins after the very first prototype. The audit therefore treats
the root as the earliest available launch baseline, not the invention of every
concept.

## First-pass evolution map

### February: a useful prototype meets production reality

The first month established workers, tasking, the web UI, mobile/PWA support,
Queen directives, drones, and then replaced tmux with a direct PTY holder. It
also accumulated dense follow-up work around terminal approvals, false state
classification, unsafe command matching, revive loops, operator typing guards,
poll cancellation, resource cleanup, authentication, XSS/CSP, and unbounded
collections.

Evidence includes `c7e2efb7` (direct PTY management), `d547b6e2` (do not inject
while the operator types), the series from `599d0947` through `b6b568e7`
(approval and idle-prompt guards), `703c9ca3` (revive-loop bound), `de1763d1`
(seven drone concurrency races), and `f40abbd6` (auth-by-default and CSP).

**Lesson:** durable terminal ownership, operator engagement, permission
boundaries, state truth, and retry identity are architectural contracts. They
cannot be reconstructed safely from terminal text plus timers.

### March: decomposition follows orchestration growth

The pilot and API were repeatedly decomposed after their responsibilities
expanded. Resource pressure, custom providers, state detection, Queen
oversight, pipelines, replay hardening, and development-mode operation became
first-class. Notable changes include `315bfa18`, `24d71aa3`, and `cedabc3d` for
handler/dispatcher extraction, and `68e9a698` for broadcast debounce and
poll-loop performance.

**Lesson:** one deterministic scheduler is useful, but its detectors, state
transitions, commands, and integrations need independent typed owners. Swarm
Next should keep a modular monolith and avoid another all-purpose pilot.

### April: autonomy exposes injection and classification hazards

Speculative task preparation shipped in `8b693339`, was disabled the same day
in `d44ee3e7` after unrelated work reached the wrong worker, and returned with
four guardrails in `6b4b061a`. Later work added event-driven task pushing,
message pickup, pressure hysteresis, stuck-BUZZING protection, context pressure,
and unread-message nudges.

**Lesson:** helpful automation must be based on durable ownership and explicit
preconditions. Terminal injection is a delivery mechanism of last resort, not
coordination truth. Speculation remains deferred until it has cancellation,
identity, resource, and wrong-recipient proofs.

### May through July: breadth, tuning, and operational hardening

Legacy expanded into native loops, token budgets, richer integrations,
dashboard controls, provider safe lists, reconnect behavior, security audits,
and configuration drift handling. A large release cadence often paired a new
capability with several corrective releases.

**Lesson:** hot configuration, observability, mobile recovery, and explicit
resource budgets are daily-driver outcomes worth keeping. A large matrix of
regex rules, poll intervals, and confidence knobs is not itself a product
outcome and should not be recreated unless evidence demands it.

### August: classifier fixes and browser-process failure

Late history concentrated on worker-state restoration, Queen retry semantics,
Jira verification, release churn, and the browser-memory incident. The incident
showed that a continuous event path can expose browser-process costs invisible
to renderer heap instrumentation. Service-worker re-registration amplified the
failure but did not explain its original onset.

**Lesson:** Swarm Next needs owned end-to-end resource evidence, bounded event
streams, quiet steady state, and browser-process soak tests. State changes must
be event-derived and diagnosable rather than inferred from replayed terminal
snapshots.

## Capability disposition

| Legacy capability | Disposition | Swarm Next interpretation |
| --- | --- | --- |
| Durable PTY holder | Keep outcome | Independent Rust worker engine with protocol compatibility and retained provider conversations. |
| Worker state classifier | Redesign | Provider/runtime events plus explicit engagement and lifecycle state; terminal text is supporting evidence only. |
| Drone safe approvals | Remove as a core need | Provider-native automatic approval supersedes most prompt clicking. External effects still require explicit Swarm authority. |
| Drone scheduling and housekeeping | Keep outcome, redesign | Deterministic coordinator below Queen; no LLM call for typed, reversible, policy-complete work. |
| Idle/revive/context-pressure watchers | Redesign | Typed health policies with hysteresis, idempotency, bounded retry, and visible reason/evidence. |
| Headless Queen on high-volume paths | Reduce | Queen handles ambiguity, prioritization, cross-worker judgment, and operator decisions—not polling or mechanical transitions. |
| Interactive Queen | Keep outcome | Primary operator relationship and bounded conductor, protected by engagement leases and approval policy. |
| Proposals/confidence tuning | Redesign | Confidence never creates authority. Present decisions only when evidence or policy cannot resolve them. |
| Inter-worker broadcast messaging | Remove | Worker outcomes flow to Queen; targeted task evidence remains durable. No fleet broadcast. |
| Direct targeted worker messaging | Keep with role limits | Operator and Queen may steer workers; workers return outcomes to their sender through durable channels. |
| Task/Jira/email intake | Keep outcome | Canonical ownership, typed sync/outboxes, explicit imports, reviewed external replies. |
| Speculative task preparation | Defer | Reconsider only after wrong-recipient and cancellation proofs exist. |
| Dreamer/learning miner | Investigate | Measure whether durable operator corrections improve decisions without creating hidden policy. |
| Pipelines/playbooks/standing loops | Defer by journey | Bring back only when a current dogfood journey cannot be expressed by tasks plus deterministic coordination. |
| Resource-pressure suspension | Keep outcome | Owned process-tree and machine evidence; sleeping workers remain unloaded, loaded workers are attributable. |
| Hot development reload | Keep outcome | App/API swap that preserves the independent worker engine; clearly distinct from worker-engine restart. |

## Deterministic coordinator boundary

The useful part of legacy drones becomes a boring, auditable coordinator below
Queen. It operates only when all required facts and authority are present.

1. **Deterministic state work:** reconcile outboxes, expire leases, wake a worker
   for an already assigned task, apply allowed lifecycle transitions, refresh
   sync cursors, and retry idempotent deliveries. No LLM call.
2. **Bounded evidence policies:** detect a stale lease, exceeded retry budget,
   missing verification evidence, resource pressure, or a provider session that
   exited. No LLM call when the response is fully specified.
3. **Queen escalation:** choose a repository worker, resolve ambiguous blockers,
   prioritize competing work, interpret incomplete evidence, or frame an
   operator decision.
4. **Separate external authority:** Jira writes, email replies, deployments,
   purchases, messages, and other effects require their own recorded approval
   regardless of whether the coordinator or Queen proposed them.

Every coordinator action needs an event identity, precondition revision,
idempotency key, bounded retry policy, typed outcome, human-readable reason,
and durable audit record. It must never scrape terminal text to infer authority
or silently retry an action whose delivery became uncertain.

## Measures that prove the layer helps

- deterministic actions completed;
- Queen calls avoided, by rule and journey;
- escalations to Queen and to the operator;
- false or repeated escalations;
- uncertain deliveries and manual recoveries;
- action latency and retry count;
- resource cost per loaded worker and per coordination cycle.

The target is not maximum automation. It is fewer unnecessary LLM calls and
interruptions without hiding failures or expanding authority.

The first live rule is now implemented: a Queen-originated Ready assignment to
a sleeping worker creates one revision-bound durable wake action. Operator
assignments remain manual, and an ambiguous wake never replays. Settings shows
the completed action and avoided-Queen-call count so the layer's value and
failure state are visible during dogfooding.

The next two evidence rules are also live. Revision-stale Active work surfaces
only when its loaded worker is resting and unengaged. Active work whose worker
process exited surfaces only after the five-minute recovery window, when no
replacement session or engagement exists. Both observations are bound to the
exact task revision, owner, and process incarnation; neither injects a terminal,
changes a task, or spends a Queen call merely to discover the condition.

Automatic wake admission is now serialized as well. A safe resource sample can
claim only one sleeping worker; remaining Queen-originated wakes stay durable
until the next pass samples the newly changed process tree. This preserves the
legacy pressure-management outcome without recreating fleet-wide start bursts,
suspension, or timer tuning.

Queen's lifecycle authority is ordered behind that wake. Ready and Blocked
work cannot become Active through MCP until the assigned worker has a live,
transaction-validated session. This closes the same-turn assign-then-start race
without imposing local-worker semantics on Jira's externally canonical state.

## Complete commit ledger

The repeatable ledger pass now covers all 1,431 reachable commits. It records
full and short identity, timestamp, change type, release marker, file-aware and
subject-only capability tags, linked issue references, file count, churn, and
subject in `docs/legacy/commit-capability-ledger.csv`. Its generation script is
`scripts/analysis/build-legacy-commit-ledger.cjs`; the self-test protects log,
classification, and reference parsing.

The file-aware pass finds 346 task, 341 worker, 279 terminal, 234 drone, 174
Queen, 136 resource, 97 mobile/PWA, 86 Jira, 85 worker-state, 82 messaging, 75
recovery, and 30 email commits. These overlapping counts are deliberately
broader than the subject-only baseline above: they include implementation files
and commit bodies, so they describe evidence touched rather than author intent.
The generated summary and bounded regression candidates live beside the ledger.

Six high-value chains were then checked against commit messages and touched
source/test files in `docs/legacy/validated-regression-chains.md`: terminal
ownership, automated-input authority, revive loops, speculative preparation,
mobile scrollback, and state classification. Their stable outcomes already map
to Swarm Next invariants; none creates a port ticket by itself.

The final product-contract pass now compares the README's operator promises
with the final implementation owners and executable tests in
`docs/legacy/final-contract-audit.md`. It separates held contracts from limited,
partial, and contradicted claims and promotes ten outcomes into Swarm Next. In
particular, it rejects equal-provider and offline-PWA implications that the
final legacy tree itself did not support.

## Remaining history passes

1. Expand validated regression chains beyond the six highest-risk operator
   journeys when a new dogfood gap needs historical evidence.
2. Sample the implementation at stable release boundaries rather than only the
   final tree.
3. Feed each surviving outcome into the capability inventory and dogfood plan;
   do not create port tickets directly from this atlas.

## Acceptance for the archaeology milestone

- Every legacy capability has one disposition and an operator outcome.
- Repeated incident classes link to the Swarm Next invariant that prevents
  recurrence.
- Removed or deferred features state why their original constraint changed.
- The deterministic coordinator backlog is ordered by avoided Queen cost and
  operator interruption, not implementation similarity.
- No legacy database, configuration, repository, or running process is changed
  during the review.
