# Returned-review recovery checkpoint

## Live acceptance failure: an unfinished run lost from Queen's context

The full 507-test API rerun and strict API lint pass. Recovery integration
`a053ae1139999c486503e25a066f295543422fe3` was pushed to main and the clean Linux
clone fast-forwarded. CI 34084913920 is in progress. Normal dev reload was
requested and its service is running as PID 3821846 at this checkpoint.
Before reload, all 16 loaded workers retained their captured session identities;
the live API was still 8e3d6647. Final version, engine reconciliation, worker
continuity, CI and post-deployment behavior must still be verified. No release
was cut, and the inactive support foundation was excluded.

The downstream demo eventually completed on the **existing 8e3d6647 build**,
not on the still-local continuation changes. Audit sequence 7507 moved Blocked
to Ready at 1788755995; 7509 is the assigned Contract worker's pickup, and 7512
is system-owned Review-to-Completed at 1788756076. The assignee was preserved.
The worker explicitly reported that this documentation-only repository contains
no tests: `node --test` executed zero tests, not a meaningful test pass. This
proves eventual routing, pickup and automatic no-code settlement, not contract
correctness or prompt resumption by the new implementation. The earlier lost-run
stall remains a verified failure. No manual unblock was performed by this task.

The two full-suite failures both use ambient resource admission before the
operation under test. A deferral yields exactly the observed no-revival or
Sleeping result. Their fixtures now use the existing test-only Allowed admission
input for drain/return and recovery-circuit behavior; dedicated pressure tests
remain in place and production admission is unchanged. The full API rerun now
passes all 507 tests in 132.83 seconds. This addresses the two fixture failures;
it does not prove that every historical CI failure shared this cause.

Full API evaluation finished **505 passed, 2 failed**:
`package_return_preparation_requires_auth_and_drain_and_preserves_sessions`
returned no revival after drain cancellation, and
`repeated_queen_recovery_opens_a_visible_circuit_instead_of_respawning`
reported Sleeping instead of Blocked. Both pass when rerun individually.
That establishes suite-sensitive behavior, not its cause or a waived gate.
Inspect shared resource admission and fixture assumptions before promotion;
do not weaken production safety or call the entire suite green.

The recovery-only main integration is clean at `8aaac213`, based on `8e3d6647`.
It excludes the inactive support foundation and introduces no migration changes.
It has not been pushed or deployed. A temporary content-identical index entry
interrupted the first cherry-pick sequence; that owned sequence was aborted and
reapplied in full dependency order, with the final real-PTY test included.

The HTTP handler regression now verifies authentication is required, responses
are no-store, an exhausted idle review reports its wait, and resumed terminal
work clears that explanation without changing run/state/attempts. Two fixture
initialization attempts failed authentication before the correctly configured
fictional-credential fixture passed. No production credential was used.
The real terminal-host/PTY handoff test now also verifies continuation queues
and redelivers the same run through normal delivery, increments its existing
attempt count, and retains coverage enforcement when Queen finishes. It passes.
The full 507-test API suite is running at this checkpoint; the live demo outcome
and authenticated rendered acceptance are still open.

Host-failure acceptance now passes for missing socket, explicit host error and
a stalled response bounded by the two-second observation deadline. All preserve
the Running review, run ID and attempt count without writing terminal input.
The nine-scenario adapter regression still passes after extracting the shared
observation helper. Exhaustion reporting is now a read-only projection requiring
current idle evidence and exact durable run/session/budget/engagement checks;
busy work does not qualify. It creates no decision or retry. Queues displays the
API's running-review wait explanation and drops it when the explanation clears;
all 31 queue tests and TypeScript checking pass. The focused persistence budget
test passes. Strict lint identified two redundant borrows in the helper callers,
which were removed. Full redelivery, HTTP projection and rendered acceptance
remain unproven; this slice is still local, not deployed.

Adapter acceptance now exercises the real host protocol against an isolated
Unix socket for idle, busy, unknown, unsent input, wrong-session, ended-process,
truncated-snapshot, concurrent engagement and concurrent finish cases. Only idle
queues continuation, none of these observation calls writes terminal input, and
the original run identity/attempt count is preserved. This passes on Linux.
An additional lifecycle regression found and fixed disabling automation erasing
an already-delivered queued continuation. Disabling now holds it without another
claim or losing its identity; an explicit finish still works. All 42 conductor
tests pass. Lint flagged a nested test conditional, subsequently corrected.
Unavailable/timeout-host acceptance, end-to-end redelivery and exhausted-budget
visibility remain required before deployment. No live task was manually unblocked.

The next local slice adds domain-gated same-session continuation: a current,
complete, resting snapshot with no background work or unsent input can queue
the exact Running review again. Persistence rechecks session, engagement and
Steward ownership and retains the existing three-attempt budget. A concurrent
finish can close an already-delivered continuation while queued/delivering;
initial undelivered runs remain protected. The queued continuation keeps its
unattended authority ceiling. No additional timer or migration is introduced.
The domain gate, two new continuation lifecycle tests and all 41 conductor
tests pass; the prior 41 delivery tests also passed before the final continuity
guidance addition. Strict lint initially found a missing semicolon in logging.
The corrected source passes formatting and strict domain/persistence/API lint.
Do not deploy this slice before adapter-level busy/input/failure coverage and
budget-exhaustion visibility are verified. The live demo is still not accepted.

Local context-restoration implementation now exposes an unfinished delivered
run/session identity in Queen-only coordination attention using a pure database
read. Preparing a task-message notification for that exact session appends a
bounded reminder to recheck and finish the existing run, not replace it. The
notification retains its original delivery marker and final submit byte. No new
timer, database migration, worker restart or standalone prompt is added.

Linux validation passes the new API identity/authentication/finished-run check,
all 41 coordination-delivery tests and all 39 Queen-conductor tests, including
the new read/restart/finish identity regression. Full formatting and strict
API/persistence all-target/all-feature lint pass. This is local, not deployed;
same-session idle recovery with no incoming notification is still missing, and
the live two-worker acceptance remains open. The formatter/identity tests do not
by themselves prove live notification routing or recovery safety end to end.

At the September 7 follow-up, the authoritative automation endpoint still
reported run `01a079e8-0b21-7af1-9314-5a9894417eee` as Running, with no finish
time/outcome, 67 actionable records and 22 Queen-owned tasks. The demo downstream
`01a079e9-2fd9-7261-8acb-c3c594cb3187` remained Blocked, assigned to its original
demo worker, Queen-owned and without a dispatch. This acceptance has **not passed**.

Queen's current terminal showed that provider compaction had finished. A worker
notification then received a substantive response, ending at 23:42 EDT with an
empty prompt and the statement that there was no active automation run or run id
in play. She deferred the demo item to another run/notification. That statement
contradicts the durable run record. The terminal report also described correcting
a false-premise C19 dependency; those real-task mutations were not independently
verified in this check and are not acceptance evidence for the demo.

Source inspection explains the missing recovery route: session-ended recovery
requires the delivery session to end, whereas this is the same live session.
The running-run fallback instead waits for a one-hour expiry into Uncertain.
The coordination-attention response does not currently expose the active run
identity, and ordinary task-message delivery does not reconnect the notification
to an unfinished review. Coverage checks only protect an explicit finish call;
they cannot help when Queen believes there is no run to finish.

Next implementation must restore the exact unfinished run from durable state,
without treating compaction, a notification, or an empty prompt as completion.
Use fresh same-session terminal evidence and existing input/engagement guards;
do not replay side effects, clear provider context, restart workers, create a new
run to hide the old one, or depend on a shorter timeout. Test notification and
compaction/context-loss recovery, busy/background execution, unsent input,
operator engagement, duplicate observations, restart and a concurrent finish.
The live downstream remains untouched so the eventual handoff can be observed
through the product's recovery path rather than a manual unblock.

## Live two-worker acceptance on 8e3d6647

CI 34079309932 now has successful Rust, web, Linux packaging and security-audit
jobs. During the live acceptance review Queen handled an earlier real dependency,
then explicitly reached the demo prerequisite item before provider-native
conversation compaction. No manual kick, resume, restart or blocker removal was
performed. The downstream outcome remains to be observed after compaction.

Bounded quiet-server sample at 23:35 EDT: 25,230 MiB available of 32,042 MiB,
load averages 1.90/1.75/1.93 on eight CPUs. Five one-second pidstat samples measured
API PID 3753112 at 0.00% CPU and 90,024 KiB RSS, terminal-host PID 3748322 at
0.40% average CPU and 83,528 KiB RSS. Host service memory also includes its worker
processes, so the roughly 5 GB cgroup total is not terminal-host process memory.
This quiet five-second sample is not a long-term soak or browser performance test.

`scripts/dogfood/recovery-acceptance.sh` created two fictional read-only tasks in
the existing isolated demo repositories. Upstream
`01a079e9-2fbd-7a61-a313-4c4e16a9757f` ran nine Node tests, reported no commits,
and submitted Review. Activity sequence 7489 is a **system** transition to
Completed on recorded no-commit evidence; no Queen approval was needed.
This is live evidence for routine automatic completion, not just a passing unit
test. The worker reported a clean unchanged repository and no external actions.

Downstream `01a079e9-2fd9-7261-8acb-c3c594cb3187` retained its assignment and
Blocked state while the explicit upstream prerequisite was outstanding. Once
upstream completed, next-move ownership changed to Queen without a manual
unblock. Queen run `01a079e8-0b21-7af1-9314-5a9894417eee` has now started through
normal scheduled delivery. Downstream pickup is still pending; do not claim the
whole acceptance passed until its actual handoff and outcome are observed.

The first inline setup command failed before creating anything; a read confirmed
no tasks existed. The checked-in fail-fast setup script then completed, and
refuses duplicate setup. It never sends raw terminal input or edits real tasks.

## September 7 recovery deployment in progress

Final deployment check: `1.5.0-dev-8e3d6647072d-20260907032044-3749690` is
healthy with no degraded subsystem or database-recovery requirement. Source is
clean/current and `worker_engine_update_required` is false. All 16 captured
workers returned with new session identities after the engine swap; their
conversation freshness reports current. This verifies product continuity checks,
not independent provider-history or productive-task-resumption acceptance.
CI 34079309932 and actual multi-worker recovery observation remain pending.

Root checkpoint `7d0b7bf5` was integrated onto main as `9130b38b`, excluding the
inactive support foundation. In an exact main-only Linux checkout, 34 recovery
tests passed, including deployed schema 142 to recovery schema 144 with the
support-outbox table absent. Both recovery endpoint tests and strict lint passed.
The earlier full root-branch evaluation passed 502 API, 36 application and 127
domain tests; persistence had 628 passes and the subsequently corrected newest-
migration fixture. Its corrected focused rerun passed.

Normal dev deployment brought `9130b38b7635` up healthy. Engine reconciliation
then replaced engine `54dec4b9bf05` with `d6a2e9b3a041` at 23:19 EDT. The 16
pre-update worker/session identities were captured; restoration was still
progressing (eight returned at the last check), so this is not yet a claim that
every worker returned or resumed productive work.

CI 34079060850 failed Rust formatting in the main-only migration test; web and
Linux packaging passed. Full `cargo fmt --all --check` was then verified, and the
formatting-only correction was committed/pushed as `8e3d6647`. The remote clone
is current; its second normal dev reload is running (PID 3749666). CI 34079309932
is in progress. No release was cut. Completion still requires final health,
worker return/conversation checks, CI result and actual multi-worker recovery.

## Recovery finish integration (local, not deployed)

Full Linux library evaluation: 502 API, 36 application and 127 domain tests pass.
Persistence finished with 628 passing and one migration-fixture failure: the
previous-schema fixture still removed the support-outbox table instead of the
new recovery table. The recovery table is now registered as the newest schema
artifact; its focused rerun passes, as does final strict all-target/all-feature
lint for persistence, application and API. The full persistence suite was not
rerun after this test-fixture-only correction.
This is not a claim that the main-only migration has been verified yet.

The agent endpoint round-trip now passes: a checked external wait is saved,
replaying the same request returns the same accepted identity, and finishing the
run recognizes coverage without changing the Active task or worker session.
Unavailable terminal evidence remains unavailable; this scenario checks a
separate external-condition judgment, not proof of healthy execution. The
ordinary-worker/missing-observation endpoint regression also passes. Strict
all-target/all-feature lint passes for persistence, application and API before
this final endpoint test addition. Full library suites are now being evaluated.

Live health rechecked during isolated validation: `a78c53e0fb62` remains healthy,
both API and terminal-host services are active, and root disk is 69% used with
20 GB available. Upstream main still matches the clean main integration checkout.

The external-wait command path now records a bounded condition (reason), checked
evidence and source as an authenticated judgment, never completion or operator
authority. Coverage requires the same run and task/session/terminal revision;
the task remains Active. Five domain and ten persistence recovery tests pass,
including blank-evidence refusal, ordinary-worker refusal, and changed-run or
terminal invalidation. This supersedes the earlier unsupported-path notes below.
Successful endpoint round-trip, full lint/migration checks and live acceptance
remain open; the feature is still undeployed.

Follow-up wiring adds Queen-only `swarm_record_recovery_assessment` through
TaskService, using freshly obtained server observations and the existing fenced
persistence command. The public schema does not expose trusted fact booleans;
external-wait assessment remains unavailable pending its explicit judgment path.
Tool surface revision is now 21 so stale provider tool caches are detectable.
Thirteen existing tool-related API tests pass after the initial command wiring.
The additional missing-observation/ordinary-worker endpoint regression also
passes: both requests are refused and the fictional task stays Draft. No
deployment or full recovery acceptance is claimed.

The finish endpoint now obtains fresh bounded canonical-terminal observations
before evaluating recovery coverage in the persistence transaction. Current
working evidence needs no Queen approval receipt; unknown or missing observations
cannot certify an old stall as covered. Explicit incomplete remains a valid
terminal outcome without another observation round trip. Persistence separately
enumerates obligations, so observations omitted by the adapter's bound remain
uncovered rather than silently disappearing.

Linux isolated validation passes nine recovery persistence tests, eleven API
observation tests and all 38 Queen conductor tests. These include restart/replay,
atomic failure and capacity recovery, stale/delivered evidence rejection, and
fresh working coverage without a Queen approval round trip. The previous test
handle was unavailable; these results came from a new completed focused run.

Worker-facing no-deployment guidance also no longer claims every pending claim
requires Queen approval: it distinguishes automatic documentation/no-code
settlement from exceptions requiring judgment, retaining the warning to record
anything that actually shipped. This does not expand automatic eligibility.

Still required before deployment: explicit recovery assessment command and
external-condition judgment/source handling, endpoint failure/recovery tests,
main-only migration verification excluding the inactive support foundation,
and isolated live multi-worker acceptance. This is not yet the fleet recovery fix.

## Isolated live dependency verification

On healthy `a78c53e0fb62`, created fictional unassigned dogfood tasks
`01a0797a-11b0-7290-b209-e4c85b0800fe` (consumer) and
`01a0797a-11c0-7f82-bcea-6d6a9731c367` (upstream). Adding the explicit link kept
the consumer in Review and projected its next move as blocked. Awaiting Release
was refused with HTTP 409 `task_prerequisite_refused`. Removing the fictional
edge kept Review, restored Queen ownership, and permitted Awaiting Release.
Both fixtures were then abandoned with explicit verification notes; no worker
was assigned or prompted and no real backlog task was changed.

The initial Completed attempt returned HTTP 400 for missing completion evidence,
before reaching the prerequisite guard. It does not count as a live completion
guard test. No deployment or commit evidence was invented to make the test pass.
Automatic settlement and satisfied-upstream recovery remain backed by the Linux
tests above/below, not by this narrower live operator-route exercise.

## Final update-cycle verification (September 7 UTC)

Main `a78c53e0fb62` is healthy as
`1.5.0-dev-a78c53e0fb62-20260907011940-3649827`; the development reload unit
finished successfully. No release was cut. The earlier exact-session preservation
snapshot below was not the final outcome of the engine update: host reconciliation
deferred at 21:10 and 21:13 EDT because one of 16 workers was mid-turn, then
replaced the engine at 21:16 once the fleet was quiet. All 16 workers returned
with new session identities. The provider-conversation freshness endpoint reports
current for each running worker. That verifies the product's continuity check,
not independent proof that each task resumed productive work.

CI run 34071894455 passed for `002ef1999bf2`. Follow-up run 34072636383 for
`a78c53e0fb62` also passed. Browser validation remains
blocked on unlocking the separate Edge tab, not on server availability.

Recovery-accountability work has begun locally in the domain layer. Five tests
cover session/revision fencing, working versus unknown/resting observations,
operator input, exact pending delivery, and verified decision/external sources.
This is not runtime enforcement: durable source validation, receipt persistence,
agent commands, completion coverage and live recovery acceptance remain open.
Do not deploy or describe this domain-only foundation as the completed fix.

Local follow-up adds a consistent persistence recovery-identity read using the
existing live-attention predicate and task-evidence revision. The returned-review
regression proves a same-second replacement invalidates its old identity. The
bounded agent observation now exposes current/changed/unavailable identity status.
That regression, all nine observation API tests, and strict persistence lint pass.
No new recovery receipt is saved yet: this remains undeployed foundation work.
Completion coverage must refresh terminal observations rather than reuse a
working assessment after output changes or a provider returns to its prompt.

The local identity now includes an optional terminal revision derived only from
a current, non-truncated canonical snapshot. Persistence leaves it absent;
working/unsent-input assessments cannot be certified from database facts alone.
Domain tests reject changed or absent terminal revisions, and the API regression
checks output, sequence, geometry and truncation changes. All five domain recovery
tests and ten observation API tests pass. Finish-time refresh and command wiring
are still required before this becomes enforcement or is deployed.

The subsequent local receipt implementation uses migration 144 and rechecks live
identity, review run, engagement, pending delivery and decision sources inside
its write transaction. It caps records at 256 and retains task-activity audit
history when retiring obsolete receipts. Five persistence tests pass: reopen/
idempotent replay, unauthorized/invented delivery rejection, atomic failure and
retry, capacity recovery, and delivered-request invalidation. Strict persistence
lint passed before the last two test additions. External-wait judgment writes
remain deliberately refused until their explicit source path is implemented.
Nothing in this section has been deployed or proves full recovery supervision.

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
