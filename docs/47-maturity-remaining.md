# Daily-driver maturity: remaining delivery and acceptance

This is the current completion checklist for the approved scope in
`45-daily-driver-maturity-plan.md`. Implementation history is in
`46-maturity-execution.md`. A passing component test does not close a live journey.
The operator authorizes commits and direct development-Hive deployments; releases
remain operator-controlled. The overall goal was restored on 2026-09-05.

## Immediate live defects

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
- [ ] REC-02: isolated corruption/restore drill and graceful failure behavior.
- [ ] OPS-01: rolling update convergence including Queen, tool freshness and
  automatic work admission after machine pressure eases.
- [ ] PROV-01: capability checklist, experimental opt-in and unattended gates;
  retain builder-only promotion and explicit provider selection.
- [ ] UX-01/P6: review the pending composition, complete coherent desktop/mobile
  surfaces, accessibility, shortcut behavior and optional return briefing.
- [ ] P7: representative workday and overnight/mobile soak with recorded limits.

## Evidence from the September 4–5 demo run

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

Full REC-02 remains open: package restore requires a successful online export
of current state before replacement. If corruption prevents that export, the
current restore path cannot recover. A separately explicit offline/quarantine
path must preserve the damaged database and sidecars with the API confirmed
stopped, then verify a restored isolated instance. Never test corruption by
damaging the live Hive or silently discard a failed pre-restore export.

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
