# Current maturity acceptance ledger

## September 10 — invitation observation ownership

Keeper invitation reads now use the existing visible-page polling owner: one
in-flight read,8-second abort deadline,hidden/unmounted cancellation and no hidden
polls. The API adapter passes the cancellation signal. Confirmed mutation results
are retained immediately and fence earlier reads, so stale empty results cannot
erase a newly created invitation. A follow-up read failure no longer converts a
confirmed create/cancel into failure. Mutation transport itself is unchanged.

Initial unavailable status no longer claims there are no invitation links.
Failures retain last confirmed information and offer a read-only retry; fresh
success quietly clears them.31 focused invitation/settings/polling tests pass,
including timeout,hidden/disposed ownership,stale read versus creation and failed
refresh after successful creation. TypeScript and production build pass before
the final copy refinement and additional regression.

Edge's isolated invitation-recovery fixture showed unavailable status and retry.
After Restore fictional invitation service, the next automatic read cleared the
warning before the attempted manual retry; therefore manual retry is unit-tested,
not claimed browser-tested in this run. At390px DOM measurement shows no horizontal
overflow. Viewport reset and owned tab closed. No real link,grant or membership
was created or changed. Local only, pending publication approval.

## September 10 — Apiary guidance and full web regression

The full local web suite passed1555 tests across158 files after64922011.
Live6ac6c145 Edge inspection verified Keeper overview -> Manage Apiary navigation,
registration-versus-presence wording, shared ownership summaries and disabled
return-to-personal controls with three registered Hives. No invitation, access
grant, departure or configuration mutation was performed.

That review found misleading Keeper guidance promising a simple departure, and
invitation instructions pointing to the retired Settings -> Apiary location.
Local copy now distinguishes Keeper return prerequisites from Member departure
and consistently names Settings -> Connections -> Your Apiary. All27 affected
Apiary/invitation/departure tests and TypeScript pass. The full1555-test result
precedes this copy-only change; it is not a claim of a subsequent full-suite run.
Owned live review tab closed. End-to-end multi-Hive invitation/security/recovery
acceptance is not established by this read-only UI review and remains separate.

## September 10 — task details entry point

Live Edge review on6ac6c145 found task titles were plain headings: opening details
required Edit or an undiscoverable double-click. TaskCard now exposes a native
button in each heading, opening the same existing dialog without transitioning,
assigning or waking work. Existing Edit and double-click paths remain available.

The regression failed before implementation and all67 TaskBoard tests now pass.
Type checking and the production web build pass. In the isolated no-proxy harness,
Edge verified Enter -> details -> Close -> focus returns to the title. At390px,
single-click opening/cancellation also passed; all six fixture title targets were
44px tall and the document had no horizontal overflow. Desktop and phone-width
rendering were visually inspected. This is not Android/iOS device acceptance.
Temporary viewport override was reset and owned browser tabs closed.

Live filter recovery also passed: an unmatched search exposed Show all open work,
which cleared the filter. No task edits or workflow transitions were made in the
live Hive. The title fix is local pending publication approval, not deployed.

## September 10 — latest verification checkpoint

Live health confirms App/API6ac6c145 and engine30ad01d with equal engine build
fingerprints,13 running/retained sessions,0 unreadable and no degraded flags.
The latest check was read-only, not another update or restart.

CI34506586132 ultimately failed its Rust schema-ceiling fixture: the fixture
dropped schema160 data while claiming to represent160. Local commit32dc50ee
models the161 reader fence without dropping prior data and adds a populated
native-source preservation regression. Targeted migration checks pass; the
main push and CI rerun remain pending permission. Other CI jobs passed.

The native final-batch comparison now passes8 parser tests, formatting and strict
terminal Clippy. The fictional provider probe demonstrates that final batch input
loses earlier programmatic-answer evidence; batch completion alone must never
authenticate an operator. ADR0065 records the fixture and bounds. Hooks remain
disabled, exact decision binding and authentic end-to-end reconciliation remain
open. No claim that directly answered Needs You items now close automatically.

## September 10 — answer controls and update recovery

Deployed App/API `1.7.0-dev-6ac6c14543de-20260910171052-580861` is healthy.
Engine PID565859 and its 12:54:56 Eastern start are unchanged across this ordinary
reload;13 running/retained sessions remain,not draining,0 unreadable. Its engine
build ID still matches the API. The restored development watcher is active.

Four new regressions failed before the answer-control fix: question headers
`constructor`, `toString`, and `__proto__` crashed rendering; option `__other__`
incorrectly opened custom input. Map-backed selections and a non-string internal
sentinel preserve those literal values. All101 affected tests pass, plus TypeScript
and the production web build. Edge's fictional free-text journey submitted the
exact entered text. The form now describes unanswered fields without asserting
that a worker is currently waiting. This does not close native decision reconciliation.

Edge also verified fictional support review, Close -> Keep editing draft retention,
and pending submission wording. No central message,email,or diagnostics were sent.
These checks are not an end-to-end BFG Admin delivery/approved-reply acceptance.
Owned browser tabs were closed. CI34506586132 has passing web,package,and audit
jobs;Rust remains in progress at this checkpoint. The preceding revision's Rust
job failed formatting only;the standard formatter correction is included here.

Checkpoint: September 10, 2026. Scope remains [45](45-daily-driver-maturity-plan.md),
including BFG Admin integration. This is a routing index over evidence, not a
replacement specification or a declaration that partially verified rows are done.
Historical implementation notes in [47](47-maturity-remaining.md) remain evidence;
an old unchecked deployment note is not automatically current missing code.

**Current execution priority:** [93](93-ux-first-remaining-plan.md), directed by
the operator September 9, puts complete user-facing UX/UI packages first and
retains backend/performance work as an explicit handoff. At approximately five
percent account usage remaining, consolidate unfinished work rather than claiming
the overall goal complete. Earlier "local/not deployed" checkpoint prose below
is historical where superseded by the verified deployment section.

## Verified runtime and delivery

- App/API: `1.6.0-dev-fd0385d26bf8-20260910093249-176487`.
- Engine: last verified PID 3408834, started September 9 at 17:01:49 Eastern,
  after the operator's manually applied update. Subsequent app-only deployments
  preserved that engine and its running worker identity projections.
- Latest App/API deployment preserved all 34 worker records and twelve exact
  running session/provider identities. The worker-list projection did not expose
  conversation IDs, so this does not independently prove conversation continuity.
  Swarm Next and D365 remained asleep.
- Confirmed full CI `34458764563` for display recovery83a88edd passed all four
  jobs. Later queue-retry/navigation CI remains separate and unconfirmed here.
  Subsequent UX receipts and the consolidated remaining milestone map are in95/96.
- No release is authorized. No customer-facing send is authorized by task closure.

## Requirement-by-requirement disposition

### QUEEN-03 / ATT-01 native answer capture — foundation verified, integration open

Exact question representation now retains bounded option descriptions through
decision storage, native conversion and the rendered Needs You form. Schema161
protects against older database readers; MCP surface25 advertises the field.
Domain153, affected persistence54, tool-contract1, UI63, TypeScript, production
web build and strict affected-crate clippy passed. Desktop/390px Edge fictional
views were checked, not native-device acceptance. This package remains local and
awaiting final-font browser verification. The HTTP answer path now requires an
exact rendered-question snapshot for described questions, checked inside the
resolution transaction. Domain154, affected persistence54, two HTTP route tests,
two focused client/full-App tests, strict affected-crate clippy and web build pass.
Changed/missing snapshots leave requests pending; an exact retry succeeds. Plain
legacy questions remain compatible. Immutable decision binding, final native-result
verification and actual provider capture installation still remain open. Main
publication remains approval-gated; no live deployment or worker restart occurred.

An isolated native Claude2.1.267 adversarial callback is now retained as a
regression: programmatic input and the early completion result said Amber while
a later hook supplied Blue to the model. The parser correctly refuses that
programmatic callback. All seven focused parser tests and strict terminal clippy
pass. This proves an exclusion, not operator-to-Queen acceptance; final-result
correlation and exact decision binding remain open (ADR0065/93). No provider
capture installation, decision resolution or deployment occurred in this check.

The private engine path now binds native invocation callbacks to live capabilities,
conversation-selection revisions and generation-checked operator input. Prepare/admit
round trips refuse delayed admission after intervening input. Bounded completed
evidence survives API-reader replacement until exact acknowledgement, not engine
exit. Full Linux terminal165 + host27 + command11 tests and strict terminal/host
clippy pass, with one unrelated opt-in profiler skipped. See ADR0065 and93.
Schema160 now implements bounded private native-source persistence, including
exact questions/descriptions, restart-safe ID retries, capacity refusal and
pending-decision retention. The existing supervisor now connects the engine read
to application-owned durable admission and only then acknowledges saved IDs.
Seven new application/API tests and eight affected supervisor/shutdown regressions
pass; strict application/API all-target clippy passes. This remains local work,
with main publication awaiting the operator's approval after the execution gate
refused it. Intake does not establish human authorship or exact decision binding. No hook
is installed and no live native evidence is used to resolve Needs You.
Validation:151 domain tests passed; initial persistence724 passed/24 migration
failures exposed the missing schema-version update. After correction,99 selected
storage/migration/restart tests passed, including every failed test and all nine
native-source tests. Full terminal165 passed/one opt-in profiler ignored; strict
domain/persistence/terminal lint, API/host consumer compilation and formatting pass.
Evidence logs remain in `/tmp/swarm-native-answer-check.6fkv8E/`:
`native-storage-tests.log`, `native-storage-migrations.log`,
`native-storage-terminal.log`, `native-storage-consumers.log`.
Direct-answer reconciliation remains open; parser/engine/storage tests are not
the required complete operator-to-worker-to-Queen acceptance. No live engine restart
or release occurred in this package.

### UI-4 prerequisite-edit recovery — delivered

80c7d1c4 fixes a reproduced indefinitely locked Saving dialog. Client wait now
has an owned8-second deadline and unmount cancellation; late responses cannot
invoke stale update/close callbacks. Uncertainty preserves choices, never replays
automatically and warns that closing the draft does not roll back server state.
88 focused tests passed; after test typing correction, final12 prerequisite tests
and production build pass. Fictional browser held-save -> timeout -> Keep editing
-> explicit close passes. No real task changed. Exact34-record/12-running identity
comparison and enginePID/start preserved. See95; CI34452563704 remains pending.
Prior Night Watch CI34450368822 is green. This is not Queen-loop completion.

### UI-3 Night Watch configuration clarity — delivered

cb915ab1 separates saved schedules from unsaved edits, identifies overnight versus
same-day windows, explains disabled automation/manual availability and gives a
correction for definitive invalid-timezone400 rejection instead of retrying the
same input. Unknown network outcomes retain not-confirmed wording.48 focused
tests/build pass; fictional edits and live read-only summary/search checked.
Exact schedule configuration,34 worker records and12 running identities remain
unchanged; enginePID/start unchanged. See95 for receipts. CI34450368822 pending;
preceding history-navigation CI34449170913 green. No live schedule changes.
Native scheduled activation and desktop/mobile return remain separate acceptance.

### UI-4 retained task history — delivered

Cancellation444f7a56 and bounded older/latest navigationcb7d4206 are live. The
browser retains one30-event page, resets to latest on reopen, retries older-page
failures at the same cursor, and rejects late cancelled responses. ADR0095 and
[95](95-ui-journey-acceptance.md) record the contract and complete evidence.
148 focused web tests, production build, seven Linux history tests and strict
Rust library lint pass. Fictional desktop browser traversed75 records as30/30/15
and returned to latest. Live A6 history renders26 records with no unnecessary
paging controls; live authenticated five-record cursor reads are disjoint and
strictly earlier. No real tasks/decisions changed. Narrow/native paging remains
an explicit visual gate; desktop target measurement is not phone acceptance.

All34 worker records and12 running session/provider identity projections match
before/after in pre-history-pages-workers.json/post-history-pages-workers.json
under /tmp/swarm-clarification-deploy.cvm5VI. EnginePID3408834/start unchanged;
no release or engine update. CI34447225894 for cancellation is green; navigation
CI34449170913 is still running. Prior clarification/mobile/diagnostic CIs are all
green, superseding their historical pending notes below.

### UI-3 diagnostic assessment consistency — delivered

`b01aef1d` removes the competing machine-aggregate verdict from the capacity
line. The existing assessment still reports CPU/process/machine pressure, and
every detailed check retains its classification. No measurement, safety policy
or polling interval changed. All31 diagnostic/report/assessment tests and the
production build pass. Fictional CPU-only390px and live desktop checks pass.
The live page now reads31.3GiB/8CPUs separately from the current assessment.

`/tmp/swarm-clarification-deploy.cvm5VI/pre-diagnostics-workers.json` and
`post-diagnostics-workers.json` confirm34 worker records and12 exact running
session/provider identities preserved; enginePID3408834/start unchanged. The
pending engine update was not applied. CI34444605660 remains a separate gate.
The isolated fixture server is stopped and the browser viewport restored.

Live queue -> decision navigation also passed: the RCG Networks operator-owned
task opened its existing Needs You decision and focused that exact article.
No decision was answered, dismissed or created. This validates navigation, not
Queen's ability to clear the31 items currently recorded as hers.

### UI-3/UI-4 mobile runtime navigation follow-through

Live Edge390x844 reproduced an expanded System tray retaining most of the screen
after Diagnostics navigation, including within Settings. Explicit navigation now
collapses the tray through the existing navigation owner. Inline details/retries
do not navigate and remain open. All71 App tests and the production build pass;
the full-app fictional phone journey and deployed live journey both pass. This
is a presentation change, not a service/worker/attention-state mutation.

`fd35ca3e` is deployed; `/tmp/swarm-clarification-deploy.cvm5VI/pre-ui-workers.json`
and `post-ui-workers.json` prove34 records and12 exact running session/provider
identities unchanged, plus enginePID3408834/start unchanged. The phone viewport
was restored and the local fixture server stopped. CI34443735026 is a separate
pending gate. Live Apiary -> Manage Apiary correctly opens Connections; narrow
Apiary ownership/empty states and diagnostic preview were inspected without
creating invitations, altering configuration or sending reports.

Next UI-3 finding: during the live diagnostic journey, the machine summary said
no current pressure while the assessment said fresh samples report pressure.
Inspect their sample/time ownership before claiming consistency; the screenshot
is an observation, not evidence of a server performance defect. Do not detour
into reload profiling or BFG communication.

### UI-1 question-before-decision milestone

Non-authorizing clarification is deployed. Queen and a sleeping ordinary author
both completed actual browser question -> native MCP reply -> explicit final
answer journeys in an isolated Hive. Waiting questions remain visible without
counting as operator answers; replies return attention. Existing final choices
and Say something else remain separate. Delivery uncertainty has explicit,
bounded operator recovery rather than automatic duplicate sends.

Evidence and boundaries: [94](94-clarification-native-acceptance.md). Full local
gates passed561 API (3 ignored),48 application,739 persistence and1512 web tests,
plus production web build. Live deployment preserved34 records and12 exact
running identities, with enginePID/start unchanged. Live editor opens without
answering either real request. CI34442276338 passed all four jobs.
Direct-terminal answer correlation, older-provider tool-list
refresh and physical-device checks are not closed by this milestone.

### UI-3/UI-4 ordinary-user checkpoint

Settings search recovery, explicit update interruption warnings, honest stale
diagnostics, support draft/retry copy, worker setup guidance/readability/focus,
and blocking dialog coverage are deployed through d090bbb9. Detailed journeys
and exact evidence remain in [93](93-ux-first-remaining-plan.md).
The operator accepted the Android task editor: it covers navigation and retains
unsaved changes through Close -> Keep editing. This closes that specific device
check. It does not close the entire maturity goal, native attachment picker,
iOS, physical presence, non-authorizing decision clarification, direct-worker
answer reconciliation or the backend/performance handoff.

Deployed follow-through `414de057`: distinguish unavailable saved-session checks from
expired authentication and offer explicit reconnection without another token.
New tests cover failures from both session and snapshot reads, repeated failure,
successful recovery and actual authentication rejection. All 1,497 web tests,
TypeScript and the production build pass; Edge desktop/390px recovery succeeds
without credentials. `/tmp/swarm-session-recovery-deploy.E3bxLd` verifies healthy
activation and unchanged engine plus all 34 worker identities (12 running).
CI 34416874473 remains pending; this checkpoint is not whole-package closure.

### September 9 runtime safeguard placement deployment

`f482b460` is live and healthy. `/tmp/swarm-ui3-deploy.mlsUrp` records identical
before/after identity projections for all 34 workers, twelve running session/
provider identities, and unchanged engine PID 2947820/start time. Coordinator
start admission is currently allowed with no standing holds; no pause notice is
expected in that recovered state. Full-App fictional desktop/phone light/dark
checks verified the pending safeguard remains visible in runtime without inflating
Needs You, and opens diagnostics correctly. Seventy-two focused tests, TypeScript
and the production build pass. Current full CI remains a separate pending gate.
The last completed full CI is `34400256735` for `221e66cd`.
No worker engine update, release, real task decision or BFG message was sent.

### September 9 terminal controls deployment

`221e66cd` is live; full CI `34400256735` passed. Evidence in
`/tmp/swarm-ui2-deploy.G5GcQu` confirms identical worker identity projections,
twelve running session/provider identities and unchanged engine PID/start time.
The new build is healthy and visible in Edge. The browser does not expose native
provider conversation IDs through this projection. Controls layout, optional
storage refusal and draft-state regressions are verified as described in [93](93-ux-first-remaining-plan.md).
Desktop reload jumping, native mobile/picker acceptance and the direct-worker
Needs You reconciliation gap remain open; do not infer their completion from
this presentation batch. No release or engine activation was requested.

### September 9 UI-first package checkpoint

`eb931114` implements and deploys the first UI-1 presentation batch. Changes and
focused browser evidence are recorded in [93](93-ux-first-remaining-plan.md).
Normal development reload activation succeeded; health is good. All 34 exact
worker records' identity fields and twelve running session/provider identities
match before/after, as do engine PID 2947820 and start time. Evidence is in
`/tmp/swarm-ui1-deploy.N3wwzR`. Conversation IDs were not exposed by this list;
do not claim independent provider-conversation proof. No engine update requested.
CI `34396506702` completed successfully with web, linux-package, rust-audit and
Rust passing. Preceding CI `34391815461` also passed all jobs.

Live Edge confirms shorter decision cards and unclipped queue titles. The
operator's RCG Networks report remains an explicit UI-1/QUEEN-03 blocker:
four original requests still exist server-side as Pending despite direct worker
discussion and resumed activity. Clarification is currently missing as a distinct
non-resolving inbox action; typed-answer provenance and exact decision correlation
remain necessary. Do not close cards based on activity or inferred approval.

The existing read-only observation completed without being restarted: 30 samples
over 1,800 seconds, 12 sessions, engine PID 2947820 retained. Evidence:
`/tmp/swarm-maturity-deploy.KyeMzE7g/normal-workload/20260909T185605Z-live-samples.csv`.
API RSS ranged 63,963,136–92,442,624 bytes; terminal-host cgroup memory (including
workers) ranged 5,777,129,472–11,575,361,536 bytes. This observation is explicitly
`performance_acceptance: not_evaluated`, not a browser or long-session pass.

### September 9 approved deployment — worker continuity verified

After fresh operator approval, six commits through `7c648e4d` were pushed to
main, and the clean Linux clone fast-forwarded. The normal development reload
created `pre-v156-reload-7c648e4d88f0-20260909T183739Z.sqlite3` before requesting
the build. The updater completed with exit zero and live health reports the
version above, no degraded subsystems and no database-recovery requirement.

Snapshots in `/tmp/swarm-maturity-deploy.KyeMzE7g` preserve all 34 worker records
and compare exact IDs, names, providers, running flags and session IDs before
and after activation: all match, with twelve running sessions. Engine PID
2947820 and its September 9 12:37:54 Eastern start time are unchanged. The
worker-list response does not expose conversation IDs; nullable placeholder
fields in the comparison do not establish conversation continuity independently.

The new engine binary is installed but not activated; the runtime correctly
reports `worker_engine_update_required=true` and no protocol migration. No
engine maintenance, worker wake, terminal input, BFG worker communication,
customer-facing send or product release was performed. The separate Edge tab
authenticated and loaded Queues with the new runtime version after navigation.

Live resource evidence independently reports memory pressure and memory stalls
as Normal at 49.13-percent used and zero memory PSI, while combined machine
pressure is Advisory at load 9.11 on eight CPUs. This verifies the deployed
CPU/memory classification distinction, not long-session performance acceptance.
Edge also rendered only Compute load as the actionable check during a later
CPU-pressure sample, and automatically cleared the warning when measured pressure
subsided. The verification tab was then closed to stop its diagnostics polling.

CI `34390001518` passed web, Rust audit and package lifecycle jobs but failed one
API test (555 passed, one failed, three ignored). The package-return fixture
advertised `old-engine`, then incorrectly expected cancelling drain to bypass
the engine-version guard. The failure reproduced independently in the isolated
Linux tree. The fixture now explicitly covers both a matching engine returning
and an old engine retaining its unattempted promise; its child process is owned
until explicit cleanup rather than expiring after ten seconds. This is a test
correction, not a relaxation of production return admission. Final verification
and replacement CI must be recorded before counting the full CI gate passed.

### Queen staged-delivery handoff — live partial acceptance

Fictional task `01a08761-415f-7810-91e2-28ff5ce31356` progressed unattended
from Ready through Active and Review to evidence-based Completed, preserving
its worker session. Independent hashes matched committed and delivered artifact
bytes. However, Queen's requested Awaiting Release transition never occurred;
the system completed directly from Review. This is not a pass for the missing
release-handoff acceptance case. Exact events and verification limits are in
[91](91-engine-return-dependency-acceptance.md#staged-delivery-exercise--partial-not-awaiting-release-acceptance).

### CPU contention is not memory pressure (local, not deployed)

Live Edge diagnostics on September 9 around 13:43 Eastern reported about 65 percent
CPU wait and load 17.91 on eight CPUs, but also called 43-percent memory use and
near-zero memory stall pressured. A subsequent read-only Linux snapshot measured
CPU PSI avg10 34.52, memory PSI zero and load 14.53. Several short-lived Vitest
processes led the process CPU listing; these are point/lifetime process counters,
not a complete causal profile or browser CPU measurement. No process was stopped.
The live warning later cleared without intervention.

The API had reused combined CPU/memory pressure to judge memory footprints; the
UI also reused it for memory rows and the headline. Both failing-before regressions
now pass. Memory classification is domain-owned and independently exposed, with
unchanged thresholds. CPU-only pressure still defers automatic starts; older
responses do not invent a memory verdict. Quiet recovery is covered in the UI.

Verification: all 146 domain tests; API pressure (6) and runtime (45) filtered
runs, with overlap; strict all-target/all-feature domain/API Clippy; 27 related
web tests; TypeScript and production build pass. The existing terminal-chunk
warning remains. Logs are `cpu-memory-*.log` under
`/tmp/swarm-maintenance-admission.BsOnHB`. Separate Edge inspection of the full
fictional App with `machinePressure=cpu-only` verified the compute row leads while
memory remains neutral in the expanded checks. No native-device acceptance,
performance improvement or deployment is claimed. PERF-01/02 and broader DIAG-01
acceptance remain open. No BFG Admin communication or release occurred.

### Verified package rollback (local, not deployed)

Failed updates no longer claim successful rollback after ignored service, link,
database or health-check failures. Recovery holds the existing lifecycle lock,
confirms affected services stopped, verifies a staged copy of the retained backup
with the previous release, and checks restored health. API-only failure recovery
preserves the worker engine. Rollback is armed before unit installation as well
as health checking. Missing backups and refused stops remain explicitly partial.

The missing-backup regression failed against the prior script. The final isolated
Linux package lifecycle smoke passes, including that case, refused stop and failed
unit installation. Release-apply smoke and shell syntax checks also pass. Logs:
`/tmp/swarm-maintenance-admission.BsOnHB/rollback-final.log` and
`rollback-release-apply.log`. No production service or database was changed.
This closes a package-failure safety gap, not native maintenance admission or
whole-program acceptance. Publication still awaits fresh push approval.

### Durable failed and unconfirmed worker returns (local, not deployed)

Schema 158 records an exact attempt before a maintenance return contacts the
provider. Failed and unconfirmed outcomes retain their source promise outside
the automatic queue. Exclusive lifecycle ownership identifies abandoned attempts;
neither elapsed time nor an API restart silently authorizes another launch.
Supervisor, Queen and task-dispatch starts respect this hold. A confirmed binding
settles the promise transactionally, and stale replies cannot settle newer work.
Needs You now has a concise affected-worker card linking to recovery diagnostics.
No terminal content or raw provider error is persisted in the attempt records.

Verification: 713 persistence tests passed, including schema 157 upgrade, database
reopen, stale-result fencing, atomic binding settlement and content-free events.
The final API rerun passed 26 targeted tests: bounded return/API replacement,
Queen/task-wake refusal and the recovery suite. Strict all-target persistence/API
clippy passed. An incremental Rust compiler error required disabling incremental
compilation for the final rerun; this was not an application code workaround.
Sixteen UI tests, TypeScript checking and the production web build passed.
Edge inspection of fictional fixtures caught and corrected an inherited icon-grid
layout error, then verified readable desktop and 390px iframe layouts. This is not
physical Android/iOS acceptance. The standard terminal bundle-size warning remains.

This checkpoint has not been pushed or deployed while push permission is pending.
The live API remains healthy on 86100b8b with no degraded components. A read-only
check found engine PID 2947820, started September 9 at 12:37:54 Eastern, replacing
the older ledger baseline. This turn issued no production restart or deployment;
the initiator of that restart is not established here. No BFG Admin communication
or release occurred. OPS-01 remains partial: native settled-turn/background proof,
combined admission/return receipts, negotiated IPC/package activation and live
all-session maintenance/return acceptance are still required.

### Exact source records for engine returns (local, not deployed)

Schema 157 ties each existing bounded return promise to its actual immutable
source session. The authenticated drain-required preparation endpoint validates
the complete engine snapshot in one transaction and returns exact worker/session
pairs; it no longer silently excludes unbound sessions. Missing/ended/duplicate
identities and capacity or storage failures refuse without a partial promise.
Same-source retries preserve the original record; a stale snapshot cannot replace
a newer source. Explicit manual maintenance also records available active sources.
Legacy promises remain unknown rather than being backfilled from current state.

Focused tests passed for database reopen, retained records after source stop,
stale-source refusal, whole-request rollback, capacity, explicit cancellation and
schema-156 upgrade. The actual API fixture passed authentication/drain gates,
idempotent preparation, exact response identities and refusal of a live unbound
shell without stopping either fixture session. All 708 persistence tests passed,
as did nine targeted preparation/maintenance/return API tests and strict all-target
clippy for persistence/API. A lint-only test-helper extraction was followed by a
passing rerun of the preparation fixture and clippy. Tests used the isolated
`/tmp/swarm-maintenance-admission.BsOnHB` source, not the development database.
Live health remains `ok` on the same 86100b8b App/API build and engine PID 2271655.
No production migration, engine update, release or BFG Admin communication occurred.
This checkpoint remains local while push permission is pending.

### Engine maintenance admission core (not activated)

ADR 0085 now has an engine-library all-session transaction core. It freezes
membership and remote authority, acquires all stop/control guards before any
eligibility check, validates the exact running-session return set, refuses local
and remote owners or in-flight effects, and reports exact partial stop progress.
Refusal releases every hold; successful stops fence subsequent input. Seven new
focused tests, 141 terminal tests in the final rerun, 27 host tests and strict
all-target clippy passed in `/tmp/swarm-maintenance-admission.BsOnHB`. The unchanged
sustained-output test passed in the first full run; it was not repeated, and the
existing explicit process-profiling test remains ignored.

This does **not** close OPS-01. There is no production IPC/package caller yet.
Current providers cannot supply the required settled-turn/background proof and
therefore refuse admission. Exact durable return recording, input-to-completion
correlation, negotiated protocol activation, failed/lost-response recovery across
API/package replacement, and a real demo update/return journey remain required.
The provisional IPC addition was removed after the protocol-pin test correctly
required a migration: do not force a worker restart for an unfinished interface.
Protocol 16 and live package behavior remain unchanged. No live engine was updated.

### Component-owned hover and interview contrast

The earlier queue-title fix is present; it was not reimplemented. Rendered Edge
inspection found a remaining cascade defect: generic button hover outranked the
queue's transparent background, painting amber behind pale dark-theme text.
The measured foreground/background contrast was 2.25:1. Base hover now uses
zero-specificity state matching so component-owned backgrounds take precedence;
ordinary action buttons retain their honey hover. The same pass found interview
choices inheriting dark button ink when unselected and using page-background ink
when selected. They now use existing text/on-accent tokens.

Two regressions failed before the change; 121 style/decision/queue tests and the
production build pass after it. Real Edge rendering of the isolated full App
verified dark queue hover at 13.65:1 and retained honey task actions. A fictional
interview verified dark unselected choices at 12.62:1 and selected choices at
10.00:1 dark / 5.25:1 light. At 390x844 there was no horizontal overflow; a custom
answer submitted exactly into the local receipt. No Hive decision was resolved.
Viewport was reset, the owned tab closed and the harness stopped. This is rendered
desktop Edge evidence, not native Android/iOS acceptance. Development deployment
completed with healthy status and exact before/after equality of all 34 worker
records, twelve running session/provider/conversation bindings and engine PID
2271655 in `/tmp/swarm-component-contrast.yC9vaL`. The separate live Edge tab
loaded Queues, the exact new version and the new base-hover selector; the user's
live light theme was not changed. The tab was closed. CI 34369343017 remains in
progress; the preceding snapshot-ordering and documentation CI runs passed.

### Live navigation attribution and snapshot ordering

On September 9, a separate authenticated Edge tab at swarm.bfgsolutions.net
showed twelve loaded workers. A 32.782-second Queues observation consumed
0.489079 main-thread task seconds (about 1.49% of one core), including 0.161560
script seconds and 0.062450 layout seconds. Heap point estimates moved from
24,744,252 to 27,099,804 bytes. This short sample does not reproduce the reported
sustained CPU pressure or establish a leak/plateau. A separate 45.766-second CPU
profile covering six Tasks/Queues/Needs You transitions sampled 41.884 seconds
idle. Automation/native work, GC and app rendering share the remainder; do not
attribute the whole remainder to Swarm or claim normal-user input latency from
automation. No terminal input was sent. Profiling was disabled and the tab closed.

Inspection found an independently reproducible ordering defect: an event refresh
could finish after a confirmed command and replace the newer snapshot. A regression
failed before the fix. The shared owner now rejects overtaken refreshes so the
existing feed retries without advancing its cursor. All nine snapshot mutation
paths, overlapping refreshes, cancellation and recovery are covered; 98 targeted
model/feed/application tests and the production web build pass. ADR 0071 records
the ordering rule. This is not a claim that the race caused every stale queue or
CPU complaint. Development deployment completed successfully. Exact snapshots in
`/tmp/swarm-snapshot-order.lBpLXb` match all 34 worker records and twelve running
session/provider/conversation pairs; engine PID 2271655 did not change. Health has
no degraded components, and a separate Edge tab loaded Queues and the exact new
runtime version. The owned tab was closed. CI 34367317966 remains in progress;
this checkpoint does not claim its outcome or completion of the wider program.

### Restart warnings and remaining runtime dialogs

The busy-worker census no longer promises a lossless engine restart when no
worker appears busy. Settings, update confirmations and release notes distinguish
observed activity from maintenance-safe admission and explain conversation return
versus interrupted commands. Automatic all-session admission remains open.

Restart confirmation, release notes and broadcast now use the shared modal focus
owner. Destructive confirmation starts on Cancel; pending actions cannot be
dismissed. Broadcast preserves failed-send drafts, prevents editing during a send,
and asks before discarding a draft. Browser inspection exposed undefined broadcast
overlay classes; it now uses the existing dialog backdrop and bounded scrolling.
The fictional runtime-dialogs harness verified nested draft cancellation at
390x844 and restart/release-note readability at 390x480, with no horizontal
overflow and no Hive calls or worker messages. Viewport was restored and the
owned tab closed. This is desktop Edge viewport evidence, not real-phone acceptance.
All 152 web test files / 1,440 tests and the production TypeScript/Vite build pass.
The existing large terminal-chunk warning remains; this change does not claim a
resource-performance improvement or completion of the wider UX acceptance row.
Development deployment completed successfully. Exact before/after snapshots in
`/tmp/swarm-runtime-dialogs.ctJvxD` preserve all 34 worker records and twelve
running session/provider/conversation selections; engine PID 2271655 is unchanged.
Health is good. CI for 912cb83a and 400a023d (34356215924) passed.

### Review-return metrics — deployed with worker continuity

ADR 0093 adds content-free, build-linked review-return episodes keyed by exact
request ID, and answers only after the existing reply-to/assignee checks. This
preserves repeated follow-ups which the current per-task marker replaces. It is
not a task completion, review quality score or current waiting count. Four focused
Linux transaction tests pass, covering replaced requests, exact/conflicting replies,
rollback/recovery, expiry and bounds; eight web tests and the production web build
pass. All 701 persistence/migration tests pass. The expanded panel was checked in
the isolated Developer Dogfood fixture at desktop and 390px width; the page has no
horizontal overflow. Fixture browser metrics were brought up to the current eight
fields to restore this existing surface. Four private API tests and strict
`cargo clippy -p swarm-api --all-targets --all-features -- -D warnings` pass.
Development deployment completed successfully; CI 34359188725 passed. Exact
before/after snapshots in `/tmp/swarm-review-history-deploy.d5BEoM` preserve all
34 worker records and twelve running session/provider/conversation selections;
engine PID 2271655 is unchanged. The pre-update SQLite backup exists with mode
0600 (43,515,904 bytes). Health reports no degradation. The private history API
reports the new bounded review-return page with zero episodes since activation;
that is not historical zero work or live review-return acceptance. Broader
DOG-01 remains open.

### Matched renderer-retention experiment — keep production default

September 9 Edge comparison at 1438x959 used fifteen fictional terminals, real
WebGL and a fixed 81,333-byte ANSI snapshot (640 rows). Reproduce with
`pnpm --dir web run harness --port 5211`, then open
`/harness.html?surface=terminal-pool&gpu=enabled&history=large`. Visit workers
1 through 15, then repeat that order twice. Reload and repeat with the
five-renderer experiment enabled. No Hive calls or real worker input are involved.
The fixture refuses snapshots above 131,072 bytes; initial and resize snapshots
are tested to preserve identical bytes. Five fixture tests, TypeScript and the
production build pass; the existing terminal chunk warning remains.

Both modes kept one live WebGL context. The default retained 15 terminal instances;
the experiment retained five, with 40 evictions after 45 visits. All 30 cold
returns completed: p95 178 ms, maximum 179 ms, no pending, abandoned or failed
attempts. The slowest return comprised 43 ms setup and 136 ms connection through
applied state. This passes the synthetic timing threshold, not real-network
paint/input-ownership acceptance.

Content-free CDP Performance counters (thread ticks, no forced collection):

| Mode / checkpoint | JS heap bytes | DOM nodes | JS listeners | Cumulative task seconds |
| --- | ---: | ---: | ---: | ---: |
| 15 / populated | 23,108,176 | 1,124 | 1,455 | 2.747936 |
| 15 / return cycle 1 | 21,337,276 | 1,440 | 1,815 | 3.933692 |
| 15 / return cycle 2 | 22,008,756 | 1,492 | 1,828 | 5.123831 |
| 5 / populated | 22,605,876 | 656 | 595 | 2.758733 |
| 5 / return cycle 1 | 21,450,968 | 680 | 682 | 5.120946 |
| 5 / return cycle 2 | 20,725,104 | 892 | 889 | 7.883944 |

The 30 return switches consumed 2.376 task seconds retaining fifteen versus
5.125 retaining five; script time increased by 1.504 versus 3.483 seconds.
Final heap was only about 5.8% lower in the smaller pool. These point samples
include instrumentation and automation, variable inter-action gaps and normal GC;
they are not total browser/GPU memory, a leak finding, matched-duration CPU
percentages or a production soak. In particular, fewer retained renderers did
not demonstrate lower switching CPU. Keep the production default unchanged;
PERF-01/02 still require live matched workloads and sustained resource evidence.
Profiling was disabled, the experiment stopped and the owned tab closed.

No row labelled partial is a program-completion pass. Links identify the evidence
or implementation boundary to inspect before changing it again.

| Requirement | Established evidence | Remaining acceptance or implementation |
| --- | --- | --- |
| PERF-01 | Process attribution, one-hour mostly-idle server observation, bounded output bursts; [87](87-live-resource-and-restore-checkpoint.md). | Matched fresh/aged 10–15-worker interactive workload, tab CPU attribution, instrumentation overhead; slow navigation/event-entry tails remain unresolved. |
| PERF-02 | Bounded controller experiment and output handling; task reader now bounds/cancels preview reads. | Decide warm-pool policy from equivalent workloads; do not adopt it from a single good cold-return run. Prove resource plateau and no missing output under normal use. |
| DIAG-01 | Browser/server surfaces and bounded content-free capture exist. | Correlated fault attribution and consequence routing across browser/network/provider/server; unsupported measurements must remain unknown. |
| DOG-01 | Private revision-linked browser history and bounded Queen explicit-finish history; ADRs 0063/0091. | Review yield/returns, duplicate questions, recovery/false-alert and update convergence comparisons; finish history alone is not task productivity. |
| TERM-01 | Engine-owned generation fencing; exact sessions preserved across App/API updates. | Repeated real Android/iOS desktop handoff, suspension/reconnect and uncertain-input journeys. Desktop reload jumping is not fully accepted. |
| TERM-02 | Operator accepted Android native AskUser questions 2/3 on September 7; renderer regressions recorded in [47](47-maturity-remaining.md). | iOS acceptance and current-build repeated redraw/scroll/replay checks; selection correctness alone is insufficient. |
| MOB-01 | Existing composer, draft and failed-submission protections retained. | Real keyboard/dictation, suspension and immutable-session draft recovery on both mobile platforms. |
| MOB-02 | Attachment lifecycle fixes and tests recorded in [47](47-maturity-remaining.md). | Camera/gallery selection through shared-file receipt and provider reference acceptance on Android/iOS; desktop task-image previews do not prove this. |
| QUEEN-01 | Autonomous two-demo dependency journey, routine documentation, executable-code/no-deployment and actual isolated local deployment settlement; [91](91-engine-return-dependency-acceptance.md). | Queen-owned Awaiting Release handoff and worker-first escalation journeys; the local Review-to-system-completion proof does not establish production rollout or fixture crash recovery. |
| QUEEN-02 | Engagement guards and bounded recovery delivery; normal same-task idle recovery completed autonomously in the isolated demo, retaining its exact session/conversation; [91](91-engine-return-dependency-acceptance.md). | Protected-input, background-work and failed-recovery escalation journeys remain; no real-project terminal experiments. |
| QUEEN-03 | Authenticated composer source records and exact-ID reads exist. | Direct-terminal/AskUser authored-answer capture and exact decision correlation remain unwired. Proposed source-to-deferral linking awaits the operator's answer. |
| QUEUE-01 | Owner grouping, worker grouping, structural prerequisites, exact held-briefing reasons, calendar reassessment journey. | Prove owner/reason consistency through remaining handoff/review/operator-resolution paths. Text-only deferrals without authoritative links remain a real evidence gap. |
| ATT-01 | Concise custom-answer UI and resolved/pending decision projection exist. | Direct worker answers must reconcile the same Needs You item without duplicate questions; depends on QUEEN-03. |
| ATT-02 | Runtime diagnostics/update/recovery entry is rendered; no new alerts for recovered evidence. | Complete fault-to-consequence and quiet-resolution acceptance, including normal-user feedback versus development repair. |
| PRES-01 | Schedule/DST/desktop-return policy and persistence tests exist; live locked desktop observed as Reachable. | Real scheduled/manual Night Watch, mobile non-dismissal and desktop dismissal. Live schedule is unset; do not change overnight policy merely for testing. |
| REC-01 | Planned engine replacement returned all twelve workers to exact conversations; [91](91-engine-return-dependency-acceptance.md). | Full chosen-conversation switch and missing-context safe/continue/fresh ladder with real provider evidence, plus failure/cancellation journeys. |
| REC-02 | Isolated corruption containment, backup/restore and package failure drills recorded in [87](87-live-resource-and-restore-checkpoint.md) and [47](47-maturity-remaining.md). | Retain this acceptance; no reason to corrupt or restore the live Hive. |
| OPS-01 | App/API continuity and one planned engine-return journey verified; resource admission guards exist. ADR 0085 engine-library all-session core passes isolated concurrency/failure tests. Local schema-157 exact source records and schema-158 failed/unconfirmed return reporting are implemented and tested. | Native input/completion/background evidence, combined durable-record/admission receipts, negotiated IPC/package integration, deployment of the local return changes and live all-session admission remain unimplemented/unaccepted. Rolling provider/tool freshness and pressure-to-resumption acceptance remain. |
| PROV-01 | Opt-in framework, host availability and Night Watch exclusions checked; [48](48-provider-acceptance.md). | Required provider journeys remain distinct from framework acceptance. Unavailable alpha CLIs stay unavailable; only the builder promotes providers. |
| UX-01/P6 | Approved visual direction retained; focused rendered queue, runtime, support and task-reader checks. | Coherent complete desktop/mobile accessibility, empty/error/offline and shortcut journeys. Optional return briefing still requires its mockup gate. |
| BFG Admin integration | Actual development-Hive text intake/receipt/reload retention accepted; [86](86-support-ui-acceptance.md). Admin readiness independently confirmed at e6a2167 on September 9. | One Swarm-UI fictional attachment submission and the existing fictional task's completion remain approval-gated below. No Admin worker communication is authorized; no customer reply is authorized by task closure. |
| P7 | Operator dogfooding plus bounded server/demo observations. | Integrated current-build workday/overnight/mobile evidence and operator acceptance. Narrow passing tests do not close this gate. |

## Next work, in dependency order

Current Swarm-owned change: attachment-revision sizing fence validated by a
failing-before regression, 1,435 passing browser tests and the fifteen-worker
synthetic Edge journey (ADR 0062). Development deployment completed; exact
before/after worker/session/conversation and engine-PID snapshots match in
`/tmp/swarm-attachment-sizing.LW9sst`. Health is good and the public URL loaded
in a separate Edge tab, which was then closed. No engine activation or release.
This
does not close the real-device, aged-workload or remaining orchestration gates.

Admin's owner confirmed source revision `3a0f8bdbac5640776704af5c1922d679fab1d6c9`:
the public `/api/feedback/swarm-support/submissions` route is strict text-only JSON;
unknown attachment fields are rejected. The retained-email attachment route is
authenticated download-only and its reader token must never be placed in a Hive.
The Admin-owned bounded attachment contract has now been agreed in ADR 0080;
Admin implementation is in progress, with migration/storage permission review
required before deployment. Swarm schema 155 and its transport now preserve
reviewed file bytes and identity through the outbox. Authenticated local ingress,
review UI and browser retry-byte retention are now implemented and verified in
isolation; no live attachment route acceptance is claimed. The existing fictional request remains untouched and
dispatch still requires restored Admin sign-in.

1. Continue Swarm-owned orchestration, performance and whole-surface acceptance.
   Do not communicate with the BFG Admin worker without fresh operator approval.
   Keep its existing linked-task fixture and external activation gates; do not
   repeat coordination or passed paired tests while those gates are pending.
2. Resolve the pending scoped operator-source linking proposal, then implement
   exact authored-source/decision reconciliation with no implicit authorization.
3. Establish provider-native settled-turn/background evidence before automatic
   engine admission. The pending opt-in prototype proposal is not approval to
   change existing workers, replace their execution mode, or trust a Stop hook alone.
4. Finish comparable browser workload attribution and whole-surface UI acceptance
   while the above external decisions are pending. Do not repeatedly rerun passed
   image or dependency fixtures as a substitute for these missing gates.
5. Run the real-device and integrated acceptance journeys against the delivered
   candidate. Leave unavailable-device and operator-approval gates explicit.

The program is active, not achieved. Its remaining scope cannot truthfully be
reduced to a release checklist or a count of passing commits.

## Native attachment backend checkpoint — September 9, not deployed

### Current activation and existing-task gates — September 9

This supersedes historical Admin-deployment-pending statements below. Read-only
inspection found the current Admin contract in
`C:/projects/bfg-operations/.worktrees/admin-mail-contract`, not its older main
checkout. Its release evidence records deployed schema18/private attachment
storage and exact production replay. Independently, Admin's canonical
`/api/ready` returned `ok:true, build:e6a2167` from the Linux host. No certificate
validation was bypassed when the Windows client's revocation check failed.
Swarm's authenticated support status returned `configured:true` and
`attachments_supported:false`; no activation or submission was performed.

The existing fictional task `01a08652-9d8e-7793-96cb-02493c5bc022` remains Draft
and unassigned. Its current description explicitly requires leaving it unassigned
and recruiting no worker. The later Admin handoff instead points to the existing
Swarm Dogfood Contract worker for normal lifecycle verification. Neither external
instruction is a substitute for operator direction. That demo worker exists and
is running; no screen was read, input sent or assignment changed. The ordinary
operator exemption endpoint approves an existing no-deployment claim rather than
creating one, so it is not a shortcut to fabricate this task's verified completion.

Two concise operator approvals were requested: enable native intake and create
one fictional PNG/text report through Swarm's UI after readiness verification;
and use the existing demo worker for this one no-code task despite its original
unassigned instruction. Pending answers do not authorize customer mail, a new
replacement task, real-project workers, scope-credential changes or Admin worker
communication. Existing reports and the reserved linked task are preserved.

Schema 155 adds immutable ordered metadata and private file BLOBs in the same
transaction as the report. Existing text payload/destination/attempt identities
remain unchanged. Files and their metadata count against the existing 16 MiB
outbox cap; new work is refused at capacity without purging pending reports.
The parent manifest prevents missing rows from becoming a silent text-only send.
Confirmed local-copy removal cascades to its files, never central attachments.

The application saves reviewed files before network effects. The sole sender
uses the agreed multipart route for saved attachment reports, retains the same
IDs, hashes, bytes and manifest order across attempts, refuses redirects, and
never downgrades failures to text. Existing text-only routing is unchanged.
Admin remains responsible for bounded raster decoding, private object storage,
atomic publication and original-channel customer replies.

Isolated checks passed: two domain validation tests; all 695 persistence tests
before the additional independent-connection race test, which also passed;
19 focused API tests including HTTP multipart replay and process-owner uncertain
delivery recovery; four application boundary tests and strict all-target Clippy
across domain, persistence, application and API. The optional paired Admin test was explicitly ignored without
its fixture, not counted as acceptance. The local upload/review UI, populated
browser tests and real paired Admin attachment journey remain required.

### Local ingress and browser recovery checkpoint

The authenticated bounded multipart route and file review/retry UI are now wired,
not deployed. Four new ingress tests cover auth/admission before stalled-body
reads, exact atomic save/replay, invalid/duplicate/missing files and deadline
failure. All 23 focused API tests passed (the optional paired fixture remained
ignored); strict API all-target Clippy passed. The compiler first hit an incremental
fingerprint ICE; disabling incremental compilation completed the same checks
without changing unrelated source or deleting shared build caches.

All 25 focused browser tests pass, including bounded file-reader cancellation,
durable byte-copy recovery, cross-tab
overwrite/deletion refusal, storage failure before sending, explicit same-report
retry and refusal to clear an unconfirmed payload merely from a status-only key.
A broader run before the ArrayBuffer correction passed 150 files/1410 tests and
failed the two new recovery tests; the focused rerun after the correction passed.
Type checking and production build passed; the test-only IndexedDB implementation
adds no runtime dependency.

In Edge at 1465 x 1339 (document width also 1465), the real dialog retained both
fictional filenames and report text after a failed upload and full page reload.
Explicit retry reached Saved to Hive / waiting to send, not delivery confirmation.
A subsequent reload showed the empty form, confirming browser-copy cleanup.
The test used the no-proxy local harness and its fictional retry seed; it is not
proof of native camera/gallery selection, mobile layout, or a central Admin send.
The owned tab and harness server were closed after the check.

### Paired native file transport — September 9

The opt-in Rust acceptance test passed against BFG Admin's real routes/schema in
its loopback-only, memory-object-storage fixture. A new fictional text attachment
was frozen in the Hive outbox and accepted remotely. Dropping the Hive handles
before settlement, reopening SQLite and recovering the interrupted claim replayed
the same bytes and returned the same conversation, message and creation timestamp.
Changed bytes under that same submission key returned Conflict; the original
receipt remained replayable afterward. No reserved linked-task fixture, customer
data, production Admin state or email sends were involved.

The contract now explicitly requires nonempty files; domain and browser rejection
match Admin. Two focused domain tests and 15 focused web tests passed. This paired
text-file journey does not establish raster decoding, real camera/gallery input,
mobile layout, private operator downloads or production storage behavior.

Admin's owner supplied and root reviewed the bounded relay upload deadline and
persistent deletion-marker/conditional-create fence. Production schema 17-to-18
and feedback-only storage credential activation remain held for explicit operator
approval. Native file intake remains disabled while that gate is open. An explicit
default-off intake setting now permits installing the additive Hive storage and
unrelated App/API corrections without activating Admin or exposing a file picker.
Twelve support HTTP tests and strict API all-target Clippy pass, including refusal
before reading a stalled upload, preserved saved bytes, truthful disabled capability,
and ordinary text submission with file intake off. Sender recovery of existing
frozen reports is unchanged. Actual deployment/continuity remains to be verified.

### Terminal post-grant recovery correction

Two new regressions reproduced a terminal remaining at Connecting after either
an invalid socket URL or a synchronous WebSocket-construction exception. The
connection cleared its attach-attempt identity before those operations, so its
catch handler discarded their errors as if an obsolete request had failed.
The attempt fence now survives until socket setup finishes; failures use the
existing bounded-rate reconnect owner. Legacy-protocol refusal remains explicit.
Both regressions pass, including recovery through a new grant and canonical
snapshot with no replay of refused input; all 61 connection tests and TypeScript
checking pass. This is a demonstrated failure-path repair, not proof that it
caused the reported desktop redraw jump or every mobile reconnection delay.
The correction is deployed in the f5c30de8 development build below.

### Worker-preserving gated-intake deployment

The configured development updater completed f5c30de85f95 successfully; API PID
is 2436275 and engine PID remains 2271655. Exact sorted snapshots before/after
match all 34 worker identities/running/assignment records and all 12 session IDs,
provider selections and confirmed conversation selections. Swarm Next and D365
remain asleep. Evidence: `/tmp/swarm-intake-gated-update.atA9ld` on bgsdev.
Health is okay with no database recovery; support reports configured/running and
attachments_supported=false. No central Admin infrastructure, file-intake
activation, worker-engine activation, customer send or release was performed.
Local production web build passed. Earlier native paired-test CI 34332128576
passed; full CI for this final intake-gated revision remains separately pending.

### Shared dialog keyboard and draft safety

Three failing-before regressions demonstrated nested Escape closing both dialogs,
background focus escaping the active modal, and hidden/collapsed controls becoming
Tab endpoints. The shared hook now derives the top mounted modal from the DOM,
contains focus there, filters unavailable controls and includes disclosure summaries.
It owns and removes its listeners/marker without a retained stack or timer.
Unsaved-changes confirmation uses the same contract, with Escape meaning Keep
editing rather than closing or discarding the parent. Portal image zoom and support
draft confirmation have dedicated integration regressions.

All 152 web test files / 1,423 tests and TypeScript checking passed. In Edge's
isolated no-Hive task-preview fixture, Escape closed image zoom only and returned
focus to its thumbnail. A changed fictional title survived Escape from the discard
confirmation; the editor remained open and focus returned to the title. Viewport
and document widths were both 1465 pixels. No live task save/delete occurred.
The owned browser tab and fixture server were closed. This is desktop keyboard
evidence, not full mobile/screen-reader acceptance. The fccc31e2 development
deployment completed with API PID 2442360 and unchanged engine PID 2271655.
Exact worker/session/conversation snapshots match before/after in
`/tmp/swarm-modal-update.qMsHV3`; no worker was restarted.

A further failing-before application regression showed Alt+4 navigating behind
an open dialog when the event did not originate from a text field. Global
navigation now yields to the mounted modal owner and already-handled events;
closing the modal restores normal shortcuts. All 78 application, palette and
focus tests plus TypeScript checking pass. The a65e14b5 development update is live;
exact before/after snapshots in `/tmp/swarm-modal-shortcuts.W9RY1D` preserved all
34 worker records and twelve session/conversation pairs, with engine PID 2271655.
In the owned live Edge tab, Alt+4 on the open navigation dialog left Needs You
unchanged. Escape closed only the palette and returned focus to its opener.
The test tab was closed immediately afterward. No engine activation occurred.

### Executable-code orchestration acceptance

Task `01a08585-edab-7a02-a10a-0f3c593eb854` completed through ordinary assignment,
briefing, worker execution, code evidence, Queen review and approved no-deployment
settlement. No terminal nudge or task repair followed admission. Independent
inspection found only the two requested demo files in commit `18126b3199c7744620127cd54cd2d980857216d9`;
all twelve new tests passed. Durable events distinguish operator admission, demo
worker execution and Queen completion. See [91](91-engine-return-dependency-acceptance.md)
for the exact timeline and evidence boundary; this does not close deployment or
real-backlog recovery acceptance.

### Inactive terminal presentation cleanup

Two failing-before regressions showed that hidden views retained pending redraw
work and detached retained surfaces scheduled timers on global viewport events.
Fit/redraw scheduling now refuses disposed, detached and hidden surfaces;
visibility loss and rendering deactivation cancel their pending presentation work.
Visibility return still measures current dimensions. No worker, transport history,
input ownership, canonical parsing or stable-fit contract changes.

All 100 focused surface/controller/workspace tests, the full 152-file/1,425-test
web suite, TypeScript checking and production web build pass.
The isolated Edge WebGL fixture switched away and back with two retained renderers,
one attached and one inactive, and showed the intact fictional terminal. The
fixture tab and server were closed. This proves the lifecycle correction, not a
measured reduction in whole-app CPU or acceptance of aged production responsiveness.

The normal development updater installed 8145b902 successfully. App/API PID is
2454791; engine PID remains 2271655. Exact sorted before/after snapshots in
`/tmp/swarm-inactive-render.jSTPUx` match all 34 worker identities/running/session
bindings and all twelve provider/conversation selections. Swarm Next and D365
remain asleep. Health is okay; support file intake remains disabled. No engine
activation, central Admin infrastructure change, customer send or release occurred.
CI for 8145b902 remains separate from the passing local suite and live continuity.

### Queue execution mismatch visibility

The task-only Active projection hid work even when the roster explicitly showed
its assigned worker stopped or using a different immutable session. Two new
regressions failed before correction. These cases now remain in the recorded
owner's visible queue with a short stopped/waking or session-reconciliation reason.
Navigation counts use that same projection. Exact-session return clears the
exception; absent/unknown roster data and ordinary Resting activity do not invent
a stopped worker. No wake, reassignment, task transition or Needs You decision is
created by presentation. Existing exact recovery evidence still identifies Queen's
own check without changing execution ownership.

All 118 queue/application tests, TypeScript checking and production web build pass. Edge rendered the
fictional mismatch fixture at a measured 390px container/scroll width without
overflow, with both explanations visible. This is a narrow desktop rendering,
not native mobile acceptance. The fixture tab/server were closed.

Development deployment 75ea754c completed successfully with API PID 2463002 and
unchanged engine PID 2271655. Exact before/after snapshots in
`/tmp/swarm-queue-execution.wMii0s` preserve all 34 worker identity/running/session
bindings and all twelve running provider/conversation selections. Swarm Next and
D365 remain asleep. CI for the earlier inactive-render fix (34337026969) passed;
this newer queue revision's CI remains a separate pending check.

The separate native /resume acceptance attempt stopped before any input or worker
mutation: auto-review refused reading the demo terminal's current screen. Specific
demo-screen permission was requested; no alternate capture path was used. The
demo remains in confirmed conversation c4435eee-57b0-4546-b6ef-dc182080e7b5.

### Single-pass review presentation — deployed with worker continuity

A rendered regression measured three complete task/snapshot comparisons for one
queue refresh: checked waits, historical rechecks and investigations each repeated
the same canonical serialization. One memoized projection now performs that exact
comparison once and partitions the accepted results. It does not cache evidence
across refreshes, weaken same-second change checks, alter ownership or accept a
different authorization source. The regression now measures one comparison per
task/snapshot pair. This removes two-thirds of these serialization passes, not
two-thirds of overall browser CPU. Preview character allocation is also capped
at 194 UTF-16 units rather than expanding the full explanation; a failing-before
8,000-unit Unicode example now retains the exact 96-character preview and complete
expandable statement. All 138 related queue/application/selection tests and the
production web build pass, including stale/failed refresh and changed evidence.

The combined run exposed an existing test ordering race: the external roster
renders before the saved-selection effect finishes. The test now waits for that
observable write rather than assuming the heading establishes it. A new test
proves reload during that window rejects the stale session while retaining the
chosen worker, both during the gap and after its replacement returns. No runtime
selection or recovery semantics changed. The full web suite passes: 152 files,
1,430 tests. Development deployment completed successfully. Exact snapshots in
`/tmp/swarm-queue-projection.5lGqqa` preserve all 34 worker records and twelve
running session/provider/conversation pairs; engine PID 2271655 did not change.
The separate Edge tab loaded Queues with its owner sections intact and was closed
after inspection. This is not whole-program acceptance or a release.

### Deployment retry scope — deployed and continuity verified

A partial deployment retried with whole-task scope reproduced premature
Awaiting Release completion despite the saved receipt remaining partial.
ADR 0092 now rejects scope conflicts atomically in both directions and exposes
an explicit conflict rather than acknowledging an unsaved scope change. Exact
retries remain idempotent; a genuinely new whole-task receipt restores normal
settlement. The regression failed before the fix and passes after it. All 697
persistence tests and strict API Clippy checks passed on the isolated Linux tree.
No migration, engine activation, customer send or release is part of this fix.

Development update 4184ae2f completed successfully. Health reports the version
above. Exact worker and session/provider/conversation snapshots match in
`/tmp/swarm-deployment-scope.fKQm9U`; engine PID 2271655 is unchanged. This
verification closes this deployment checkpoint, not the overall maturity goal.
