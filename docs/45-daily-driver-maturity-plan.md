# Swarm daily-driver maturity: scope and delivery plan

Status: scope, delivery sequence, implementation, and local phase commits approved.
Execution: active goal; no push, deployment, or releases. Operator tests the dev build and chooses releases.
Date: 2026-09-03. Source: the operator's UX, stability, efficiency, and orchestration interview in this Codex task.
Code baseline: clean `main` fast-forwarded from `a10bf2c` to `36420b3` during planning.

## 1. Overall objective and authority

Make Swarm responsive, stable, self-managing, observable, and delightful through
long daily use, including desktop/mobile handoff and unattended work, while
preserving its cute, feminine-leaning bee/Hive identity and independent worker
engine. Performance and reliability are at least as important as visual polish.

The operator subsequently approved continuous execution and local commits between
phases, without routine approval pauses. Historical proposed/authorization wording
below records planning context; this approval governs execution. No releases or
deployment are authorized. Track actual evidence in `46-maturity-execution.md`.

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
