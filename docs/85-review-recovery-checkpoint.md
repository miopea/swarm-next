# Returned-review recovery checkpoint

## Review prerequisites and routine completion (local validation)

Deployed main `002ef1999bf2` through the normal development updater. Health
reports `1.5.0-dev-002ef1999bf2-20260907010656-3634784`, no degradation and no
database recovery required. All 16 captured running worker/session pairs remained
identical after the update. The reported worker-engine build ID changed, so this
checkpoint claims verified session preservation, not an unchanged engine build.
The dedicated Edge test tab still requires operator unlock; no visual acceptance
is claimed. No release was cut.

Follow-up audit found the Awaiting Release deployment writer could close directly
without rechecking a prerequisite that had become invalid. The follow-up keeps
the deployment evidence, withholds completion, and lets the existing deployment
sweep close after reconciliation. Isolated Linux validation passes all 27 email
persistence tests, 58 task-outcome tests, and strict persistence lint. This
follow-up is not included in the `002ef1999bf2` deployment checkpoint above.

QUEEN-01 explicitly requires machine-verifiable routine work to settle without
mandatory Queen approval. Both automatic paths are wired into the API coordinator:
whole-task deployment evidence and reported documentation/no-code outcomes.
Review is a lifecycle state, not a requirement for a Queen approval on every task.
Missing or partial evidence still needs resolution; being idle is not completion.

The local prerequisite extension preserves Review while naming its upstream wait.
Automatic completion skips unmet prerequisites without holding unrelated work,
rechecks prerequisites at the transition boundary, and completes eligible work
after dependencies clear without requiring Queen. Coordinator-approved exemptions
interrupted before completion remain eligible for recovery, with current commit
facts re-derived rather than treating an old approval as sufficient by itself.

Linux isolated validation passes all 58 task-outcome tests, including dependency
wait/recovery on both automatic paths and interrupted exemption settlement.
The full Linux suites pass 122 domain and 618 persistence tests. The actual
agent-endpoint test passes for both Blocked and Review, preserving Queen-only
dependency authority. The editor now supports Review without rewinding its state;
104 focused queue/task UI tests and TypeScript checking pass. Strict persistence
all-target, all-feature lint passes after correcting its findings. These changes are
deployed but not yet accepted in the live browser. No real backlog edges
were inferred from prose or changed manually as part of these tests.

## Deployment verification (September 7 UTC)

Main `109ba917271f` is running healthy as
`1.5.0-dev-109ba917271f-20260907001509-3572626`. The worker engine is unchanged.
All 16 pre-deployment worker/session pairs survived exactly; 17 workers were
loaded at the follow-up, with Scout and Platform reporting Active. Member
Services and D365 remained Resting. This proves deployment preservation, not
successful fleet recovery. The queue grouping and returned-review detector are
now deployed; authenticated Edge visual acceptance and live recovery remain
open. The older checkpoint below records pre-deployment validation history.

Development Hive health verifies `0023ef4f4be4`, with the worker-engine build
unchanged. This corrects the contradictory delivery envelope, not the full
orchestration recovery loop.

Queue worker grouping is committed locally as `e2b549a0`: existing owner
sections, roster-ordered workers, position/id task ordering, explicit unassigned
and missing workers, and clearer returned-review wait labels. All 34 queue
tests and the TypeScript check pass. Not deployed or visually accepted; the
dedicated Edge tab is at the unlock screen.

Recovery work extends observation to outstanding returned Reviews
assigned to the request's worker. It preserves Review state and excludes
answered requests. The new regression and all 45 coordinator tests pass in
`/tmp/swarm-support-check.hX4wMI` on Linux.

Observations and deduplication now include the exact returned-review message
identity. A same-second replacement regression proves an old candidate is
refused, its observation disappears, and the new request is eligible. All 45
coordinator tests pass after that change. Strict persistence/API all-target,
all-feature lint also passes. Follow-up validation passes nine API evidence
tests and the new returned-review decision/reassignment regressions. The
combined App/Queues run passes 94 tests after explicitly waiting for the
experimental handoff button to become enabled; its preceding combined run
failed while the isolated test passed. This is not proof all CI flakiness is
resolved. Pending versus delivered request checks and a live multi-worker run remain.
A live multi-worker recovery run is still
required; no passing detector test proves end-to-end recovery.

ADR 0082 recovery-accountability enforcement and the broader program remain
open. Observation coverage does not prove that Queen recovered a worker.
D365's restricted code/docs approval is saved on its task; provider receipt and
handoff remain unverified.
