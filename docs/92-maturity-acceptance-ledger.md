# Current maturity acceptance ledger

Checkpoint: September 9, 2026. Scope remains [45](45-daily-driver-maturity-plan.md),
including BFG Admin integration. This is a routing index over evidence, not a
replacement specification or a declaration that partially verified rows are done.
Historical implementation notes in [47](47-maturity-remaining.md) remain evidence;
an old unchecked deployment note is not automatically current missing code.

## Verified runtime and delivery

- App/API: `1.6.0-dev-4184ae2f157c-20260909112742-2500458`.
- Engine: retained PID 2271655, `cf83c980`; candidate `cf179f73` remains pending.
- Latest App/API deployment preserved all 34 worker records and twelve exact
  running session/conversation pairs. Swarm Next and D365 remained asleep.
- CI through gated-intake `34333537210`, modal `34334885015` and shortcut
  `34335279693` passed. Later acceptance-document CI remains separate.
- No release is authorized. No customer-facing send is authorized by task closure.

## Requirement-by-requirement disposition

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
| OPS-01 | App/API continuity and one planned engine-return journey verified; resource admission guards exist. | Automatic engine-owned all-session admission, durable exact return set and trustworthy native completion/background evidence; ADR 0085 is not implemented. Rolling provider/tool freshness and pressure-to-resumption acceptance remain. |
| PROV-01 | Opt-in framework, host availability and Night Watch exclusions checked; [48](48-provider-acceptance.md). | Required provider journeys remain distinct from framework acceptance. Unavailable alpha CLIs stay unavailable; only the builder promotes providers. |
| UX-01/P6 | Approved visual direction retained; focused rendered queue, runtime, support and task-reader checks. | Coherent complete desktop/mobile accessibility, empty/error/offline and shortcut journeys. Optional return briefing still requires its mockup gate. |
| BFG Admin integration | Actual development-Hive text intake/receipt/reload retention accepted; [86](86-support-ui-acceptance.md). | Native attachments; existing fictional linked-task dispatch/closure and Admin-approved original-channel reply. Admin contract clarification is now active, without new fixture duplication. |
| P7 | Operator dogfooding plus bounded server/demo observations. | Integrated current-build workday/overnight/mobile evidence and operator acceptance. Narrow passing tests do not close this gate. |

## Next work, in dependency order

Current Swarm-owned change: attachment-revision sizing fence validated by a
failing-before regression, 1,435 passing browser tests and the fifteen-worker
synthetic Edge journey (ADR 0062). Development deployment is pending. This
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
