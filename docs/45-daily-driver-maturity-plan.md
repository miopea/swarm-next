# Swarm daily-driver maturity: scope and delivery plan

## Authoritative next gates — September 11, overnight

02:03 CURRENT PRIORITY: fix the now-reproduced shared-task lifecycle persistence
bug, not another speculative UI polish. f66e9e68 is live on production/WSL at
1.7.1-dev-f66e9e684e2b-20260911055744-1127069. Both engines preserved. WSL's
real Set up a worker link reaches the roster and add-worker form. Integrated
1649 tests/170 files pass. Fictional task01a08f02-fc6b-7953-bf57-4c73e79cf3d8
still Ready because supported POST transition to abandoned returns503; outbox
remains empty and revision1 unchanged. WSL log proves CHECK constraint failure:
target_state IN ('draft','ready','active','blocked','review','completed').
local_apiary_task_commands schema in federation_tasks.rs:1613 omitted later
abandoned/awaiting_release states. Existing domain permits retirement and
the canonical task table already gained abandoned. Inspect all involved schema
constraints and current migration version; add forward migration preserving
queued commands/receipts, regression and failure/recovery tests. No raw DB edits
or fake Active/Review transitions to bypass this. After verified deployment,
retire this exact fictional task normally and prove Keeper/member convergence.

02:00 checkpoint: real shared-task acceptance is now the active journey. Fictional
task01a08f02-fc6b-7953-bf57-4c73e79cf3d8 reached existing WSL Hive and survived
an API restart with engine6458 preserved. It remains Ready, unexecuted, for the
next browser check. f66e9e68 adds worker-setup guidance to the empty chooser,
retains ownership, and suppresses impossible non-Ready/stale-worker dispatch.
70 task-board tests and production build pass. Supported production deployment
is started, not yet verified. Next: verify deployment, update WSL with the exact
bundle, browser-check Set up a worker navigation, then retire ONLY this fictional
task through supported lifecycle commands and verify Keeper/member convergence.
Do not run a provider merely to clean up this test. Earlier2e4e0dfb CI is green.

Latest: feedback contact identity reuse is committed and live on production/WSL
at2e4e0dfb;61 affected tests, TypeScript/build and390px fictional account-choice
acceptance pass. Both engines preserved. CI pending; see92. This completes the
contact-entry gap, not central feedback delivery. Keep native answer reconciliation
and real-device gates open. The pending Daisy membership approval must not be
retried without the requested approval; continue unrelated work. Usage69% consumed
at the last check; prioritize coherent closures, not repeated passed UI checks.

01:25 checkpoint:78ec2bc1 is healthy on production/WSL with both engines retained.
Integrated UI1641/170 passes. Optional Jira setup no longer claims failed membership
or hides real catalog/policy holds; no-Jira, mixed-failure, recovery and unknown
states have390px fixture acceptance. Live WSL retains1 usable Jira project despite
an additional catalog project's setup requirement. See92 for exact boundaries.

01:15 checkpoint:94b210fb is live on production and WSL. Existing member rename
and restoration both reached Keeper without rejoining. Direct Invite a Hive
navigation passes desktop1465px/phone390px focus, visibility and overflow checks;
invitations precede advanced delegation settings. Optional-Jira labels are live
and prior78a212de CI is green. Preserve the pending Daisy approval gate below.
Do not repeat workspace, creation-profile, rename or invitation-entry acceptance.
Next: remaining user-facing proof gaps in93/96, with native/device boundaries
explicit. No BFG Admin communication or release merely to create activity.

Latest checkpoint: member claims fix8f320a9b is live on production and WSL.
All overview reads now200; warning absent; three-Hive roster retained; both
worker-engine PIDs unchanged. CI34561952448 passed all four jobs. Workspace
desktop/mobile creation, search, tilde override and invalid-path recovery passed;
both never-started demo workers removed, RCG search root retained.

Fresh joining reached the verified Daisy Member request without Jira. Auto-review
blocked the production Approve Hive action; explicit operator approval requested.
Do not retry that grant or bypass it. Continue unaffected work. Final browser-closed
membership completion remains open. Disposable test services stop after two hours.

The creation-profile correction is now live at b4322b193f6a on both instances.
Isolated browser creation retains Bea Keeper, bea@example.test and Bea's Hive.
47 focused UI tests, TypeScript and the full1630-test/169-file UI suite pass.
Next: finish CI receipt, then unaffected UI-1 through UI-4 gates from document93.
Keep the pending fresh-join approval separate; no unrelated BFG Admin messaging.

Historical deployment checkpoint: production and WSL served development
revision 4e46e680; both engine processes were preserved. Enrollment timing and
workspace recovery are deployed. Desktop and actual 390px Manage Apiary scrolling
passed. Existing WSL membership and the three-Hive directory survived update.

Resolved blocker (8f320a9b): member shared-work returned 403 because it
used a Keeper-only local read. Scoped outbound member claims, isolation and HTTP
recovery tests now pass; WSL warning cleared after deployment. Workspace acceptance
also passed. These completed checks must not be repeated as unfinished work.

The operator has authorized overnight commits, deployment, WSL modifications
and a release if needed for testing. Do not release merely to bypass validation.
Other worker changes are finished; recheck the shared checkout before updates.
No BFG Admin worker communication without explicit operator approval.

## Historical next gates — September 10, 23:18 Eastern

The latest operator priority is guided Apiary onboarding and workspace setup.
The dated sequencing paragraphs below are historical where they conflict with
this checkpoint. Keep the full maturity objective; do not claim completion from
individual UI patches or deployment health.

1. Finish current-main CI and deployment of the enrollment timestamp correction.
   The operator approved isolated transfer. All four enrollment tests and ten
   repetitions of the previously intermittent restart test now pass, as does
   pinned-toolchain formatting. The isolated checkout is older than current main;
   this targeted result does not substitute for current-main CI or live acceptance.
   Validation now uses receipt time for invitations issued during the request,
   without relaxing consent checks. The correction is not yet deployed.
2. Restore the requested Edge extension connection, then verify the fresh WSL
   submit-once/Keeper-approval flow and existing-member directory/rename behavior.
   No remove/rejoin workaround for existing members. Component tests are not
   browser acceptance; do not substitute another browser surface without approval.
3. Activate verified workspace setup recovery from main 8a96fe34 only when the
   shared Linux checkout is clean and the supported updater is inactive. It now
   has another worker's uncommitted API changes; do not overwrite or deploy them.
4. Complete desktop/mobile workspace selection, path override and inline-error
   acceptance, then return to document93's remaining UI-1 through UI-4 gates.

Live app 1be4f138 is healthy; its update preserved all 10 worker sessions and the
existing engine process. No release is authorized. Documents92/93 distinguish
current evidence from the historical checkpoints below. Do not contact the BFG
Admin worker without operator approval, and do not replace major acceptance
work with unrelated polishing while a gate is blocked.

## Current sequencing — 1.7.1 deployed, September 10

The operator confirms deployment and instructs continued work. The release hold
below is historical, not a reason to stop or repeat candidate CI. Document93's
current execution checkpoint governs: resume exact native-answer/Needs You
reconciliation, then remaining ordinary-user UX acceptance. Preserve the open
live Apiary browser-evidence gap separately; do not expand the released scope.
No new release is authorized by this continuation.

## Immediate priority — September 10 Apiary onboarding

Release-candidate scope is frozen following the operator's concern about a moving
finish line. Finish the in-flight explicit synchronization retry, CI, and the
existing Keeper/WSL in-place acceptance checks. Do not start cosmetic additions
or unrelated maturity work before reporting this gate. New findings are deferred
unless they prevent this agreed onboarding/recovery outcome or threaten data.
The broader maturity goal remains open; this freezes sequencing, not its scope.

The operator wants this work completed before inviting another developer. Do not
cut a release yet. Finish the joined experience, not just the underlying protocol:
shared profile preview/editing, default-name replacement without changing custom
names, a complete Keeper/two-member roster, rename propagation, and honest
offline/mixed-version recovery. Keep private Hive work and authority isolated.

Current local checkpoint: signed profiles and full directories are committed
through `254e7ecf`. Normal HTTP sync and public roster projection now pass the
real HTTP join/rename test; five domain directory tests, 27 Keeper/member/settings
UI tests, TypeScript checking and strict API Clippy pass. These changes are not
yet deployed or browser-accepted. The HTTP check uses one member; independent
three-Hive persistence evidence does not substitute for the final browser journey.

Profile preview/default naming is now implemented locally: the existing connect
or join action saves the reviewed profile before sending membership work; there
is no extra Save click. Five profile tests (including integrated join/save failure)
and seven policy/join recovery tests pass. The authenticated default-name API test
passes and verifies stable IDs and no implicit membership. Visual acceptance is
still missing: Edge tooling failed before connecting with a kernel-assets path
error twice. Do not claim browser acceptance from component tests.

Existing Hives must recover **in place**. No fix may require removing/rejoining
members, regenerating invitations, or replacing stable identities/credentials.
Validate upgrades of existing memberships, not only fresh joins. Missing public
names/emails are now editable through “Edit shared profile” for joined Hives.
Integrated tests verify saving without join calls and preserved success after
refresh failure (27 focused profile/settings tests pass). Real-browser validation
and cross-Hive delivery remain open; local save is not a synchronization claim.

Directory freshness UI is now implemented in Member and Settings: incomplete or
unreadable status is explicit, saved snapshots are dated, expired snapshots warn,
and refresh failure preserves the prior snapshot. Thirty focused roster/settings
tests pass. Visual/live synchronization checks are still required.

Next: verify profile preview and directory status visually, then
verify Keeper and two members end to end. Announce release readiness only after
those gates close. Preserve the unrelated untracked native-signal probe. Source
transfer to the isolated server test checkout is approved. The operator has now
explicitly approved pushing verified changes for another worker to validate.
Do not describe that handoff as release-ready or cut a release.

## Active execution contract — September 9, 2026

Read this section and [the UX-first remaining plan](93-ux-first-remaining-plan.md)
first after any compaction or restart. They supersede older sequencing and
coordination instructions below, not the approved requirements.

- **Latest operator direction: ordinary-user UX first; incremental delivery is
  welcome.** Focus on one-to-five-worker navigation, Settings, mobile controls,
  attachments, understandable messages and consistent layouts. Defer reload
  profiling and deep orchestration; neither should dominate this pass. Finish
  verifying the in-flight search-layout deployment, then resume remaining UI-3
  Settings and UI-4 integrated visual finish. Do not restart reload profiling
  after compaction. Reload and direct-worker reconciliation remain open scope,
  not acceptance claims or reasons to postpone everyday usability improvements.
- Earlier UI-2 checkpoint:
  UI-1 presentation (`eb931114`) and UI-2 controls (`221e66cd`) are deployed with
  passing CI. UI-3 runtime safeguard placement is `f482b460`; verify its activation
  and current CI in [93](93-ux-first-remaining-plan.md) before claiming it live.
  Do not redo those implemented batches. Then finish remaining UI-3 Settings/
  runtime/recovery and UI-4 integrated visual finish. UI-1 clarification and
  direct-worker decision reconciliation remain open, not solved by presentation.
- Inspect the rendered experience in a separate Edge tab at
  `https://swarm.bfgsolutions.net`. Preserve the cute bee identity and ordinary
  one-to-five-worker experience. Use fictional fixtures for intrusive tests.
- Each package ends with focused checks, browser evidence, a coherent commit,
  and safe development-Hive deployment. No release is authorized.
- Defer deep engine/performance work to the documented backend handoff unless
  it directly blocks this package or threatens data, input or session safety.
  Do not repeat passing tests or start unrelated investigations.
- **Do not communicate with the BFG Admin worker without fresh operator
  approval.** A documented handoff is not authorization to send or delegate it.
- Check account usage at package boundaries. Around 10% remaining, start no new
  package; around 5%, stop implementation and consolidate unfinished work,
  revisions, evidence, risks and next steps. Do not consume resets or buy credits.
- Keep [the acceptance ledger](92-maturity-acceptance-ledger.md) current.
  Implemented, verified, deployed and accepted are different states. The overall
  goal stays open until its actual acceptance criteria are met.

Status: scope, delivery sequence, implementation, and local phase commits approved.
Execution: active goal; local commits, mainline push, and direct development-Hive
deployment are authorized. The operator chooses releases; no release is authorized.
Date: 2026-09-03. Source: the operator's UX, stability, efficiency, and orchestration interview in this Codex task.
Code baseline: clean `main` fast-forwarded from `a10bf2c` to `36420b3` during planning.

## 1. Overall objective and authority

### Added scope: BFG Admin feedback and development integration (2026-09-06)

Integrate with BFG Admin's shared Messages, Requests and diagnostic workflows,
using contracts owned in `C:\projects\bfg-operations`; coordinate contract changes
with the BFG Admin worker. Keep two directions distinct:

- Admin's reviewed development-task submissions enter Swarm as linked work.
- Swarm's own customer feedback, bugs, feature requests and support emails enter
  Admin's shared support workspace, not a second independent support system.

Preserve originating app, full sender identity, conversation history, supported
attachments and source links. Development tasks retain their support conversation
association. Operator-approved replies return through the original channel and
subsequent replies remain in the same conversation. AI may classify, suggest
duplicates and draft; only the operator approves customer-facing sends.

Contract inspection must preserve existing authorization and frozen/idempotent
submission semantics, not grant workers access to operator-only triage routes.
Prevent duplicate ingestion and reply loops, and make failures and retries
visible. Verify with fictional messages, including lost responses, stale approvals,
replayed provider events and subsequent replies in the original conversation.
Admin currently pulls privileged app-owned conversation resources; there is no
generic Admin feedback-ingestion endpoint. Its current reply contract is
email-specific, and attachments/source links require coordinated typed additions.
Do not silently drop those fields or substitute email for an in-app channel.

Operator hosting decision: every Hive submits to one shared central support
source. Email is the only required contact identity; no BFG account and no inbound
access to the customer's Hive/workers/repositories. Preserve additional identity
when supplied. Admin provides conversation handling, triage and approved replies.
The central source must accept explicit submissions safely without making private
conversations/diagnostics publicly readable. Support IDs remain distinct from
development tasks and diagnostic IDs.

When Swarm closes linked implementation work, completion summary and evidence
return to Admin's originating request/conversation. Admin prepares the customer
response for operator approval and delivers through that conversation's original
channel. Closure is neither send authorization nor evidence that code is deployed;
customer-facing wording must distinguish implementation from verified deployment.

Make Swarm responsive, stable, self-managing, observable, and delightful through
long daily use, including desktop/mobile handoff and unattended work, while
preserving its cute, feminine-leaning bee/Hive identity and independent worker
engine. Performance and reliability are at least as important as visual polish.

The operator subsequently approved continuous execution, commits between phases,
mainline push, and direct deployment/testing on the production development Hive
without routine approval pauses. Historical proposed/authorization wording below
records planning context; this approval governs execution. No release is
authorized. Track actual evidence in `46-maturity-execution.md`.

This is one overall maturity program, delivered in multiple independently
verified phases with commit checkpoints. The operator wants the complete scope
before choosing delivery order. This document does not authorize implementation,
deployment, commits, provider restarts, or changes to live worker tasks.

The interview is finished. Do not repeat questions about existing scroll behavior,
roster order, task creation, mobile composition, or other settled requirements.
Ask only when evidence reveals a genuine unresolved product conflict.

Labels used below:

- **A — Approved outcome:** implement or improve within existing architecture.
- **V — Preserve/verify:** capability exists or is reported to exist; prove wiring and recovery before rebuilding.
- **E — Experiment:** approved to evaluate, not to ship regardless of results.
- **M — Mockup gate:** visual proposal requires review before implementation.
- **Q — Conflict:** explicit product reconciliation required; see section 10.

Accepted ADRs remain implementation authority. Amend them explicitly where this
interview changes behavior. Update the capability inventory when capability status
actually changes, not because a plan has been written.

Final operator clarification: this interview defines the target system. Recent
commits are patches to the existing system, not competing product requirements
or proof that a reported pain point is fixed. Reconcile ADRs to these decisions
before affected runtime changes; do not reopen settled outcomes merely because
older code or documents implement something different.

## 2. Evidence and recent-change reconciliation

### Operator-observed failures, not inferred causes

- Remote Linux host: 8 cores, 32 GiB RAM. Edge Task Manager reported roughly
  6–38% CPU with about ten open, mostly idle workers; one screenshot showed 38.9%.
- Whole application feels sluggish: typing, switching, and terminal redraws;
  redraw/reconnect can take 4–8 seconds. Reload appears to reset degradation.
  Usage varies, so a leak or specific cause is not yet proven.
- Desktop/mobile terminal geometry can fight. Mobile was sometimes backgrounded,
  so simultaneous intentional input does not explain all instances.
- Android installed PWA reconnects after a very brief trip to another application.
- Claude AskUser renders question one correctly on mobile; subsequent questions
  overwrite prior text. Selection mapping works, desktop is correct, and hiding
  the keyboard plus redraw does not repair it. No other TUI was reported affected.
- Camera and gallery attachment selection can produce absolutely no visible result.
- Needs You has contained stale/resolved questions, Queen's own backlog, and
  misleading terminal-wait warnings while Queen was working.
- Queen sometimes distrusts genuine operator instructions relayed by workers,
  over-reviews, or redirects work at an inappropriate pause.
- Conversation resumption has chosen stale or incorrect threads.

### What recent code changes mean for this plan

| Evidence | Current finding | Scope treatment |
| --- | --- | --- |
| `205cebe`, `909d343` | Replaced looping roster box-shadow animation with transform/opacity and added a guard. Commit explicitly says CPU was not re-measured. | V: profile current build; do not claim sluggishness fixed or redo the same CSS change. Composited animation is not free. |
| `15cac17` | Moved held briefings/unjudged Queen work to Queues; reconciled Needs You counts. Retains blocked-over-12-hour escalation and conversation drift. | V/A: verify current data, reconcile resolved items, make remaining escalation genuinely actionable. |
| `a8278dc` | Added five-minute protection after operator terminal input. | V/A: verify protection, and distinguish prompt-idle from task-safe. |
| `9690c33`, `6b8c119`, `f66045c`, `3d1d460` | Delivery session identity, submission confirmation, and bounded broadcast follow/expiry work landed. | V: extend actual delivery lifecycle; never rebuild a parallel message bus from assumptions. |
| `c9b331f`, `b952d88` | Reduced fetching/rendering of collapsed completed work. | V: preserve; inspect remaining hidden page work. |
| `a10bf2c`, `be11076` | Non-image reference injection fixed. | V: this is not proof camera/gallery image selection works. |
| `MobileTerminalComposer.tsx` | Existing composer, keys, Refresh, upload status, disconnected-picker handling, and picker-return notice. | V/A: mature this implementation, reproduce silent path. In-memory picker flag cannot survive page destruction; delayed callback cleanup also needs scrutiny. |
| `DiagnosticsWorkspace.tsx` | Machine memory, load/CPU pressure, swap, worker process-tree memory, view-switching evidence, sanitized reports already exist. | V/A: earlier interview description of RSS-only diagnostics was incomplete. Validate existing samples and fill browser/per-process gaps. |
| `TerminalController.ts` | Registry retains controllers until explicit session/all closure, with no count eviction inside the registry. Detached/hidden rendering is already paused. | E: inspect caller lifecycle and measure retention; this alone does not establish a leak. |
| `TerminalConnection.ts` | Already has bounded pending render bytes (3 MiB), resume support, and render suspension. | V/A: preserve bounded behavior; improve pacing/recovery from measured traces. |
| `6ff680a` / 1.4.0 | Codex now receives Swarm MCP configuration. | V: doc 43's claim Codex cannot reply is stale for new sessions; old sessions still need reconnect evidence. |
| `82d3285` / 1.4.0 | No-deployment claims and Queen approvals can be withdrawn. | V: doc 44's missing-withdrawal scope is partly superseded; verify callers and re-evaluation. |
| `5330f9f` / 1.4.1 | Terminal host asks Claude whether a pinned conversation exists instead of API-side filesystem guessing. Explicit missing result starts fresh under the id. | V/Q: preserve provider-authoritative lookup; reconcile fresh-context fallback with interview. |
| docs 43/44, `NextMoveOwner` | Owner-of-next-move design exists; generalized durable addressee/reason and operator ownership remain relevant. | A/V: finish coherent domain model rather than add UI-only grouping. |

### Live review limits

A separate Edge tab at `https://swarm.bfgsolutions.net` displayed the unlock page
and runtime `1.4.1-dev-cf245ff3ad13-20260903163720-606699`. This differs from the
local baseline, so local source is not asserted to equal the deployment.
Authenticated UI inspection awaits the operator unlocking that separate tab.
No live CPU profile, SSH sampling, mobile reproduction, benchmark, or soak was
performed as part of this plan. Commit test claims are historical evidence,
not tests rerun by this planning task.

## 3. Performance, diagnostics, and Developer Dogfood

### PERF-01 — Attribute the cost before optimizing (A/V)

Correlate browser, connection, App/API, terminal host/worker engine, provider
processes, database, and queues. Distinguish CPU utilization, runnable load,
kernel pressure, I/O stalls, memory use, and unavailable/stale readings.
Use Linux process-tree/cgroup-aware checks where applicable; avoid double counting
shared memory and confusing host capacity with process limits. Read-only SSH
sampling may be used on the operator's remote host, without stopping services.

Browser evidence: interaction/paint latency, long tasks, terminal parse/render
backlog, route transitions, active/warm controller counts, sockets/traffic,
Swarm-owned timers/listeners/observers, DOM growth, renderer/context loss, and
heap trends where supported. A normal web page cannot promise Edge Task Manager's
per-tab CPU value; cross-check external measurements during profiling.

Build comparable scenarios: fresh load, one worker repeatedly visited, normal
10–15-worker switching, output bursts, mostly idle operation, full-day use,
PWA return, and reload after degradation. Record hardware, build, browser,
worker count, output volume, elapsed session time, and instrumentation overhead.

### PERF-02 — Bounded browser work (A/E)

- Isolate terminal output from roster and whole-page rendering. Apply urgent
  prompts/failures immediately; coalesce ordinary status updates around 250 ms.
- Unmount inactive major pages and stop their owned subscriptions/observers.
  Keep lightweight filters/selection/position and bounded cached data, not hidden
  DOM machinery. Audit what already unmounts before changing it.
- **Experiment:** active terminal plus four recent warm renderers, with a bounded
  snapshot for colder sessions. Never stop the worker/provider to evict a view.
  Restore newest output on returning to a worker, per the operator's final choice.
- Cold restore target: interactive within 500 ms at p95 under the representative
  test load, with no geometry jump or missing output. This is a percentile target,
  not a promise every sample is below 500 ms. Keep/reinstate more warm terminals
  within an explicit resource cap or reject eviction if it hurts UX.
- Pace terminal paint work by animation frame without dropping/reordering ANSI
  input. Server history remains bounded under its retention policy, not infinite.
  A renderer falling behind gets a truthful catch-up state and sequence recovery.
- Preserve the control/terminal traffic separation already required by architecture.
  Evaluate multiplexed terminal subscriptions separately from the low-volume
  control channel. Exact socket count/protocol is an ADR decision and measured
  optimization, not a reason to rewrite working transport prematurely.

### DIAG-01 — Operator diagnostics (A/V)

One Diagnostics unit with Browser, Server, and correlated incident views. Lead
with a plain-language finding, evidence freshness, and confidence. Do not call an
unmeasured subsystem healthy or infer database integrity from one successful read.

Browser capture is local and immediate, not a second long-term telemetry system:
30–60 minute bounded ring, a small before-reload aggregate, latest few incidents,
hard 24-hour expiry. No terminal content, raw keystrokes, image contents, prompts,
credentials, or private paths in automatic traces.

Automatic content-free incident window: approximately two minutes before and one
minute after detection. Initial capture thresholds: input/redraw >1 second,
critical classification >3 seconds, reconnect >2 seconds. Repeated long tasks and
resource growth can also trigger bounded capture. Thresholds are starting points
to tune from evidence, through owned authenticated configuration.

These are capture thresholds, NOT automatic Needs You thresholds. The later,
stronger operator decision governs: recovered problems need no operator alert.
Only unresolved actionable consequences escalate. Distinguish unavailable metrics
from normal metrics, and distinguish suspected cause from established cause.

Run diagnostics starts passive and non-disruptive. Run deeper test is explicit,
bounded, reports its own overhead, and uses disposable render/transport fixtures;
it never injects test commands into an active worker or writes test data to the
production database merely to obtain a measurement.

### DOG-01 — Developer Dogfood (A)

Own Settings unit, automatically selected by existing development-mode detection
(`DevelopmentRuntime.enabled` path), not a second authority toggle. Show revision,
instrumentation profile/overhead, recent captures, soak evidence, regressions,
retention, and release-readiness evidence. Deeper collection remains bounded.

Ordinary server aggregates start with 30-day retention; Developer Dogfood keeps
revision-linked comparisons long enough for release analysis under explicit byte
and age budgets. Exact extended retention is an engineering proposal to publish,
not an unbounded promise. Local browser traces are not silently exported to a
third-party telemetry service. Developer-Hive aggregates stay in its owned store.

Measure Queen review yield/returns, time-to-delivery, queue age by owner, duplicate
operator questions, recovery outcomes, false alerts, connection phases, and update
convergence. Subscription use is the current model: monetary cost estimation is
deferred; preserve extensibility for direct API billing without a speculative UI.

## 4. Terminal, mobile, and device continuity

### TERM-01 — One explicit interactive owner (A/V)

Existing server-side PTYs outlive browser/App/API. Preserve that boundary.
One owner per terminal; passive viewers receive output without input/resize rights.
If no active owner exists, resume automatically. Otherwise Resume Here atomically
transfers input and canonical geometry. Backgrounded clients cease resize claims;
brief disconnects must not cause lease flapping. Reject stale-generation input
and resize events after takeover. Amend ADRs 0012/0045 explicitly.

PWA resume: show cached shell and terminal with age/reconnecting state; permit
navigation/reading, but disable input until connection and ownership are confirmed.
Restore control state, selected terminal, then warm views. Do not try to guarantee
an always-running background mobile browser. Distinguish suspension, eviction,
network failure, expired authentication, and API restart.

Unsent composer text is recoverable. Uncertain terminal delivery is never blindly
replayed, especially Enter. Do not reconstruct raw keystrokes into a claimed draft.
Keep draft/submission bound to immutable worker/session identity across reconnect.

### TERM-02 — Render fidelity (A/V)

Reproduce Claude AskUser question two and later on mobile independently of desktop
resizing. Capture a safe representative sequence/fixture with consent where needed;
compare renderer modes, wrapping, cursor movement, clears, alternate screen,
keyboard viewport changes, and snapshot replay. Fix the renderer; no separate
Swarm question interface in this scope. Correct selection alone is not a pass.

Existing scroll behavior is preserve/verify: follow at bottom, pause when reading
older output, Jump to latest, and newest output after leaving/returning. No rebuild.

### MOB-01 — Mature the existing composer (A/V)

Use native autocomplete/dictation and multiline text. One Send submits text and
Enter; no Insert-only action and no per-worker draft store. Keep one draft bound
to its original worker; warn before an explicit action would discard it.
Preserve text on disconnection or attachment failure. Audit existing delayed Enter
for unmount/reconnect races; arbitrary timing cannot establish delivery truth.

Compact controls: arrows, Enter, Escape, Attach, Redraw; less-used Tab/Ctrl controls
under More as layout permits. Preserve current useful commands and slash workflow.
Use touch targets, keyboard-safe positioning, and visible connection/input state.

### MOB-02 — Reliable attachments (A/V)

Test camera and gallery in installed Android PWA, plus iOS equivalents. Immediately
acknowledge a selection with thumbnail/placeholder, then uploading, ready, and
delivery status. Upload success is not proof the worker received the reference.
Keep retry/remove controls and draft text; hold Send until attachments are ready.
Distinguish deliberate picker cancellation from an actual failure.

Trace picker launch/return, file availability, page suspension/eviction, preview,
HTTP upload, shared-file availability, reference insertion, and submission. Retry
without reselecting while the File is still available; if the OS destroyed it,
explain that re-selection is necessary rather than promise impossible recovery.
Make retries idempotent and bound temporary files, preview URLs, upload size,
concurrency, and abandoned-upload cleanup. Preserve existing non-image attachments.

Uploaded artifacts remain in the shared filesystem accessible to Queen and workers;
no new worker-to-Queen privacy partition. That does not authorize external upload
of artifacts or diagnostic contents to GitHub or another service.

## 5. Queen orchestration and trustworthy work state

### QUEEN-01 — Orchestrate, do not become the bottleneck (A/V)

Machine-settle facts derivable from trustworthy evidence. Queen handles judgment,
asks the assigned worker (usually the best context holder), and gives a concise
recommendation that remains visible when escalation reaches the operator.
Reliable machine-checkable completion evidence settles routine work without a
mandatory Queen approval. Queen handles exceptions, conflicting evidence, and
genuine judgment. A worker's unsupported self-declaration is not verification.
Measure review yield and waiting time to improve this policy, not to defer the
already approved removal of mandatory Queen review for machine-verifiable work.

Queen may use idle Scout for a second opinion, not arbitrary peer workers.
Queen creates/assigns cross-worker or cross-repository dependent tasks so workers
stay in their lanes. Independent tasks may run on multiple idle workers, subject
to deterministic resource admission and existing single-active-task rules.

### QUEEN-02 — Respect active work and human engagement (A/V)

Protect operator typing and active engagement with the selected terminal.
Queen may kick a genuinely stalled idle prompt to continue the same task/context,
but prompt-idle does not mean a multi-turn task is finished. Preserve polite
delivery and extend it with owned task/engagement evidence, not an arbitrary timer.
Apply the same protection to Queen. New work queues behind current work unless an
explicit scoped intervention changes priority for a recorded reason.

Bounded recovery: observe/reconcile, attempt a safe task-scoped correction, assess
the result, then escalate only if it cannot move. No retry storms or repeated
generic prompts. Destructive work already authorized within a task is not
automatically forbidden; do not invent broader authority than that task supplies.
Recover in the same conversation. Sleeping/waking is not a context-clear action.

### QUEEN-03 — Proven operator instructions and deduplicated answers (A/V)

Provide verifiable first-party records for operator answers supplied in terminals
or composer, linked to worker, session, time, and relevant decision. Exact operator
statements may be preserved for credibility. Separate operator input from model
output, pasted third-party material, tool output, and a worker's assertion.
Reuse ADR 0054's verification model; a trusted source proves who said something,
not unlimited authorization for every interpretation of it.

Reconcile answers given directly to workers with Needs You and Queen so the same
question is not asked twice. Exact correlation resolves deterministically; ambiguous
semantic matches should not silently close unrelated requests. Record resolution
and delivery separately; recover uncertain delivery without duplicate action.

### QUEUE-01 — Who owes what next (A/V)

Queues is an exception-oriented view grouped by owner, not transport mechanism.
Show owner, concise reason, waiting age, dependency, last meaningful action, and
what will unblock it. Urgency may use color plus text/icon, never color alone.
Moving work stays minimized; recently resolved/history supports later inspection
and metrics. Align with docs 43/44's durable next-move ownership and reason.

Task dependencies must be real domain records, not inferred dispatch ordering.
Detect dependency cycles, missing owners, stale handoffs, and overdue promised
follow-ups. Assignment, wake, delivery, provider acceptance, work completion,
release, and operator resolution must not masquerade as each other.

Tasks remains the log/detail/reassignment surface; preserve the working /task
creation workflow and existing roster order/settings. Viewing a sleeping worker
is intentionally designed to wake it; sleeping workers are absent from Awake.

## 6. Attention, presence, and notification UX

### ATT-01 — Needs You means needs the operator (A/V)

Concise decision/action first, worker context and Queen recommendation available,
details progressively disclosed. Show only things the operator can actually act
on. Queen's backlog belongs to Queues. Resolved and recovered items disappear
from active attention; retain audit/metrics quietly. One shared source derives
cards, counts, urgency, and notification links. Blocked-task age alone never
creates an operator escalation: Queen elevates when she cannot move the work and
needs a specific operator action. Timers may inform Queues/diagnostics or prompt
bounded internal reconciliation, but cannot substitute for that escalation.

Group notifications where useful; tapping takes the operator directly to Needs
You and the relevant request, not a second intermediate click. Existing bounded
interview decisions are retained; they are not the replacement for broken
provider-native AskUser rendering.

### ATT-02 — Health indicator and runtime messages (A)

Top pulse indicator: worst active state only, healthy/degraded/action-required;
no alert count or constant decorative animation. Runtime area: compact active
system messages for updates, pressure, reconnects, diagnostics, and recovery.
Both open the same incident details, not duplicate notification systems.

Unresolved warnings persist; critical actionable incidents pin. Recovery requires
no operator acknowledgment or push. If already visible, a short recovery state
may clear automatically (up to about 30 seconds); no new attention for resolved
problems. History is bounded and collapsed. Run diagnostics from subsystem detail.
Developer incidents can offer a Swarm repair task; other installations offer
sanitized details and editable GitHub feedback, never automatic public posting.

### PRES-01 — At Hive, Reachable, Night Watch (A/V)

Reuse existing OS-lock/idle detection and server-authoritative presence.
Reachable covers desktop locked/away but phone available. Night Watch has schedule
plus manual toggle; opening/using Swarm on desktop ends it. Define timezone/DST,
schedule re-entry, stale device reports, and unsupported lock detection in tests.
Phone use does not silently end Night Watch merely because a mobile tab is active.
Presence adjusts attention/orchestration, not authentication or permission scope.
Amend ADR 0018's indefinitely dominant explicit override behavior.

Return to the worker/workspace the operator left, showing newest terminal output.
Optional concise While you were away briefing is a mockup-gated convenience, not
a mandatory modal or substitute landing page.

## 7. Recovery, resources, updates, and providers

### REC-01 — Conversation and shutdown correctness (A/V/Q)

Verify provider-native resumption of the operator's chosen conversation, including
an explicit conversation switch becoming the new default. Prefer provider-native
continue/resume semantics; do not choose a thread merely from file timestamps.
Distinguish identity of Swarm terminal session from provider conversation.
Final operator clarification: attempt safe recovery of the chosen conversation,
then provider-native `--continue` (or its equivalent). If those attempts fail,
opening a fresh session is permitted as the final fallback; clearly report that
prior context was not restored and let the operator use the provider's resume
command. Do not label a fresh session as recovered context or blindly replay the
prior task's commands into it.

Graceful shutdown drains safely where possible, records interruption, and resumes
the prior conversation/task after restart without replaying commands blindly.
Bound drain/recovery operations; expose failed recovery as an actionable condition.

### REC-02 — Database protection (A/V)

Database is the requested backup scope, not whole-repository/provider-history
backup. Starting policy: seven daily plus three pre-upgrade snapshots, bounded
and validated. Prioritize recent corruption/upgrade recovery over elaborate
long-term retention. Restore is explicit, preserves a recoverable copy, and is
tested on isolated copies. Corruption must make the operator aware and stop unsafe
writes/dispatch; do not kill independent workers unnecessarily. Drain safely only
when the surviving state permits it; never claim database writes are safe after
integrity failure. No blind automatic restore over newer work.

### OPS-01 — Resource admission and updates (A/V/Q)

Runtime pauses new automatic starts when measured machine pressure demands it,
then resumes queued work when pressure eases. It does not leave capacity paused
indefinitely or ask Queen to arbitrate hardware policy. Sustained consequences
requiring the operator surface once, with cause and available action.

Swarm worker may perform safe App/API updates under existing authority; preserve
the independent engine and active sessions. Automatic rolling provider/session
updates are desired: drain at safe boundaries, resume the same conversation,
verify tool/schema freshness, progress through the roster, include Queen, and
provide bounded deferral/escalation so no session remains obsolete forever.
Differentiate App/API, worker-engine compatibility, provider binary, and cached
tool-schema updates. Do not interpret "rolling" as license to interrupt live
work or override current destructive-migration/engine-replacement approvals.

### PROV-01 — Earned provider maturity (A/V)

Provider acceptance checklist: install/auth, interactive rendering, input/paste,
attachments, Swarm tool access, permission/question behavior, conversation
selection/resume, failure recovery, task outcomes, updates, and unattended behavior.
Experimental providers require explicit opt-in and never run Night Watch.
Only the builder promotes them when full required capability is demonstrated.
No automatic provider switching. Preserve explicit context/task handoff when the
operator spawns a different provider from an existing worker. Durable trusted
device sessions must survive ordinary reload; do not require repeated login.

## 8. UX/UI maturity without losing Swarm

### UX-01 — Whole-product coherence (A/M/V)

Retain cute, feminine-leaning bee/Hive personality, warmth, terminology, and visual
identity. Broad layout redesign is allowed, but show mocks before committing
unapproved compositions. Personality stays visual and lightweight: no sounds,
expensive continuous effects, or decoration that hides state. Respect reduced motion.

Review desktop density, mobile hierarchy, typography, contrast, spacing, touch
targets, keyboard/focus behavior, loading/empty/error/offline states, dialogs,
roster, task details, Queues, Needs You, Settings, and runtime cards together.
Preserve established workflows rather than adding features to justify a redesign.

Worker switching follows fixed configured order. Evaluate requested Ctrl+Tab
behavior in browser/PWA without stealing terminal/provider keys; browser-reserved
shortcuts may need an available configurable fallback. Preserve existing quick
navigation until actual supported shortcut behavior is verified.

Test desktop Edge/Chrome, real Android installed PWA, and real iOS browser/installed
web-app behavior for critical journeys. Responsive emulation is useful but does
not prove OS picker, suspension, keyboard, or device handoff behavior.

## 9. Recommended delivery sequence and verification plan

**September 9 operator priority update:** [93](93-ux-first-remaining-plan.md)
supersedes the remaining execution order below with UX/UI-first delivery and an
approximately 5-percent-usage handoff. All requirements and safety/acceptance
rules in this plan remain in scope. The historical sequence below is retained
for context, not permission to defer the user-facing finish pass again.

These are work packages and dependency guidance, NOT an approved delivery order.
Every phase ends in a reviewed, coherent commit checkpoint with evidence and known
limitations. Split further if needed; never mix unrelated live changes into a
phase commit. Commit/push/deploy authority remains separate until agreed.

| Candidate | Deliverable | Dependencies | Exit evidence |
| --- | --- | --- | --- |
| P0: Reconcile and baseline | Revision inventory, duplicate-task reconciliation, relevant ADR amendments, reproducible scenarios, metrics dictionary | Scope review; authenticated dedicated tab for live work | Current/deployed/engine/provider revisions distinguished; measurements reproducible; conflicts decided before affected code |
| P1: Measurement foundation | Browser/server attribution, bounded captures, diagnostics, Dogfood profile/storage | P0 | Known injected disposable faults attributed; unsupported readings honest; measured instrumentation cost bounded; privacy/expiry tested |
| P2: Terminal and mobile reliability | Ownership/handoff, PWA resume, input safety, AskUser, camera/gallery | P0; minimal P1 timing hooks | Repeated real-device cutover and multi-question flow, no duplicate input, image visible and usable by worker, no provider restart on view recovery |
| P3: Performance experiments | Warm-pool experiment, render pacing, hidden work cleanup, event coalescing, transport optimization only if justified | P1 baseline, coordinate with P2 ownership | p95 cold restore <=500 ms; input stays responsive under output; plateauing resources in same workload; no missing sequence/history or fidelity regressions |
| P4: Queen and attention correctness | Proven operator statements, recovery ladder, durable next mover/dependencies, deduplicated decisions, Queues | P0 conflicts; P1 metrics | Realistic task/Queen/worker/answer lifecycle clears itself; no mid-task derailment; queue owner/reason truthful; unnecessary escalations absent |
| P5: Operational continuity | Presence schedule, provider readiness, rolling updates, DB recovery | P2 recovery contracts; coordinate P4 | Night Watch and desktop return transitions; safe update across stale Queen schema; conversation identity preserved; isolated corruption/restore drill |
| P6: Visual and interaction maturity | Reviewed desktop/mobile mocks, coherent controls, runtime feed, simplified surfaces, optional briefing | Early mock review can precede implementation; shared state from P1/P4 | Operator-approved visuals, cute identity retained, accessible critical paths, no measured performance regression |
| P7: Integrated acceptance | Daily-driver evidence bundle and remaining-risk report | Delivered candidates | Full workday plus normal overnight/mobile use on latest build; no blocking recovery/data-loss regression; operator accepts maturity outcome |

### Recommended sequence for approval

Use P0 through P7 in the order above as one overall goal, with these refinements:

1. **P0 is a short evidence/setup phase, not another interview.** Refresh upstream,
   verify deployed revisions, build a requirement-to-code checklist, reproduce
   priority failures, and record baselines. Commit the reconciled scope and ADRs.
   A failed reproduction remains an open verification item, not a closed defect.
2. **P1 delivers minimum useful instrumentation first.** Add browser/server timing
   and bounded incident capture before building elaborate historical dashboards.
   Commit the measurement foundation; extend Dogfood views alongside later phases.
3. **P2 addresses the highest-friction journeys.** Device cutover, PWA reconnect,
   mobile AskUser, silent attachment failure, and input preservation. Bring the
   conversation recovery fallback into this phase because it protects continuity;
   P5 then exercises it through updates/shutdown rather than implementing it late.
4. **P3 reduces measured cost.** Compare eviction/pacing/subscription experiments
   against P1/P2 evidence. Ship only experiments that meet UX gates. A rejected
   experiment with evidence is a valid outcome, not unfinished optimization work.
5. **P4 makes orchestration and attention trustworthy.** Deliver machine-verifiable
   completion, Queen exception handling, protected engagement, verifiable operator
   input, next-move ownership, and decision reconciliation together with their UI.
   Needs You remains an open product defect until the actual live contents pass.
6. **P5 closes unattended and update recovery.** Presence scheduling, safe rolling
   update convergence including Queen, provider maturity gates, database backup/
   restore and shutdown proof. Reuse the conversation contract completed in P2.
7. **P6 completes the visual pass, but design starts in P0/P1.** Review desktop and
   mobile mockups early. Apply approved controls and layout patterns as their
   functional phases land; use P6 for whole-product coherence, accessibility, and
   remaining polish. Do not defer all UX work until after the architecture work.
8. **P7 consolidates evidence, not starts testing.** Soak each approved deployed
   phase through normal operator use; compare by revision. The final checkpoint
   closes cross-phase regressions and records accepted residual limits.

### Commit and release checkpoints

Recommended cadence: one or more coherent implementation commits inside a large
phase, followed by an explicit phase-completion checkpoint. Do not accumulate a
large unverified diff solely to achieve exactly one commit per phase. Each phase
must be independently reviewable and leave the product usable.

Before its checkpoint: run affected checks, exercise relevant failure/recovery,
record evidence and outstanding live verification, review the diff, then commit
once execution/commit authority is granted. Reconcile upstream again before the
next phase. A checkpoint commit is not a deployment or a claim of live acceptance.

Use the existing safe App/API update path only under granted deployment authority.
If a phase needs a schema migration, session interruption, provider reconnect, or
engine replacement, identify that consequence before rollout. Never use an
incompatible database rollback to undo a code deployment.

Delivery sequence remains proposed until operator approval. No runtime work,
commits, push, or deployment has been performed by this planning task.

Tests follow risk, not every theoretical vector. Use focused unit/domain tests,
failure/recovery integration tests, real renderer fixtures, a small set of browser
journeys, and the operator's ongoing soak. Prefer current `scripts/verify.sh`
entrypoints (introduced in `d9680c7`) over a second verification recipe; inspect
them before execution. Avoid full-suite repetition for unrelated documentation.

For live acceptance use a separate Edge tab, disposable sessions for intrusive
fixtures, and read-only host profiling. Never wake/restart the operator's real
workers just to test. Local, deployed App/API, worker engine, provider version,
and cached schema generation are recorded independently. A passing local unit
test or a released fix is not proof the current live session received it.

Each commit checkpoint records: requirement IDs, files/ADRs changed, checks run,
real-device evidence, before/after metrics, fallback/rollback procedure, known
limits, and unverified items. Refresh recent commits before each phase to avoid
duplicating concurrent work. Do not revert unrelated user changes.

### Program completion

- No recurring 4–8 second stalls in the agreed normal-workload acceptance runs;
  report tails and exceptions honestly rather than silently excluding samples.
- Resource usage settles under equivalent long-running work instead of growing
  with every visit. Quantitative CPU/heap budgets are calibrated from baseline,
  not invented universal percentages.
- Mobile/desktop cutover is stable, drafts are not silently lost, attachments
  have explicit outcomes, and AskUser remains readable across all questions.
- Updates/recovery preserve sessions where promised and never silently substitute
  a new conversation. Provider inability to resume is handled explicitly.
- Needs You is trustworthy; Queues identifies the next owner/reason; Queen moves
  work within authority without operator approval solely to keep the system alive.
- Diagnostics distinguish browser/network/server/provider causes with honest
  uncertainty, and self-resolved incidents consume no operator attention.
- UX is coherent and recognizably Swarm. Experimental optimizations pass their
  own UX gates. Outstanding accepted deferrals are named, not marked complete.

## 10. Conflicts, corrections, and targeted decisions

All three final product reconciliations are settled. Remaining differences from
existing implementation are engineering/ADR work, not open interview questions.

1. **Missing Claude conversation (resolved, REC-01).** Final operator answer:
   safe recovery first, then provider-native `--continue`, then a fresh session
   as the last attempt. The operator can use the provider's resume command from
   that session. Retain provider-authoritative detection; do not jump directly
   from missing exact id to fresh context or claim fresh means restored. This
   replaces the proposed mandatory confirmation before starting fresh.
2. **Completion authority (resolved, QUEEN-01).** Reliable machine-checkable
   evidence completes routine work without mandatory Queen review. Queen owns
   exceptions, conflicting evidence, and genuine judgment. Unsupported worker
   self-approval remains insufficient. Recent patches do not supersede this target.
3. **Blocked escalation (resolved, ATT-01).** Replace blanket twelve-hour
   escalation. Queen elevates because she cannot move the work and needs the
   operator, not because a timer expired. Age remains visible in Queues and
   diagnostics. Self-resolved problems consume no operator attention.

Architecture amendments already implied by approved outcomes: explicit Resume Here
versus input-driven takeover (ADRs 0012/0045); scheduled Night Watch ending on
desktop return versus indefinitely dominant manual override (ADR 0018); diagnostic
consequence routing versus existing pressure refusal escalation (ADR 0058).

No silent relaxation of update authority: preserve current migration/production
and session-ending safeguards until the rolling-update design reconciles them.
No exact mock from the long interview is treated as approved merely because the
operator agreed to see one. Any unavailable earlier visual must be recreated and
reviewed rather than fabricated as an approved artifact.

## 11. Immediate next step

Review this full scope, then choose phase order and authorize execution/commit
cadence for the overall goal. The interview and final reconciliations are complete.
The planning
deliverable is complete independently of the pending authenticated live baseline.
