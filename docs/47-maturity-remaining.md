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
- [ ] Verified code tasks that intentionally require no deployment have a truthful
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

### Current review obligation in Queues

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
