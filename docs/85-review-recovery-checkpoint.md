# Returned-review recovery checkpoint

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
