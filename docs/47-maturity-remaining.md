# Daily-driver maturity: remaining delivery and acceptance

This is the current completion checklist for the approved scope in
`45-daily-driver-maturity-plan.md`. Implementation history is in
`46-maturity-execution.md`. A passing component test does not close a live journey.
The operator authorizes commits and direct development-Hive deployments; releases
remain operator-controlled. The overall goal was restored on 2026-09-05.

## Immediate live defects

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
- [ ] Unchanged Queen/outcome deferrals stop producing broad control-room refresh
  events. Preserve durable delivery ownership and visible failure/recovery changes.
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
