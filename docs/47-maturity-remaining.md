# Daily-driver maturity: remaining delivery and acceptance

This is the current completion checklist for the approved scope in
`45-daily-driver-maturity-plan.md`. Implementation history is in
`46-maturity-execution.md`. A passing component test does not close a live journey.
The operator authorizes commits and direct development-Hive deployments; releases
remain operator-controlled. The overall goal was restored on 2026-09-05.

## Immediate live defects

### Live-host release retention repair (September 5)

The deployment unit's mount protections deny `/proc/2323268/exe` inspection
even to the same user. A transient unit with matching NoNewPrivileges,
PrivateTmp, ProtectSystem and ProtectHome settings reproduced that denial.
The same sandbox successfully queried the engine socket in 43 ms, reporting
the original `17c46d45b23e` release and eleven running sessions. Release cleanup
now protects that socket-reported version, independently of moved symlinks,
and defers deletion on unconfirmed identity with a three-second deadline.

Isolated `test-release-apply.sh` passed, including two consecutive unchanged-
engine updates (the live release is no longer previous), missing/unsafe identity,
unavailable status, failed status with valid-looking output, timeout, and later
successful reclamation. Its status fixture now reports busy/unreadable counts;
the previously missing fields correctly prevented its deferred migration from
completing. `test-package-lifecycle.sh` also passed. This repair is not yet live
and does not repair the old process's deleted executable mapping. The Contract
demo's new launch still lacks a provider callback; REC-01 remains open.

### Engine update and D365 conversation regression (September 5)

Development revision `17c46d45b23e` deployed without cutting a release. App/API
replacement initially preserved all eleven sessions. Authorized engine maintenance
then replaced the engine with build
`de19229db431dcf421552be59969a5bd86bc688f2a8e1621f8f1d80f2c8ae3e5`.
All eleven original worker IDs returned automatically, with no runtime errors
reported; API PID 2321565 and engine PID 2323268 were active and health had no
degraded subsystems. Restoration proceeded approximately one worker per thirty
seconds despite normal resource pressure. Recovery scheduling latency remains open.
Push to main bypassed four expected required checks; this is not CI-green evidence.

The operator reported D365 resumed the wrong conversation and corrected it using
`/resume`. Read-only live metadata establishes that startup attempted and restored
exact conversation `019ffdae-baf9-7012-9120-0ee870a733c9`; neither continuation nor
fresh fallback was used. Thus exact restoration of a saved ID did not establish
restoration of the operator's intended conversation. Pre-maintenance provider IDs
were not captured, so the point where that default became stale is unproven.

The same running session (`01a0723a-5232-7f93-a942-26f9607e9337`) subsequently
reported selection revision 2, conversation
`c4d71311-c854-4ac8-b637-542b4b280401`. The API's persistence-validated
`confirmed_selection` matched both revision and ID. This verifies the corrected
selection was saved for this binding, not just seen by the engine. No restart,
input, manual pin update, or transcript extraction was performed during diagnosis.
Do not close REC-01: trace stale-default provenance and exercise a real disposable
provider `/resume` followed by maintenance, verifying conversation identity before
and after rather than merely counting returned processes.

Read-only follow-up covered all eleven open workers: each provider startup
matched its exact recovery target with no Continue/Fresh fallback. Only D365
reported a subsequent paired selection (revision 2, durably confirmed). The
other ten retained startup selection revision 1. All freshness projections were
Current, which does not establish pre-restart intent: resumed transcripts can
become newest through use. No additional wrong selection was proven or cleared.

Code inspection found engine maintenance already uses retained context-preserving
stop, but provider-release restart and restart-all still used destructive session
cleanup directly. Both now use the same bounded preservation handshake before
releasing the worker binding or assignments. The new isolated Linux restart-all
regression passed: the latest selection commits before cleanup, and an unexpected
cleanup response leaves the binding retained. This closes two unsafe call sites,
not the still-unproven provenance of D365's stale default. Direct terminal-stop
adapter preservation remains to audit. No live worker was restarted for this test.
The complete isolated API library suite passed 466 tests in 113.73 seconds;
subsequent local edits only expanded test formatting. Rustfmt validation was
unavailable in the remote stable toolchain. No release or deployment of this
follow-up fix had occurred at that test checkpoint.

Deployment follow-up: `bbe2a75f8803` is now serving as development build
`1.5.0-dev-bbe2a75f8803-20260905160634-2345644`. Managed reload completed
successfully; API PID became 2346696 while engine PID 2323268 and its build ID
were unchanged. All eleven original session IDs matched the pre-update sample,
with no runtime errors and no degraded health. D365's revision-2 corrected
conversation remained persistence-confirmed after API replacement. Main push
again bypassed four expected checks; no release was cut. This validates app-only
deployment continuity, not a new end-to-end provider restart acceptance.

Direct terminal-stop follow-up: bound worker sessions now use the same retained
context handshake before binding/assignment release. Standalone terminals keep
their existing stop path; persistence errors cannot be mistaken for an unbound
terminal. The two isolated restart/direct-stop failure regressions passed, proving
selection commit precedes cleanup and a failed cleanup keeps the binding.
The first compile encountered a Rust 1.97.1 incremental fingerprint compiler
error; retry with `CARGO_INCREMENTAL=0` passed. This was a validation-cache failure,
not live service disruption. No provider restart was performed for these checks.
The complete API library suite then passed 467 tests in 108.72 seconds with
incremental compilation disabled. Local-only formatting expansion followed the
remote source copy; runtime behavior and tested assertions were unchanged.

Direct-stop deployment: development build
`1.5.0-dev-47cd75385c8f-20260905161700-2352005` passed health with no degraded
subsystems. Managed reload completed successfully, engine PID 2323268/build
identity stayed unchanged, and all eleven running session IDs matched their
pre-update identities with no runtime errors. D365's revision-2 corrected default
remained confirmed. Main push bypassed four expected checks; no release was cut.

Return latency inspection found the supervisor uses a thirty-second interval in
`main.rs` and exits revival after one successful or failed attempt per pass.
Per-worker revival already rechecks capacity and drain state under lifecycle
ownership. Any acceleration must retain those fresh checks, bounded work and
fairness for workers behind a deferred or failing candidate. No scheduling fix
has been implemented or accepted by the preceding deployment evidence.

Return scheduling follow-up (ADR 0077): the supervisor now considers up to four
actual owed-return attempts sequentially per pass. Each call retains fresh
lifecycle-locked admission, cancellation and drain checks; ordinary crash recovery
still attempts one worker per pass. A failed attempt no longer abandons the batch,
and policy/capacity-deferred promises are not consumed. The isolated five-worker
failure fixture passed: four attempts on the first pass, one durable promise left,
then the fifth on the next pass. No actual provider executable was configured,
so this is scheduling/failure evidence, not live startup or conversation acceptance.
The browser `/resume` journey currently awaits unlock of the separate Edge tab.
All 468 API library tests passed in 110.54 seconds with incremental compilation
disabled. The installed pinned `1.97.1` toolchain has rustfmt (the `stable` alias
did not); its reported formatting changes were applied locally after the test
source copy. Live batched return timing remains untested and this batch is not
yet deployed.

### Live lifecycle-helper deletion defect (September 5 afternoon)

`c94b5332a95d` deployed as `1.5.0-dev-c94b5332a95d-20260905162955-2359342`;
health remained clean and engine PID 2323268/build identity unchanged. Before the
isolated test, all eleven workers retained their sessions. Batch return latency
itself has not been exercised live. No release was cut.

The authenticated separate Edge tab became available. In Swarm Dogfood Contract
only, Claude `/resume` visibly failed its SessionStart callback because its hook
executable under release `1.5.0-dev-17c46d45b23e-20260905153337-2319994` no longer
existed. `/proc/2323268/exe` confirmed that exact binary was `(deleted)`.
This contradicts the claimed live-host release-pruning protection. Installed
package source includes the `/proc` guard; why that guard failed remains open.
Do not perform more app updates until this deletion risk is resolved.

Immediate mitigation restored only that missing `bin/swarm-terminal-host` from
`/proc/2323268/exe`; both SHA256 hashes matched
`09bb6036b24e1a305ef5de4a256a0d7bc189587e325bdb11424e2f8394254feb`.
This is an explicitly partial rescued release, not a valid complete rollback
bundle. It did not restart the engine or any worker. Existing providers' future
callbacks could then execute. Lost earlier callbacks cannot be reconstructed.

Demo acceptance created a second conversation with `/clear` and a harmless
`honeycomb-beta-20260905` marker. After returning to the original conversation,
restoring the helper, and using `/resume` to select the marker conversation,
engine revision 2 and persistence `confirmed_selection` both named
`c4435eee-57b0-4546-b6ef-dc182080e7b5`. Direct stop acknowledged, and waking only
demo worker `01a07193-7d79-7113-93f4-8e9a43db6964` created session
`01a07270-e63a-7241-9929-3d7132d908e0` with that exact target. The browser showed
the marker transcript restored. The other ten session IDs remained unchanged.

However, the new demo's hook path now literally ends in ` (deleted)` because
`std::env::current_exe()` still reports the deleted mapped inode even after its
pathname is restored. Its provider-start receipt is absent, so complete recovery
acceptance remains failed. A permanent fix must prevent deletion and refuse or
safely resolve non-executable helper paths; never claim callback success from a
visible transcript. Demo remains running in the marker conversation. The Edge
tab is retained for continued inspection. An early typing timeout produced a
partial unsent marker prompt; its visible contents were checked before Enter,
and no duplicate prompt was replayed.

### Disposed renderer fit cancellation (September 5)

Disposal previously checked only after font/frame waits resumed. Two regressions
proved the fit promise remained pending when fonts or animation frames did not
resume. Renderer lifetime now aborts its font wait and cancels/rejects pending
fit frames. Browser-owned font loading itself is not canceled; late font/frame
completion cannot resize the disposed surface. Ordinary hiding/detaching retains
its existing behavior and does not abort the worker or renderer.

All 70 surface/controller tests passed; the strengthened late-completion tests
and TypeScript check passed afterward. No browser stall root cause or broad
hidden-view recovery closure is claimed. This is local lifecycle cleanup, not a
deployment; waiting on fonts in a live renderer remains separate from disposal.

### Terminal-specific font readiness (September 5)

The fit path awaited `document.fonts.ready`, coupling terminal startup to all
page fonts. It now loads the configured terminal font for the cell-measurement
character W, then retains existing stable-frame and geometry-ownership checks.
Failed font loading proceeds to measured fallback rendering. The standard
[FontFaceSet.load contract](https://developer.mozilla.org/en-US/docs/Web/API/FontFaceSet/load)
supports scoped font loading and rejects failed loads. No new timer or retry.

Both regressions failed before the change: unrelated font readiness held forever,
with terminal-font success or failure. All 38 surface tests and TypeScript checks
passed afterward. Edge's real renderer pool fixture reached connected with one
attached renderer; the separate synthetic 36-column question fixture repainted
question two and restored its canonical buffer without question-one text.
This is compatibility evidence, not measured latency improvement, native AskUser
acceptance or attribution of the operator's 4–8 second stalls. Undeployed during
the fixed-build baseline; scoped-font pending time and real-device acceptance
remain part of the broader performance/recovery gates.

### Rendered mobile fixture acceptance (September 5)

Edge's separate isolated harness tab exercised local composer Send. A DOM snapshot
captured `Sending…` with all nine terminal keys disabled; the subsequent snapshot
showed an empty draft, restored keys, and the source-recording failure warning.
No worker received input. At an actual emulated 390×844 viewport, measured page
width and scroll width both equaled 390. Composer toolbar controls measured at
least 44×44 CSS pixels, and the warning remained readable in the screenshot.

The prerequisite Queues fixture at the same viewport displayed distinct operator
and Queen ownership/copy, wrapped long titles, and had no horizontal overflow.
Full-page capture timed out; a subsequent viewport screenshot succeeded and was
inspected. The Needs You demo likewise had no horizontal overflow and all measured
action buttons were at least 44 pixels high. Its worker recommendation, collapsed
details, and answer actions were visible. These are synthetic browser-emulation
checks, not live decision relevance, native AskUser, real-device keyboard/picker,
or OS suspension acceptance. The live baseline tab was preserved unchanged.

### Integrated frontend verification (September 5)

At local `c699afb3`, the complete frontend suite passed 1,141 tests across 130
files with one test worker, and the production TypeScript/Vite build passed.
Vite retained its warning about the terminal chunk exceeding 500 kB; this is not
a browser-runtime performance result. No live deployment or release occurred.

The complete dogfood suite exposed CI/local audit drift: CI calls the existing
registry-aware audit wrapper from `web`, while the local entrypoint called raw
pnpm audit. The entrypoint now uses the same wrapper and working directory.
Parity tests include shell-script commands and execute an isolated audit adapter
to verify the directory and failure propagation without registry access. All 11
dogfood tests passed, including audit failure followed by remaining checks.
These adapter tests do not constitute a fresh dependency advisory audit.

### Mobile submission key ordering (September 5)

An isolated regression reproduced toolbar keys interleaving between Send's
bracketed paste and its pending Enter frame. The composer now disables and guards
its terminal-key toolbar while that submission is pending; it never queues those
taps for later replay. Normal keys become available again after completion or
the existing disconnected-input cancellation. All 36 composer/draft/source tests
passed. This is a local input-ordering fix, not real-device AskUser rendering or
provider-consumption proof. No live worker received test input; deployment is held
during the fixed-build baseline.

A later read of the unchanged preserved baseline tab reported 41,241,364 heap
bytes, 3,509 nodes and 1,301 listeners. No forced GC or reload. These gauges still
do not establish a monotonic leak or close the operator's aged-session report.

### Pending-decision roster semantics (September 5)

Tracing Queen's `Awaiting you · 19h` label found that the API gives any pending
decision precedence over provider activity. Its age is the oldest pending
request's creation time, not evidence that Queen stopped for 19 hours. The local
presentation now says `Decision pending` when that durable timestamp is present;
provider-only awaiting-input status retains `Awaiting you`. Existing attention
state, engagement/runtime/sleep precedence, decisions and delivery guards are
unchanged. The new regression failed before the fix; all 26 focused worker
presentation tests and TypeScript check passed afterward. Not deployed during
the fixed-revision baseline. This is not proof that those decisions remain
relevant or that direct-terminal answer reconciliation is complete.

### Authenticated ten-worker switching baseline (September 5)

On unchanged live `6ea9c93f`, the separate Edge tab traversed ten already-awake
workers twice, returning to RCG Networks each time. Queen and sleeping workers
were excluded; no terminal input, Resume Here, deployment or reload. Each worker
heading was verified after navigation; this is not a measured input-ready or
paint-complete latency. Automatic foreground attachment behavior remains enabled.

First route: 50.524 seconds, 3.028% main-thread task time, 643.140 ms script,
87.945 ms layout, 88 layouts. Heap 29,716,856 to 38,979,852 bytes; nodes
1,872 to 2,762; listeners 586 to 1,026. Second route: 43.868 seconds, 2.183%
task time, 293.030 ms script, 74.751 ms layout, 60 layouts. Heap 31,830,200
to 51,605,084 bytes; nodes 2,755 to 3,509; listeners 929 to 2,081.
After at least 30 seconds without switching: heap 42,469,016 bytes, nodes 3,494,
listeners 2,754. No forced GC. Different starting gauges show natural activity
between intervals; do not subtract across them as one uninterrupted CPU window.

At a later idle follow-up, heap had fallen to 38,914,764 bytes and listeners to
1,105, with 3,495 nodes. No reload or forced garbage collection was used. The
listener decline weakens the accumulating-listener hypothesis: do not present
the earlier peak as a confirmed leak. The reduced second-pass task time does not
close the degradation report; longer observation and allocation/lifecycle
inspection remain necessary. Performance
collection was disabled and the same tab preserved for continued measurement.

### Authenticated fresh passive-browser baseline (September 5)

The separate Edge test tab became authenticated and showed 11 active workers on
live `6ea9c93f`. It viewed RCG Networks passively, explicitly reporting another
view owned control. No Resume Here, input, navigation or reload was performed
during capture. Supported CDP Performance counters measured 68.142 seconds:
0.388% main-thread task time, 69.363 ms script time, 14.347 ms layout time and
six layouts. JS heap used declined 26,801,664 to 23,334,036 bytes; DOM nodes
17,586 to 1,869 and listeners 1,100 to 384. No forced garbage collection.
These counters are not Edge Task Manager's whole-process CPU metric.

The overlapping server vmstat interval showed 93–97% idle CPU, no swap activity,
and one 2% I/O-wait interval. Performance collection was disabled after the sample;
the separate tab was preserved for follow-up. This establishes a fresh passive
view baseline, not an aged multi-terminal session or proof of a leak fix. Raw
Memory.getDOMCounters was unavailable through this browser capability; supported
Performance metrics supplied the content-free counts instead.

### Loaded-worker baseline, September 5 14:27–14:29 UTC

The operator reported ten or eleven loaded workers. Read-only API checks at
14:27:03 confirmed 11 running sessions, Queen enabled/running, and 53 actionable
items. Runtime remained healthy on `6ea9c93f`; no deployment or input injection.
Six five-second pidstat intervals averaged 4.40% CPU for API PID 2240717 and
3.43% for engine PID 2201959 (one-core percentages, excluding subprocesses).
API's largest interval was 22.2%; engine's was 8.6%. This was not sustained pressure.

A separate 14:28:34–14:29:04 interval included service cgroup counters and vmstat.
API accumulated 244,094,000 CPU nanoseconds; terminal-host including its service
subprocesses accumulated 5,373,761,000: about 0.81% and 17.91% of one core over
30 seconds. Both main PIDs were unchanged. Terminal-host cgroup memory changed
4,004,491,264 to 3,999,936,512 bytes; API 267,685,888 to 267,997,184 bytes. Excluding
vmstat's initial since-boot row, whole-server idle was 95–98%, with zero swap-in,
swap-out and I/O-wait in these intervals. This is short-window headroom, not proof
against later spikes or long-session browser degradation. Local Edge CPU and
authenticated browser measurements remain outstanding.

The offline browser-growth evaluator was also corrected: absent, invalid or
non-increasing samples produce inconclusive rather than a false pass. Empty
summaries use null instead of invented zero measurements. Any valid failed series
still fails the overall report. All six metrics/process-sampling tests passed;
the standalone browser harness itself was not launched.

### Performance baseline checkpoint before operator reboot

At 2026-09-05 14:17:36 UTC, read-only SSH checks found the development runtime
healthy on `6ea9c93f`, API PID 2240717 and engine PID 2201959. Server load averages
were 0.51/0.56/0.52; available memory was 28,091 MiB of 32,042 MiB. API service
MemoryCurrent was 229,060,608 bytes and terminal-host service MemoryCurrent was
2,166,284,288 bytes. These are service cgroup counters, not isolated renderer or
provider heap measurements. Cumulative CPUUsageNSec was 69,168,817,000 for API
and 1,347,873,675,000 for terminal-host; a single cumulative value is not CPU load.

The operator reported a slow local computer and chose to reboot before loading
the usual 10–15 workers. Browser testing, builds and deployments were held.
Post-reboot measurements must be labeled a fresh-machine baseline, not continuity
of the earlier browser session. Capture actual loaded-worker count and observation
interval with CPU deltas before comparison. Keep live revision fixed during it.
Six local commits (`29bfcf75` through `1160e61b`) remain unapplied to the live Hive;
the shared-domain ownership fix requires an engine-refresh deployment window.

### Local validation and interview-form correction (September 5 morning)

Fetched main: no new upstream commits beyond the deployed `6ea9c93f`. The five
local follow-ups through `a0166a7c` passed all 1,138 frontend tests and production
build; the existing terminal bundle-size warning remains. They are not deployed.

A further Needs You regression reproduced multi-select submission silently
discarding a selected but blank "Something else" answer when a preset option was
also selected. The form now requires completing or deselecting that custom answer.
The new regression covers whitespace, completion, clearing and deselection.
All 67 decision tests and TypeScript checking passed. This is a separate form
correctness fix, not closure of direct-terminal answer correlation.

### Latest cross-worker acceptance checkpoint (2026-09-05)

The isolated Queen-owned prerequisite journey completed. Source task
`01a07193-7d88-7cb1-888a-602a0e533cb3` requested cross-worker coordination;
Queen created and assigned upstream task `01a07199-fef1-76e0-b908-4ad528774904`
and authored its prerequisite edge (activity 6972). The harness did not create
that task/edge or resume the source. The new provider required one manual
first-run folder-trust confirmation, so this is not unattended onboarding proof.

Upstream committed `fe873a1dde6e6fd9dc9afbd5bc7bfd01d2c4dadc`. Queen resumed
the source at 13:19:20 UTC (activity 6978); the source committed
`728c67abb78bbd6f57046bbc50483987afdf5e2a` and completed through documentation-only
settlement at 13:21:02 UTC (activity 6982). Both fixture repositories were clean;
the consumer's recorded contract hash matched the actual upstream file.

These events preceded the new API process for `6ea9c93f` at 13:21:47 UTC.
Do not attribute this live success to that commit's new discovery response:
normal Queen coordination on the preceding build completed the journey.
The added discovery response has regression coverage, not a separately observed
live invocation. Earlier pending-journey notes below are historical checkpoints.

- [ ] Blocked tasks with a pending operator decision still derive next owner
  `blocked`, not `operator`. The prerequisite discovery regression exposed this
  existing domain limitation. The discovery read excludes pending decisions,
  but that alone does not fix owner grouping. Add domain/projection coverage
  and account for the engine's domain-source fingerprint before deployment.
  Local fix now derives Operator for blocked work with a pending decision.
  Answering returns completed-prerequisite work to Queen; withdrawing returns
  ordinary blocked work to Blocked. Neither changes task state. All 101 domain
  tests, 561 persistence tests before adding the withdrawal regression, and 20
  Queues tests passed. The final build passed the new withdrawal test and the
  decision-resolution projection test; strict domain/persistence library lint
  passed. Deployment remains pending an engine-refresh window, not API-only.
  The accompanying queue copy now respects operator ownership: completed
  prerequisites explicitly retain the pending human decision, and blocked-age
  text no longer overrides ownership with a blanket Queen attribution. All 20
  Queues tests and TypeScript checking passed. Edge inspection of the 390px
  fixture confirmed readable, distinct operator/Queen groups. The separate live
  tab remains at unlock; this is fixture evidence, not authenticated acceptance.

### Operator prerequisite editor checkpoint (2026-09-05)

Task actions now offer an audited prerequisite editor for blocked tasks and
tasks with existing links. It retains choices after refusal or uncertain network
responses, guards accidental dismissal and duplicate submission, and rechecks
eligibility when live task state changes. It does not start or stop workers.
Candidate rendering is bounded and searchable; server lifecycle/cycle checks
remain authoritative. Accepted responses refresh both open and loaded history.

The frontend suite passed 1,135 tests across 130 files, followed by all 48 task
board tests after adding the menu-to-dialog integration test. Production build
passed (the existing terminal chunk-size warning remains). Edge fixture visual
checks covered desktop and a 390-by-844 narrow viewport with readable controls
and no visible horizontal overflow. This is not Android/iOS keyboard acceptance
or signed-in live-Hive visual acceptance. Queen cross-worker orchestration and
the broader queue completion gate remain open.

The first server build of `8a9b7564` refused an unsupported TypeScript option
in the subsequently added integration test. The previous healthy installation
continued serving. Correction `5b461722` removes that option; the production
frontend build was rerun successfully on the corrected source before retrying
deployment. Do not treat the earlier build as validation of later test edits.
The corrected server reload subsequently completed successfully: health serves
`1.5.0-dev-5b461722f6e3-20260905121835-2214431`, with no degraded subsystem or
database recovery requirement. Engine PID 2201959 and its build identity stayed
unchanged. All 48 task-board tests passed again after the query correction.

- [x] Restore the browser task-list/settled-history split at the HTTP boundary.
  On `12ec47b4`, five localhost `/api/v1/tasks` reads returned 3,118,493 bytes
  each in 17.0–17.4 ms. The response contained 786 tasks, including 703 Completed
  and 36 Abandoned. `fetchTasks` uses that endpoint; its adapter calls the all-task
  service even though a board-specific service and separate settled endpoint
  already exist. Preserve agent history access and unresolved evidence visibility,
  but stop reloading settled history on each browser task event. Add an actual
  HTTP regression, not just a persistence projection test. Baseline captures
  contain counts/timing only, not task text.
  The HTTP adapter now calls `list_board_tasks`; all 461 API tests and strict
  API lint pass. The endpoint regression preserves open and unverified finished
  work, serves abandoned history separately, and keeps the agent all-task reader
  complete. Deployed on `7cac7f3c`: the same endpoint returned 47 tasks and
  237,740 bytes in five 3.45–4.39 ms localhost reads, versus 786 tasks and
  3,118,493 bytes before (92.4% less payload). This is endpoint evidence, not a
  claim that the browser's long-session degradation is fully solved.

- [ ] Queen's explicit `swarm_start_worker` must honor Night Watch provider
  eligibility as well as capacity. Its current adapter checks capacity only.
  Standing and per-run instructions also contradict the available start tool by
  saying there is no wake tool and recommending task-state rewinds to wake work.
  Reconcile the shared guidance with the actual tool and preserve task state.
  Local implementation now shares lifecycle guidance between standing and run
  briefings, refuses experimental Night Watch starts, and rechecks policy after
  acquiring lifecycle ownership. All 456 Linux API tests and strict API lint pass.
  The regression run also exposed wrapped delivery markers and a canonical-shell
  fixture that truncated long input. Exact markers now survive physical row
  boundaries without ignoring spaces or other text; the fixture consumes the
  complete briefing. Deployment and actual Queen acceptance follow the soak.

- [ ] Automatic crash recovery and post-update revival must honor resource
  admission without consuming attempts or clearing revival intents. Inspection
  on `754acd50` found `supervise_workers` and `revive_workers_owed_a_return`
  bypass the coordinator's resource check and can start several workers in one
  pass. Night Watch policy is checked, but that does not establish safe machine
  capacity. Extend the shared resource policy and test deferred-to-resumed work.
  Local implementation now gates recovery before attempt accounting and revival
  before promise settlement, with one supervisor recovery attempt per pass.
  All 453 Linux API tests pass on the final source, including one-attempt-per-pass,
  pressure-to-admission and retained-return tests. Strict API lint passes. The
  previous five-second stable-worker fixture now waits for explicit stop, avoiding
  expiry during parallel setup. Deployment is held for the fixed-build soak.

- [ ] Jira synchronization respects removed linked tasks and continues processing
  valid work. Recheck the recurring WWD reconciliation error after deployment.
- [x] Unchanged Queen/outcome deferral implementation stops producing broad
  control-room refresh events; delivery ownership and failure/recovery events
  remain. September 5 reinspection confirmed deployed/local source SHA-256
  equality for `task_outcomes.rs` and `queen_conductor.rs` on live `6ea9c93f`.
  The existing persistence binary passed four `held_` tests, including ten quiet
  retries followed by delivery/recovery publication for both families. This is
  not a fresh rebuild: these two source files are unchanged since that binary's
  earlier validated build. Live health was ok with no degraded subsystem.
  A 15:16:15–15:16:19 UTC event-feed sample returned one `tasks_changed` event,
  no reset. It does not attribute that event or close broader refresh-churn and
  workload-overhead acceptance under PERF-01/02.
- [x] Reconcile the five-minute follow-up cooldown with first task delivery.
  Two live demo tasks delivered in 5/3 seconds from creation, 102 seconds apart,
  and completed automatically. Follow-up pacing and engagement guards remain.
  Representative multi-worker performance remains under PERF-01/02 below.
- [ ] Task status distinguishes waking, briefing delivery, and actual active work.
- [ ] A blocked Queen has an actionable escalation path when she cannot recover;
  her own blocked input must not make that escalation impossible.
- [ ] Queues shows the known blocker, next owner and recovery action. Generic
  "quiet moment" copy is insufficient when an exact reason is known.
- [x] Verified code tasks that intentionally require no deployment have a truthful
  completion path, without false deployment claims or routine operator write-offs.

## Remaining original outcomes

- [ ] PERF-01/02: comparable fresh and long-session measurements with 10–15 workers;
  responsive input/output bursts; resource plateau; measured warm-pool decision.
- [ ] DIAG-01/DOG-01: server/Queen metrics, correlated incident explanations,
  collection overhead, build comparisons and actionable consequence routing.
- [ ] TERM-01/02: real-device handoff, background/return and multi-question AskUser
  readability. Synthetic renderer checks do not close Android/iOS acceptance.
- [ ] MOB-01/02: camera and gallery delivery, keyboard behavior, draft retention
  and interrupted picker recovery on installed mobile clients.
- [ ] QUEEN-01/02: routine verified settlement, worker-first judgment, safe kicks,
  and bounded recovery without unrelated task interruption.
- [ ] Operator evidence/ATT-01: direct worker answers reconcile corresponding
  decisions; cards, counts and notifications agree; recovered problems disappear.
- [ ] QUEUE-01: dependencies, owner, precise reason and next action agree through
  assignment, handoff, review and recovery.
- [ ] PRES-01: real desktop lock/return, Reachable and scheduled/manual Night Watch.
- [ ] REC-01: explicit conversation changes and missing-context fallback through
  updates/shutdown; ordinary demo sleep/wake is already proven, not the full ladder.
- [x] REC-02: corruption consequence routing and write/dispatch containment.
  Real isolated API/SQLite restore and package failure-path drills now pass.
- [ ] OPS-01: rolling update convergence including Queen, tool freshness and
  automatic work admission after machine pressure eases.
- [ ] PROV-01: capability checklist, experimental opt-in and unattended gates;
  retain builder-only promotion and explicit provider selection.
- [ ] UX-01/P6: review the pending composition, complete coherent desktop/mobile
  surfaces, accessibility, shortcut behavior and optional return briefing.
- [ ] P7: representative workday and overnight/mobile soak with recorded limits.

## Evidence from the September 4–5 demo run

### Fifteen-worker browser lifecycle check (September 5)

Expanded the isolated pool fixture from eight to fifteen synthetic sessions and
exposed attached/inactive/evicted counts beside retained renderers. In Edge, two
complete ordered passes retained 15 renderers (one attached, 14 inactive).
Replacing the selected session still retained 15. With the optional five-renderer
pool enabled, a complete pass retained five (one attached, four inactive), with
25 cumulative evictions including the initial trim. A later cold return retained
five with 26 evictions. The experiment was turned off afterward.

This is lifecycle evidence, not a heap plateau or real-worker performance proof.
The fast switching pass interrupted 14 of 15 cold attempts; its one completed
sample measured 3,977 ms. An isolated subsequent return completed, leaving two
samples and the same maximum. That is insufficient and unfavorable evidence for
adopting the pool against the 500 ms target. Keep it off by default. Investigate
foreground/layout/restore timing with controlled completed returns before drawing
a causal conclusion; transport here is in-memory and rendering is the harness's
DOM renderer. TypeScript checking passed. PERF-01/02 remain open.

Follow-up inspection found canonical snapshot completion always awaited another
multi-frame fit for an owning foreground view, even if viewport dimensions already
matched the restored grid. The controller now skips only that redundant fit and
resize echo; mismatched/unknown metrics retain the existing guarded fit path.
All 242 terminal tests and TypeScript checking passed, including the new exact-match
regression. This proves removal of redundant work, not the cause or resolution of
the four-second sample. The fixture now reports visibility/focus with retention;
a fresh check reported visible and focused, which does not retroactively establish
foreground conditions for the earlier run. Controlled timing remains outstanding.

### Bounded submission observation and wrapped-message recovery

The stranded-message baseline used contiguous matching while normal delivery
already allowed CR/LF row boundaries. A regression reproduced its refusal of an
exact wrapped identity; the shared matcher now recognizes it while refusing
missing spaces, intervening text and a later operator draft.

Inspection also found that continuous unsent-input redraw skipped the acceptance
deadline through an early `continue`. Submission observation now owns a ten-second
timeout around the entire operation, including IPC reads. Timeout returns
Uncertain and does not replay input. Socket-level regressions cover continuously
changing sequence numbers and a host that accepts a request but never replies.
All 464 Linux API tests and strict API all-target lint pass. This bounds submission
observation specifically; it is not evidence that every other IPC phase is bounded
or that the full Queen workflow is accepted.
Deployed as `5ccaaed3`: the normal reload completed successfully, health reported
no degraded subsystem or database-recovery requirement, and the engine build
identity remained unchanged.

Follow-up local implementation bounds the remaining delivery transport calls:
baseline, paste acknowledgement, Enter acknowledgement, render reads and the
uncertain Queen recovery read each use a ten-second IPC deadline. A baseline
timeout defers as unknown provider state before any input; timeout after input
returns uncertainty, never permission to replay. The existing whole submission
observation deadline remains. The socket-level baseline/paste/render stall
regression passed and verified no subsequent request after the timed-out phase.
The full 465-test API suite and strict API all-target lint passed. Deployment
remains pending alongside the shared-domain ownership fix; this does not claim
every IPC user in the app is bounded.

The next real workflow drill is task `01a07193-7d88-7cb1-888a-602a0e533cb3` in
the existing disposable workflow fixture. Normal assignment delivered it and
it reached Active. A new sleeping `Swarm Dogfood Contract` worker owns the
disposable `contract-fixture` repository. The consumer is instructed to request
Queen-owned upstream task creation, assignment, explicit prerequisite recording
and eventual resumption. The harness does not create an edge or upstream task,
resolve a decision or complete work. Re-running the script inspects the same
task instead of duplicating it. Actual Queen completion is still pending.

Queen subsequently created upstream task `01a07199-fef1-76e0-b908-4ad528774904`,
assigned it to worker `01a07193-7d79-7113-93f4-8e9a43db6964`, and recorded the
prerequisite. Its new provider stopped at folder trust for the empty fixture;
the test operator selected and confirmed trust explicitly. This is first-run
setup assistance, not autonomous onboarding acceptance. The upstream completed
and the downstream projection changed from Blocked ownership to Queen while
remaining Blocked, as required. Queen-owned downstream resumption is pending.

### Explain Queen's known pacing without adding attention

During this drill a queued Queen review had no waiting reason despite a current
delivery pacing timestamp. Status reads now reuse the delivery gate's exact
persistence helper and report the pacing hold, with existing engagement/takeover
and sleeping precedence. No scheduling interval or delivery policy changed.
Queues renders the reason without creating a task row or increasing Needs You;
it disappears when the review leaves Queued. The empty queue no longer claims
nothing is waiting while this known hold exists.

The 32 Queen conductor and 49 worker tests passed, including exact cooldown
expiry and engagement precedence. Strict persistence library lint passed. All
1,137 frontend tests passed, and production build passed before the final
fixture-only addition; final TypeScript checking passed. Edge fixture inspection
covered the 390-by-844 queue layout, not authenticated live or native-mobile
acceptance. This change is not yet deployed.
It subsequently deployed successfully as `99aa11c8`, with healthy status and
unchanged engine build identity.

### Make completed prerequisites discoverable to Queen

After the upstream completed, Queen finished review run
`01a07194-6b96-7870-9e8c-cdc618540340`, but the consumer remained Blocked with
next owner Queen. Inspection found no explicit ready-prerequisite population in
`swarm_list_coordination_attention`, despite the changed review fingerprint.
The existing tool now returns `prerequisite_ready` tasks, a truncation flag and
the guarded next action. Tool inputs and metadata are unchanged; this is an
additive response, not a new tool requiring provider schema refresh.

The bounded persistence read filters before its 64-task limit and excludes
future block dates, pending operator decisions, removed/locality-mismatched
tasks and incomplete prerequisites. It does not transition or notify workers.
All 13 prerequisite tests, the actual Queen-only MCP response/refusal test,
all 464 Linux API tests and strict API lint pass. Live downstream resumption
after this fix remains to be checked; no manual task transition was applied.

### Explicit prerequisites — deployed API drill; Queen journey still open

ADR 0076 introduces bounded, audited same-Hive prerequisite edges, distinct from
queue ordering. Queen and the operator may add/remove them; ordinary workers
cannot. New edges require already-blocked work and never rewind or stop a worker.
Local Ready/Active transitions and automatic briefing/wake admission respect the
current prerequisites. Completed prerequisites change Queen's review fingerprint
and next-move projection without silently resuming blocked work. Removed or
abandoned prerequisites remain unresolved and visible.

Schema 139, shared application commands, operator HTTP and Queen MCP adapters,
and a shared Queues/task-card display are deployed on `7cac7f3c`. Tool discovery is
revision 18; existing provider sessions must refresh before they can use the new
tool. The database upgrade fixture catalog and tool fingerprint were corrected
after full regression runs caught their omissions. A second-Hive fixture was
also corrected to respect one Hive per operator and local-only reads. All 460
Linux API tests, all 557 persistence tests, and strict API lint now pass.

Domain (100), application (30), and frontend (1,126 across 129 files) tests pass;
TypeScript checking and the production frontend build pass. Edge verified a synthetic full-App
Queues prerequisite link opens and focuses its actual target task card, and a
390-by-844 layout fixture renders dependency states. These are local fixtures,
not live Hive/Queen or native-phone acceptance. The final frontend rerun includes
task-card linkage and removal of premature Resume/Start actions. Shared links were
visually corrected to remain text links on the task board, with mobile-sized touch
targets. The installed API drill passed persistence, identical retry, cycle and
premature-resume refusal, abandoned-upstream visibility, explicit removal, and
ordinary resumption. Its two unassigned demo tasks were abandoned with audit
history: consumer `01a07163-c3e9-7ca0-8c67-35fa61289450`, upstream
`01a07163-c3f8-7260-baa5-37bdb51554e3`. No real project task was altered.
Remaining gates: isolated real Queen coordination and representative populated-graph
projection overhead. Operator-facing dependency editing is covered by the later
checkpoint above; signed-in live acceptance remains open. QUEUE-01 stays open.

### Engine replacement and automatic return during this deployment

The linked `swarm-domain` changes deliberately changed the conservative engine
fingerprint. Contrary to the initial expectation of an API-only reload, the
idle reconciler replaced engine PID 2088305 with 2201959. Four sessions ended
and all four workers returned automatically in staged starts: Queen, Platform,
Nexus and Swarm Dogfood. No revival intents remained. The tool surface reported
revision 18, four current sessions, zero stale and zero unknown. This proves
this particular engine-return/tool-refresh journey, not every recovery path.
The reconciler's first service run was interrupted by the service swap; the
following run confirmed the current engine. Health remained/recovered ok, with
no database-recovery requirement or degraded subsystem. Avoid claiming future
domain edits are API-only; the fingerprint intentionally covers linked crates.
The live Edge tab still requires operator unlock, so authenticated browser and
native-device acceptance remain explicitly open.

### Direct attention navigation and keyboard tabs

Navigating to a decision while the inbox showed Activity did not reveal the
card. One navigation-owned focus request now selects Needs You, waits for its
card if data is delayed, reveals resolved history when explicitly requested,
and consumes focus once. Ordinary refreshes neither refocus the card nor leave
Activity. The tabs now expose their panel and support manual arrow/Home/End
focus movement; Enter/Space activates the selected view, so focus alone does
not trigger the asynchronous Activity read. All 74 focused App/inbox/activity
tests and TypeScript checking pass. Edge verified the full-App route from
Activity through quick navigation to the specific focused card, without a second
click, and the final full frontend suite passed 1,123 tests across 129 files.
This does not close provider-answer capture or native mobile notification testing.

### Recovery and Queen deployment checkpoint

Build `c93ea3d7` is deployed through the ordinary development reload. Health is
ok, `database_recovery_required` is false, no subsystems are degraded, and the
engine build identity remains unchanged. This deploy includes provider-label
preservation, capacity-admitted automatic recovery, and Queen's guarded start
contract. Local gates passed 456 Linux API tests, strict API lint and the earlier
1,121-test frontend suite. Actual pressure-recovery and Queen/provider journeys
remain separate acceptance items; deployment is not evidence of their completion.

### Fixed-build one-hour server observation

Read-only run `20260905T085228Z-live` completed 3,600 seconds on `fc7af9f4`,
with 120 samples and all four initial sessions retained. API PID 2144511 and
engine PID 2088305 stayed fixed. There were no dropped history bytes; retained
history grew 157,912 bytes. The samples span 3,586 seconds between first and last.
Across that interval API CPU averaged 0.30% of one core; the engine cgroup,
including its children, averaged 4.93%. Largest sampled interval averages were
2.74% and 16.05%, respectively, not instantaneous peaks.

API cgroup memory ranged from 13,295,616 to 99,512,320 bytes and ended at
88,375,296 bytes. Engine cgroup memory ranged from 1,177,960,448 to
1,267,396,608 bytes. This is continuity evidence, not a memory plateau or the
10–15-worker browser performance acceptance gate. Native mobile and browser
latency were not measured. Evidence is retained on the dev server at
`/home/bschleifer/.local/state/swarm-next/soak/20260905T085228Z-live-samples.csv`.
No matching hourly integrity-probe log was observed in the queried journal;
the isolated containment/recovery drill is separate verified evidence.

### Provider policy and truthful settings

The full frontend regression suite subsequently passed all 1,121 tests.

The provider acceptance template is now in `48-provider-acceptance.md`. Current
Night Watch exclusions were rechecked with the existing Linux API/persistence
test binaries: one final-submission gate test and three presence/wake/briefing
tests passed. Wake and briefing tests verify resumed eligibility without losing
queued work or spending attempts. This is not real-provider promotion evidence.

Settings previously labeled every non-Codex provider as Claude and omitted an
existing experimental binding from its selector. The correction preserves that
binding and its real label, including when renaming or searching for the worker.
Eighteen worker-settings tests and TypeScript checking passed; Edge verified
Gemini selected in the actual component fixture. The explicit experimental
opt-in/availability contract remains open. This UI correction is not deployed
yet: the fixed-build one-hour server soak must finish first.

### One queue row per known held task

Known held briefings now show their recorded reason and queued age inside the
task's existing owner row instead of repeating the task in a second list.
Coordinator-only briefings remain inspectable; confirmed delivery removes the
old reason even when a stale coordinator snapshot remains. The dedicated Edge
fixture verified the combined row. Thirty-two queue/briefing tests and the
58-test App/queue/fixture gate passed, along with TypeScript checking. This is
presentation deduplication, not completion of dependency and recovery routing.
The follow-up assignment check also rejects a former worker's held briefing after
reassignment or unassignment without hiding the task. Sixty-one App/queue tests
and TypeScript checking passed for that correction. The full frontend gate on
the preceding queue change passed all 1,117 tests.

### Runtime database containment

ADR 0075 adds a shared persistence latch after a confirmed integrity failure.
New database access and dispatch claims fail with a specific recovery-required
error; generic domain failures, busy and interrupted checks do not latch. The
API owns an hourly opportunistic probe plus an operator-authenticated check-now
endpoint with a shared single admission permit. Probe work runs off the async
executor and holds its permit through cancellation. SQLite progress is bounded
to one second, excluding uninterruptible kernel IO; a busy lock is skipped.
Health exposes the consequence without SQLite, and Needs You/runtime diagnostics
show the recovery notice without creating a stored decision.

All 546 persistence tests and 450 API tests passed, including shared-clone refusal,
healthy reopen, busy/interrupted probes, authenticated admission and monitor
shutdown. Strict API lint and 1,116 frontend tests passed. The dedicated Edge
fixture verified the narrow warning and its recovery disclosure. Build `981bd57d`
is deployed with healthy status, no degraded subsystems and unchanged worker-engine
identity. The extended real-API drill passed on that binary: confirmed runtime
damage refused a new task with 503, health reported recovery required, corrupt
startup was refused, and explicit offline restore retained the marker task and
cleared the consequence after reopening. Backup source and original damaged bytes
were preserved. Evidence: `/home/bschleifer/.swarm-recovery-drill-8v0wmv_4`, marker
`01a070b8-b943-7820-bb32-a51b854fc401`. Only isolated owned API processes were
started/stopped; the live Hive was untouched and remained healthy. The first
extended drill exposed a duplicate response-body read in its assertion, corrected
in `ed3c8cf2` before the passing run. Hourly detection remains opportunistic; this
does not claim continuous detection or real-device notification acceptance.

### Continuous four-session server sample

`observe-live-soak.sh` completed 600 seconds / 20 samples on `baa304ef`, with
four running sessions and unchanged API PID 2114060 / host PID 2088305. Across
the 573-second first-to-last sample interval, API CPU averaged 1.384% of one core
(maximum 30-second sample 2.292%); host cgroup CPU averaged 5.443% (maximum
sample 8.209%). API cgroup memory ranged 70,963,200–98,988,032 bytes and host
cgroup memory 1,159,593,984–1,178,288,128 bytes. These are cgroup counters, not
browser memory or per-process RSS. Retained history grew by 1,158 bytes and
reported no dropped bytes. No leak/plateau or burst-latency acceptance is claimed.
Content-free evidence is retained at
`/home/bschleifer/.local/state/swarm-next/soak/20260905T073502Z-live-summary.json`
and the matching samples CSV. Representative 10–15-worker/browser work remains.

### Removed Jira workflow drift

The removal guard ran after whole-batch status validation. A dismissed issue
moving to an unmapped remote status could therefore reject retained work before
the guard ran. Mapping validation now follows the dismissal check inside the
transaction. Retained unmapped work still rejects atomically. All ten Jira
persistence tests and strict API lint passed, including both removed-status drift
and rollback of an earlier valid import when a retained mapping is missing.
The full persistence suite subsequently passed all 544 tests.

### Compact attention and visible Reachable status

Needs You no longer repeats an introductory heading and explanation above its
cards. Its accessible section name now also survives switching to Activity.
At phone width the urgency badge no longer squeezes the question beside the bee.
The same Edge full-App fixture confirmed the desktop card still retains its
recommendation, context, optional note and answer controls. Reachable's dot had
been transparent because only the old Away selector supplied its color; a scoped
fallback restores the visible phone indicator without changing presence policy.
TypeScript and all 1,115 frontend tests passed. These are fixture presentation
checks, not closure of decision reconciliation or native mobile acceptance.

### Task briefing refresh churn

Task briefing claims and guarded deferrals now follow the quiet outcome-delivery
pattern: repeated internal retries do not emit broad task refreshes. Completion,
failure and crash recovery still publish. Assignment/briefing repair publishes
even if a busy worker prevents delivery, which the previous candidate-count
condition missed. All 25 dispatch regressions passed, including ten repeated
holds, final delivery/recovery and repair while busy. Strict API lint passed.
Commit `e0423a93` deployed through the normal development reload. Health reported
that revision without degraded subsystems; the terminal host stayed at PID
2088305. Representative held-briefing browser resource acceptance remains open.

### Queues badge, heading and narrow-screen readability

Navigation and queue rows now share a waiting-task projection: ordinary active
work is excluded, task identities count once, and stale coordinator observations
cannot resurrect known closed or resumed tasks. Coordinator-only tasks remain
visible. The badge explicitly describes waiting tasks, not message counts.
The full fictional App exposed a heading bug (Queen / Persistent terminal on
Queues); the header now identifies Queues and who has the next move.

The dedicated Edge fixture showed four waiting tasks versus five open tasks.
A 390px iframe of the full App exposed squeezed-out titles and overflowing
explanations. Phone rows now place the title on its own wrapping line; status,
worker names and blocker/review context wrap underneath. Before/after screenshots
show readable titles and no horizontal scrollbar. This is responsive fixture
evidence, not authenticated live-Hive or Android/iOS PWA acceptance.
Commit `750c5d5d` deployed healthy through the normal reload; terminal-host PID
2088305 was unchanged. TypeScript and all 88 focused tests passed.

### Runtime evidence freshness

The existing health poll updated the bundle notice but left the footer and
diagnostics at the initial response indefinitely. It now updates the shared
health snapshot (and attachment limit) while retaining identical state without
another render. No polling timer was added. A return/unavailable/recovery
integration test passes, and the complete frontend suite passes 1,113 tests in
129 files. This does not add runtime database integrity detection: that still
needs an owned failure latch and consequence path independent of SQLite.

### Offline restore implementation and real isolated drill

`restore-offline` now provides a recovery path that does not need an export from
the broken API. It verifies the chosen backup copy, confirms API stop, archives
the original DB/WAL/SHM, and starts only the API. Failed health leaves the API
stopped where confirmable, with damaged evidence retained rather than reinstalled.
The complete isolated package lifecycle suite passed, including unavailable
export, invalid input, concurrent restore refusal, unknown stop state, source
migration preservation, private archive permissions, three-archive admission,
failed health and refused failure-stop reporting.

`offline-restore-drill.py` then ran the installed `a6c2ef39` API and verifier
against newly created private state, without real Hive credentials, integration
configuration or a terminal-host connection. It created a marker task, stopped
the API, saved the database, corrupted only that disposable DB, proved API
startup failed, and ran the actual offline restore command. The restored API
returned the same task ID/title; backup SHA-256 stayed unchanged and the archive
matched the corrupted bytes. The process adapter served only API start/stop/state
requests; this was real API/SQLite evidence, not a real-systemd fault drill.
The API was stopped at drill exit. Private evidence remains at
`/home/bschleifer/.swarm-recovery-drill-kc0gz1rc`; the earlier refused fixture is
retained separately at `.swarm-recovery-drill-t2u_s0ha`. No restoration or
corruption was performed on the development Hive. REC-02 still needs explicit
corruption consequence routing and write/dispatch containment acceptance.

The normal file-backed open path now checks integrity before journal-mode or
migration changes, in addition to post-migration checks. The targeted regression
preserves a damaged schema-137 database byte-for-byte and does not migrate it.
All 542 persistence tests and strict API lint passed. Normal development reload
activated `21125e1b` with healthy API status and unchanged host PID 2088305. Runtime
`IntegrityFailure` still shares a generic unavailable response; a clear operator
consequence and runtime containment must not be claimed from this startup fix.

The engine update after `5972db76` was not stuck without a cause: its active
reconciliation timer reported one of four sessions mid-turn. No force restart
was requested. It subsequently converged automatically to engine `8d22d71b816d`
with PID 2088305. The demo worker returned running/resting in session
`01a07048-0f97-7e90-a60d-b770b46e4bbb`; settled tasks and the withdrawn decision
remained intact. This proves that deferred update/return, not all provider
conversation and Queen freshness acceptance paths.

### Current blocking context in Queues

Task projections now include the note on the current transition into Blocked.
The note disappears on recovery and a later empty blocking episode cannot reuse
the old reason. Queues shows short notes directly and long notes in expandable
previews, explicitly labeled as recorded context. A file-backed reopen/recovery
regression passed, as did 16 Queues tests and TypeScript checks. The dedicated
Edge fixture confirmed the visible note and existing collapsed active-work area.
All 541 persistence tests, 46 Queues/style tests, strict API lint and formatting
checks passed. The live EXPLAIN uses the task/sequence index, not a whole-history
scan. Normal development reload activated `5972db76` with healthy API status.
The isolated API task `01a07043-4539-7263-9361-d17400613d43` proved the note
appears, clears on recovery, does not return on an empty reblock, and remains
absent after abandonment. The test created no assignment or worker work.
`check-blocker-context.sh` reproduces the sequence in the demo workspace.
Three task-route reads remained 17.5–17.8 ms. Actual host PID 2058454 and demo
session `01a0700b-9d8f-71d2-a44c-1cce6b27d2dd` were unchanged; the API advertises
the new engine build while host-current still points to the earlier package.
Do not confuse API activation with complete rolling-engine convergence.
This improves blocker visibility but does not close dependency ownership,
recovery-action integration, or authenticated live visual acceptance.

### Measured task projection scans

The live schema-137 database has 832 tasks and 6,940 activity rows, with no
task-activity index. EXPLAIN shows a correlated full activity scan per task,
plus scans for pending decisions and outcome delivery. On a disposable copy
of the verified backup (6,936 activity rows), the unchanged worker-evidence
lookup returned 769 before/after: three baseline runs took 1.200–1.219 seconds
and three indexed runs took 2 ms. The decision/outcome probes returned the
same 3/9 totals and fell from 137 ms to 1 ms. These are isolated query timings,
not whole-application speedups. `benchmark-task-history.sh` reproduces them
without indexing or modifying the live Hive.

Schema 138 adds task/sequence history, pending-decision/task and task/state/
sequence outcome indexes. The file-reopen test preserves activity and checks
that the lookup plans use each index without temporary sorting. All 540 Linux
persistence tests and strict API lint passed. No blocker-text fields are added
in this slice.
Before deployment, three local `/api/v1/tasks` calls took 1.817797, 1.846777 and
1.838573 seconds. After normal development reload to `ac861860`, the identical
three requests took 0.021745, 0.022475 and 0.018136 seconds. Live schema 138
contains all three indexes. Health was ok with no degraded subsystems; terminal
host PID 2058454 and engine build identity stayed unchanged. This establishes
the task-route improvement, not end-to-end mobile latency or browser CPU.

The completed 600-second run `20260905T055555Z-live` collected 60 samples with
four initial sessions, stable API/host PIDs and no drop counter increase. API
memory ranged 19,345,408–57,544,704 bytes; the host cgroup (including workers)
ranged 1,230,163,968–1,306,046,464 bytes. API CPU averaged 2.37% of one core,
with a maximum interval average of 55.60%; host-cgroup values were 7.21% and
16.33%. This includes the three explicit API timing requests and overlaps the
separate SQLite-copy benchmark; it is not an undisturbed idle baseline.
The private CSV is `~/.local/state/swarm-next/soak/20260905T055555Z-live-samples.csv`.
This does not close representative 10–15-worker, aged-browser or mobile soak.

### Browser session retention and detached Queues

Successful session snapshots now retire inactive browser controllers for ended
or replaced sessions. Previously only local stop/reset/logout paths disposed
these renderers, so remote lifecycle changes could accumulate obsolete copies
with the warm-pool experiment off. Still-running warm views are retained; an
attached ended terminal remains readable across background/return until detach.
No worker stop is sent and drafts remain intact. The 100-incarnation test keeps
one renderer; Edge's synthetic fixture shows one after replacement, two after
switching to another live worker, and two after replacing that second session.
This is lifecycle evidence, not a measured production CPU or heap improvement.

Detached Queues URLs now parse consistently with all five other detachable
surfaces. The all-surface round-trip test reproduced the missing Queues branch
before the fix. All 1,103 frontend tests and TypeScript checks pass. These
browser-only changes deployed as `90dbcbff`, healthy with no degraded subsystems.
The engine retained PID 2058454 and build identity `490b3f3c...99ebf1c` through
this browser-only update.

Queues now keeps ordinary worker-owned Active tasks in a collapsed "Marked
active" section, separate from waiting counts. Queued/uncertain dispatch and
returned reviews remain visible. The label makes no provider-progress claim.
Edge verified collapse and expansion with synthetic work, and the regression
tests cover active-only queues and visible delivery exceptions. This does not
yet provide full dependency/blocker reasoning or live multi-worker acceptance.

### Current review obligation in Queues

Withdrawal revision `b1577fe8` is deployed: runtime health is ok, schema 137
quick-check is ok, and foreign-key check returns zero violations. The engine
identity changed to `490b3f3c203913aecd14e2f24fd5f6e4e38c9923dbb5f3f57bb3d39c099ebf1c`.
The demo's prior session ended at 05:29:30 UTC and its replacement started at
05:30:04; no revival intents remain. This is a measured 34-second interruption
and automatic return, not uninterrupted worker continuity. Queen's earlier
terminal tail contained an unsent paste placeholder; its ownership was not
established, so it was neither submitted nor erased.

At 05:32 UTC the normal Queen workflow completed the demo task on evidence
(`closed_on_evidence=true`, `closed_unverifiable=false`, no deployment fabricated).
Queen `019ff136-7a90-7631-bbc0-f95efd1df576` withdrew her own decision
`01a06f58-4526-76d0-adb4-858e731470fb` at 1788586336, citing her verified and
approved no-deployment claim and the corrected structural refusal. Resolution
action, operator identity, resolved time and decision delivery all remain null.
This closes the local-code completion and obsolete-request live round trip;
it does not close direct-terminal answer correlation or all Needs You/Queues UX.

The following entries retain the intermediate verification history.

Withdrawal's executable Linux MCP round trip now passes: an authenticated test requester
creates and withdraws a request, and Queen's full-ID read reports withdrawn,
verified=false and no operator answer. Revision 17's served-tool fingerprint
was derived and passes its discovery check. All eight related Linux API decision
tests pass, as do 96 frontend inbox/queue/control-room tests and TypeScript.
The queued-push regression proves a queued delivery is canceled before claim
after withdrawal. All 539 Linux persistence tests and all 1,094 frontend tests
pass, with TypeScript and strict API lint passing. The newest-schema registry
now includes migration 137; the Jira upgrade fixture retains the surrounding
Hive schema rather than modeling a database with only one table. Deployment
and real Queen withdrawal remain pending.

The populated migration now also reopens its file-backed store and checks both
withdrawal metadata and the existing resolved request, then verifies integrity.
The first broad API run passed 446/448: the two failures were old assertions
against the earlier pending cap and deployment-only Queen wording. Their
fixtures now exercise overflow through history and the explicit local-code
scope branch; all 448 API tests now pass on Linux and strict API lint is clean.
A verified schema-136 live recovery
copy is retained at `~/.local/state/swarm/pre-withdrawal.w0WAROdI/hive.sqlite3`.

The verification worker reran six tests, recorded the original code exemption
successfully, verified its stored unapproved state, and messaged Queen. Its
verification task auto-completed; the original escalation remained pending.
This verifies claim admission, not final Queen settlement or inbox recovery.

ADR 0074 withdrawal work is local and not deployed yet: a distinct state,
authenticated requester/Queen tool, non-approval history, and migration 137.
Thirty-three decision persistence tests passed; the separate old-schema rebuild
test preserved pending/resolved rows, deliveries and indexes. The stale-session
application test and 28 inbox tests passed. Edge's isolated fixture showed zero
pending requests and a clearly labeled withdrawn history card. Linux API/test
compilation and tool-revision checks passed. Live Queen recovery is still required.

Runtime `40c4f74a` is deployed and healthy. Queen's pending demo escalation
`01a06f58-4526-76d0-adb4-858e731470fb` confirms she checked the heartbeat commit,
six tests and mutation evidence but could not record a truthful shipping state.
The isolated worker was woken; verification task
`01a06fcd-5b2a-7813-91dd-abc306426f0f` reached delivered/Active through normal
guarded assignment. It will retry the original claim and ask Queen to reassess
the obsolete escalation. Neither the original task nor decision was manually
settled. Acceptance is pending the actual worker/Queen result.

The withdrawal correction is deployed as `d56993bd`, healthy with no degraded
subsystems. The following completion-policy change (ADR 0073) permits a code
no-deployment claim for Queen judgment without automatic approval or a fabricated
deployment. All 55 task-outcome persistence tests passed, including the new
claim/automatic-refusal/Queen-approval path. Live demo acceptance remains open.

Development reload verified `664abff3` healthy with no degraded subsystems.
The worker-engine identity has changed since the earlier baseline; this check
does not establish uninterrupted engine continuity across that interval.

Completion review follow-up found that a late approval could stamp an already
withdrawn exemption as approved, even though it remained ineffective evidence.
The approval update now requires a live claim. A regression covers all three
authorized approvers, unchanged withdrawn history, and a corrected new claim
that can subsequently be approved. All 20 completion-evidence tests passed.
This does not resolve the broader local-code/no-deployment policy gap exposed
by the demo heartbeat task, or prove a live Queen review round trip.

Task reads now include the current unanswered review message ID/text only for
Review work bound to its current request worker. Answering clears the projection;
supersession replaces it; reassignment removes it. Queues shows short requests
directly and retains complete longer requests under disclosure, without per-task
network reads. Edge fixture DOM inspection confirmed the specific Queen request
under the correct worker-owned task. No provider delivery is inferred.

All 41 review-related persistence tests passed in the newly built Linux harness
`swarm_persistence-dd4a06d4f2c77429`. The Windows broad filter had three migration
fixtures fail while constructing home-based workspaces; these passed on Linux.
The six focused review lifecycle tests passed on Windows too. Strict persistence
lint, TypeScript checking and 12 queue/fixture tests passed. Live task/Queen round
trip and the rest of the queue ownership model remain acceptance work.

### Needs You presentation check

Dev health subsequently confirmed `c728a823c1d7` with no degraded subsystems and
unchanged worker-engine identity. The queue fixture had stale missing owner fields
and mismatched correlation IDs; these now use consistent fictional identities and
explicit lifecycle evidence. A dedicated fixture invariant test plus nine queue
tests passed, as did the TypeScript check. Edge DOM/screenshot inspection then
showed Queen, worker and blocked groups and correctly grouped held briefings.
Blocked copy no longer claims nobody can move that work. This makes presentation
inspection representative; it does not supply missing production dependency or
next-action records or prove that live queues reconcile correctly.

The existing network-isolated harness served the actual DecisionInbox component
at `http://127.0.0.1:5209/harness.html?surface=needs-you-demo` in a dedicated Edge
tab. Desktop before/after screenshots and DOM inspection confirmed optional notes
no longer reserve a textarea on every card; the requester recommendation, risk,
and choices remain visible. Opening the note and entering fixture text worked,
and the summary changed to Edit your note. Interviews use the same optional-note
disclosure. No live Hive data or API was used by this fixture.

All 61 decision tests and the TypeScript project check passed, then 36 focused
inbox/interview tests passed with the new collapsed-note regression. This is
targeted presentation work, not approval of a broader mockup or acceptance of
mobile layout, live decision reconciliation, or the whole Needs You surface.

### Database recovery audit

The CLI verifier used normal database opening before checking integrity, which
can initialize a missing/empty candidate. A read-only existing-Hive preflight now
rejects missing, empty, unrelated and corrupt inputs before initialization or
migration. Two real-SQLite persistence tests pass, including a truncated valid
backup, byte-preservation checks, and reopening a valid snapshot with its task.
The version-specific backup verifier also no longer creates missing files.
Strict CLI lint passed against the Linux target. The Windows CLI check correctly
cannot compile its Unix-only host client; no compatibility shim was added.
CLI runtime integration and an actual isolated package restore remain unproven.

Follow-up installed-binary evidence supersedes the CLI-runtime gap above:
`scripts/dogfood/verify-backup-candidate.sh` passed against live dev build
`56fd31a2ad24` using a 34,885,632-byte online export. Missing/empty inputs were
rejected, the real export passed CLI and SQLite integrity verification, and an
8,192-byte truncated copy was rejected with a normal error (not timeout) without
changing its hash. Private temporary export/auth files were removed on exit.
Health was ok with no degraded subsystems and unchanged engine build identity.
No restore or worker/service operation occurred during the drill. Isolated
package restore and offline corruption recovery remain open.

Full REC-02 remains open for corruption consequence/containment acceptance.
The explicit offline/quarantine implementation above addresses the former
online-export prerequisite without weakening the existing online restore.
Never test corruption by damaging the live Hive or silently discard a failed
pre-restore export.

### Follow-up reconciliation trace and queue presentation

Strict application/persistence lint and 43 decision-related tests passed for
the scoped-read change. Notification lifecycle inspection also found obsolete
enable/start operations could proceed after logout or disable. Generation checks
now prevent later registration/save/state publication; retry callbacks retain
their original credential rather than rereading a replacement session's token.
Eleven notification tests passed, including late permission answers after stop
and disable. Browser-owned operations already submitted cannot be retroactively
canceled; this is not real-device push acceptance.

Worker decision discovery now filters requester/current assignment before the
read cap. The application regression fills the global pending inbox and confirms
the worker can still discover its own resolved ruling without seeing unrelated
requests. Both decision application tests passed. Direct-input capture is still
separate; this fixes discoverability of already-recorded decisions only.

The full frontend suite passed 1,085 tests across 127 files before the queue
progression change; its nine focused tests and project TypeScript check passed
afterward. Queue commit `b0a3758` deployed through the normal development reload:
health reported that revision with no degraded subsystems and the same engine
build identity. This is server deployment evidence, not signed-in visual acceptance.

The inbox read cap was smaller than admission (200 versus 256), hiding pending
requests at capacity. Reads now share the 256 pending bound and keep pending-first
ordering. All 28 decision persistence tests passed, including a full-capacity
fixture with resolved history and one subsequent resolution. This closes that
specific omission, not direct-worker answer reconciliation or full attention UX.

Direct-terminal answer reconciliation remains unwired: the API has verification
reads, but no production caller of `resolve_operator_statement_interview`.
ADR 0065's authenticated provider-consumption and exact-question correlation
boundary is still required; the receipt-store tests alone cannot close ATT-01.
Do not infer a resolved decision from arbitrary terminal input or worker claims.

Queue task rows now expose the existing dispatch/outcome-delivery state instead
of presenting every worker-owned item identically. Queued, delivered-but-not-active,
and uncertain briefings are distinct; pending review transport is visible.
Task update age is explicitly labeled as update age, not time waiting on an owner.
Nine focused queue tests and the project TypeScript build check passed. This is
not full owner/blocker/recovery integration or live visual acceptance.
The dedicated Edge test tab reached the deployed runtime unlock screen; signed-in
visual checks await operator unlock. No authentication workaround was attempted.

The isolated workflow-fixture worker produced documentation commit `11ad126` and
code/test commit `b7c8e07`; all six Node tests passed. Documentation auto-settled;
the code task remained in Review behind Queen's unsent prompt. Ordinary sleep/wake
restored its transcript. The demo worker was put back to sleep.

Ten minutes with the held review produced 40 task and 40 worker events. One bounded
API sample reached one core at 100% during a six-second burst overlapping Jira
reconciliation; overlap is not attribution of every CPU sample. Browser timing
showed multi-second interactions. These establish unresolved performance work,
not a completed optimization or a universal latency baseline.
