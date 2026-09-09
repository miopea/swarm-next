# Remaining maturity work — UX/UI first

Operator-directed priority change: September 9, 2026.

**Design acceptance principle:** developer users should get the right information
at the point of action, with clear, logical, easy-to-use controls. Keep Swarm's
adorable Hive identity through lightweight visual personality, without obscuring
technical meaning or adding expensive effects. Judge increments by improved
understanding and task completion, not visual novelty. The operator explicitly
endorsed this direction; ordinary-user improvements remain ahead of reload-only
developer inconveniences.

Worker setup follow-through: the roster now exposes a named accessible group
for each worker, keeping its Edit/reorder/editor controls associated with that
identity without changing the compact visible layout or bee artwork. The real
Edge fixture verified opening Field Notes and cancelling back to its same group.
All 25 WorkerSettings tests pass, including group identity and cancellation;
TypeScript and web build pass. Together with a17d7b72 this is queued locally.
CI 34408932476 exposed one missed Settings integration assertion from the prior
diagnostics change: its epoch-second-1 sample must say Last known metrics, not
Live metrics. Update that assertion, retain it after refresh, and run the full
web suite before publishing. This is not a runtime regression or a full-program
completion claim. Verification now passes: all 154 web test files / 1,488 tests,
plus the earlier TypeScript and production build. Phone-width Edge inspection
retains readable worker names, bees, and separate edit/reorder controls. CI's
prior run has completed (web failed on that old assertion); publish this
corrected batch and verify the app-only deployment next.

Deployment verified: `55e51327`, live
`1.6.0-dev-55e513276534-20260909221157-3486945`, healthy with no degraded
services. All 34 worker identity projections (12 running) and engine PID/start
match exactly before/after; evidence `/tmp/swarm-worker-ux-deploy.4BdrNM`.
Provider conversation IDs are not exposed by that projection. CI 34410878270 is
running; do not cancel it by immediately pushing the next increment.

Next queued visible fix: the phone worker editor inherited roster-summary
ellipsis styling, clipping bee guidance and other small explanatory text.
Scope wrapping to editor small text only, preserving compact closed rows.
Edge 390px phone fixture shows the entire bee explanation after the fix;
61 style/worker tests and production web build pass. Queue for the next coherent
publication after current CI; continue UI-3/4 everyday journeys meanwhile.

**Latest operator direction, evening:** stop the reload investigation. Prioritize
visible improvements for ordinary users with one to five workers; incremental
updates are welcome. Finish the already-running search-layout deployment, then
work through Settings/navigation, mobile controls/attachments, readable messages
and integrated layout consistency. Do not substitute a deeper reconciliation or
performance project for this user-facing pass. Those unfinished requirements
stay recorded, not silently dropped or represented as completed. This sequencing
supersedes the older reload-first pointers below and in the execution log.

Search-layout deployment completed: live version
`1.6.0-dev-f23bb3cba88c-20260909210727-3415103`, healthy with no degraded
services. The operator's engine update finished first (PID 3408834, start
17:01:49 EDT); this app-only deployment preserved that exact engine PID/start
and all 34 worker projections, including twelve running session/provider
identities. Evidence: `/tmp/swarm-terminal-search-deploy.zi0a90`.
No provider conversation-ID proof is implied by the worker-list projection.
Next rendered surface: Settings/navigation for everyday users, not reload work.

### Everyday Settings search increment

Live Edge reproduced a discovery defect: `night watch` showed Queen policy but
not the actual schedule. Search now indexes the visible presence/schedule,
opening-screen and notification labels, and matches all query words across one
card's existing title/section/keywords. `night watch schedule`, `dark theme`,
`opening screen` and `phone notifications` find their expected controls. Unknown
combinations stay empty; developer cards remain hidden outside development mode.
No fuzzy-search service, new polling or backend changes were introduced.

The phone fixture exposed duplicate native/custom clear buttons. Hide the native
clear affordance as the worker search already does; give Settings search and its
single labelled clear action 44px mobile targets. Full-App fictional desktop and
390x844 phone checks cover schedule discovery, light/dark appearance, notification
search, no-match state, clear and Escape recovery. The phone fixture is not a
claim about physical-device keyboard behavior. Focused tests: 11 navigation and
25 SettingsWorkspace tests pass; TypeScript and production web build pass.
This increment is local pending commit/deployment verification. Previous search-
layout CI 34405066863 remains in progress; do not cancel it just to publish notes.

### Clear restart consequences

The full-App Updates fixture showed two running sessions but the unavailable-
engine confirmation said `Restart 0 workers now?`. The confirmation now explicitly
says all loaded workers when the count is unknown and warns that running work
can still be interrupted. Confirmed zero and positive counts remain distinct.
Recovery wording describes an attempt, not a guarantee of restored conversation.
The execution path is unchanged. Desktop/phone fixture inspection covers the
warning and Not now cancellation; no live worker restart was requested.
SettingsWorkspace now has 28 passing tests, including unknown/zero/two-worker
counts and cancellation without a callback. TypeScript and web build pass.
Prior CI 34405066863 has now passed completely; the queued Settings changes can
be published without cancelling it.

Settings increments are deployed as `a716dad8`, live
`1.6.0-dev-a716dad84cea-20260909212842-3442412`. Health is good and all 34 worker
identity projections (twelve running) plus engine PID/start match the before
snapshot. Evidence: `/tmp/swarm-settings-ux-deploy.fzEfrZ`. Current CI is
34407100626, in progress; prior f23bb3cb CI passed. The phone support-form
review/cancel check remains unverified after browser-control timeouts and fields
not retaining automated fills; this is not established as a product defect.
No support submission or worker restart was performed. Continue ordinary-user
UI-3/4 journey checks, not reload profiling or BFG Admin coordination.

### Support draft-state clarity

The support form previously promised an unsent retry copy even before it had
saved one, while its discard confirmation correctly said the draft was unsaved.
The privacy note now distinguishes an unsaved draft (reload may lose it), a
retained exact retry, and a confirmed Hive save (the obsolete retry note clears).
No persistence or sending behavior changed. Fourteen support/file tests pass,
including those three states, and TypeScript/production build pass.
In Edge's isolated support fixture, closing an unsaved draft asks for confirmation
and Escape returns to the intact form. Existing fictional retained-file setup
shows the exact two attachments; its first simulated save failure preserves the
retry, and the second reaches the fictional Hive receipt. No external send or
BFG Admin communication occurred. Initial email-field automation and real phone
picker/keyboard acceptance remain separate, not proven by this seeded retry.
This increment is local pending publication; a716dad8 CI is still in progress.

### Integrated Settings evidence checkpoint

The diagnostics fixture exposed a stale sample headed `Live metrics`, above a
correct stale-evidence warning. Its heading/indicator now reuse the existing
performance assessment: fresh, last-known (stale or clock mismatch), or unavailable.
Old machine readings are labelled `Last sample`; no monitoring or pressure rules
changed. Edge verifies the stale heading, old machine reading and evidence agree.
Twenty-five diagnostics/assessment tests pass, covering fresh/stale/future/missing
samples. TypeScript and production build pass. Prior a716dad8 CI has now passed.

| Ordinary-user journey | Current evidence | Remaining gate |
| --- | --- | --- |
| Settings discovery and search recovery | Desktop/390px light/dark; schedule/theme/notification searches, empty result, clear/Escape; deployed a716dad8 | None for this increment |
| Restart decision and cancellation | Unknown/zero/positive count tests; desktop/phone warning and Not now; deployed a716dad8 | Does not prove actual engine recovery |
| Support draft and retry | Discard/Escape; exact fictional attachment review, failed save/retry/receipt; 14 tests | Native picker and initial contact-field browser automation unresolved |
| Night Watch form | 3 validation/recovery tests pass; visible schedule and timezone copy checked | Browser text replacement was unreliable; corrected-save browser journey not claimed; physical return behavior remains |
| Diagnostics preview | Visible fixture preview, distinct browser/server evidence, stale-state correction; no copy/upload | Current stale-state correction pending deployment |

This is an incremental evidence matrix, not closure of UI-3, UI-4 or the overall
goal. Next focus is ordinary-user worker/navigation/dialog usability. Do not
resume reload profiling or contact BFG Admin. Automated schedule saves in this
pass were confined to fictional responses and did not change the live schedule.

Clarity deployment verified: `51cadc40`, live
`1.6.0-dev-51cadc405a76-20260909214905-3459418`, healthy. Engine PID/start and
worker identity projections match before/after snapshots in
`/tmp/swarm-clarity-ux-deploy.lTVzMZ`. No engine activation or release occurred.

### Worker setup empty-state guidance

The roster fixture with no discovered repositories incorrectly said every
repository already had a worker and advertised a future settings location.
The message now distinguishes empty discovery from all discovered repositories
being assigned; both give the supported next step of entering an existing full
project path. The unmatched suggestion also points to that path and the existing
folder warning. No admission, folder-security or provider policy changed.
Edge confirms the empty-discovery guidance. All 24 WorkerSettings tests pass,
including both empty states and preservation of the explicit folder opt-in.
This increment remains local pending publication/deployment verification.

### September 9 evening checkpoint — local terminal layout correction

The operator is applying a worker-engine update. Do not overlap it with another
deployment; re-observe the completed update and use its engine/session identities
as the new baseline before any later app-only deployment.

Fixed a separately reproduced terminal-search layout defect: the stage was a
row flex container, putting search beside the output and squeezing its width.
It now stacks search above a flexible output mount without a competing 100%
height. The handoff fixture now constrains height like the application.
Edge verification: at desktop width 1433px, opening search preserves terminal
width and stage height (1157.56px); the 52px search row reduces mount height
from 1157.56px to 1105.56px. The grid settles from 1120px to 1072px high.
Search failure displays No match, Escape closes search and restores input focus.
The 390x844 fixture keeps search, No match, Close and composer controls readable.
TerminalView's 29 tests, TypeScript and production Vite build pass. This change
is local only until a subsequent deployment is explicitly verified.

This does NOT close desktop reload jumping. The preceding live demo sample
showed about 5.2 seconds before the terminal appeared and about 6.9 seconds
before connected canonical geometry, but no repeated oscillation in that sample.
Source inspection shows browser-session restoration awaits the whole control-room
snapshot (including tasks, decisions and Jira links) before terminal selection.
That is a candidate startup bottleneck, not a measured attribution or a fix.
Next: attribute the startup delay and verify reload/switch stability, then return
to the separately open clarification/direct-worker decision reconciliation gate.

CI 34402184205 for f482b460 has now completed successfully. Its earlier pending
references below are historical; do not rerun that unchanged gate.

Resume checkpoint: UI-1 presentation `eb931114` and UI-2 controls `221e66cd`
are deployed with passing full CI. Runtime safeguard placement is `f482b460`;
its normal dev reload completed with evidence in `/tmp/swarm-ui3-deploy.mlsUrp`.
It is live as `1.6.0-dev-f482b460b146-20260909203810-3396484`, healthy, with
twelve exact running identities and engine PID/start time preserved. Its full CI
is still a separate pending gate; `221e66cd` passed full CI. Next substantive
UI work is the remaining desktop terminal reload/worker-switch stabilization,
then remaining Settings and the integrated finish pass. Do not repeat completed
card/control refinements or restart native-provider experiments. The direct-worker
decision reconciliation and clarification gap remains explicit and uncompleted.

This changes delivery order, not the requirements in [45](45-daily-driver-maturity-plan.md)
or the evidence standard in [92](92-maturity-acceptance-ledger.md). The overall
maturity goal remains open. A user-facing release candidate and completion of
the entire maturity program are distinct outcomes. The operator chooses and cuts
releases; this work does not authorize a release.

## Outcome and working rules

Use Codex's browser work for the visible, daily experience first. Optimize for
ordinary users with one to five terminal workers while preserving the power-user
tasking, Queen and Jira workflows. Keep Swarm's warm, cute, feminine-leaning bee
identity; personality stays visual, lightweight and reduced-motion friendly.

- Work on one coherent user-facing package at a time, through implementation,
  focused checks, browser verification, commit and safe dev deployment.
- Inspect the live surface before changing it. Existing fixes are not new work.
- Use a separate Edge tab at `swarm.bfgsolutions.net`; intrusive interactions
  use fictional local fixtures or the disposable demo workers only.
- Do not let deep engine/performance investigation delay every UI checkpoint.
  Fix backend defects only when they directly block the selected UI journey or
  threaten data/input/session safety; otherwise record an explicit handoff.
- Do not hide backend inconsistencies with optimistic labels, omitted warnings,
  fake progress or a client-side guess about whether work/permission is complete.
- No BFG Admin worker communication without separate operator approval. A handoff
  document is not a message or assignment to another worker.
- Reuse passing evidence; rerun only changed behavior and its affected contracts.
  One final integrated suite/checkpoint follows the packages, not every CSS edit.
- No new architecture experiments, dashboards or optional features merely to
  demonstrate progress. The optional return briefing stays mockup-gated.

## Current verified starting point

- Live App/API: `1.6.0-dev-7c648e4d88f0-20260909183740-3162396`, healthy.
- The deployment preserved all 34 worker records and twelve running session/
  provider identities; engine PID 2947820 remained unchanged. Conversation IDs
  were not available in that worker-list projection, so it is not independent
  proof of provider conversation continuity.
- Source main includes `71a85719`, a test-only correction after CI exposed a
  package-return fixture expecting an outdated engine to permit a return.
  Both corrected current/old-engine cases and strict API checks pass locally.
  Full CI run `34391815461` passed all four jobs. The next UI commit has its own gate.
- September 9 live Edge inspection shows eight Needs You requests. Every card
  has Say something else, but the page remains tall: repeated recommendation/
  action wording, long summaries and generous internal gaps dominate the first
  viewport. Preserve exact decision meaning while improving scanability.
- The live Queues surface already groups owners and workers. It must be finished,
  not rebuilt as though grouping does not exist. Waiting reasons, long titles,
  stalled/active distinctions and access to the next action remain acceptance work.
- The native Claude probe was stopped when priorities changed. One isolated
  interactive 2.1.266 session emitted SessionStart, UserPromptSubmit and Stop
  with empty background/cron arrays. That does not establish safe maintenance:
  background-work and hook-continuation cases were not completed. No real worker
  settings or automatic engine admission were enabled. Preserve
  `scripts/dogfood/native-signal-probe.cjs` as an explicitly non-authoritative,
  content-free diagnostic, not product behavior.

## Delivery order and exit gates

### UI-1 implementation checkpoint — September 9, afternoon

Implemented the first cohesive presentation batch: recommended answers appear
once, explicitly labelled on their action; unmatched advice remains separate;
case-sensitive authored actions cannot be incorrectly highlighted. Empty risk
dividers are removed, but real risks and exact commands stay ahead of actions.
Resolved history says Answered and treats prior advice as historical. Queue rows
show full titles and wait reasons on separate lines within the existing worker
groups. Task detail reuses board status labels and shows recorded next ownership.

Verified in Edge against isolated fixtures: desktop light/dark decision cards;
390-by-844 phone Needs You, custom text, Queues owner jumps and task detail;
quick/custom failed-send retry, exact multiline receipt and cleared attention
count; linked queue prerequisite opens the correct task, with consistent Assigned
status in its detail dialog. No real decision was answered or worker woken.
Focused coverage passes: 45 DecisionInbox, 2 inbox activity, 1 answer fixture,
44 queue and 8 task-detail tests. TypeScript and production web build pass.

Deployed `eb931114` as `1.6.0-dev-eb931114675e-20260909194140-3319797`.
All 34 worker identities and twelve running session/provider identities matched;
engine PID/start time were unchanged. Health is good. Live Edge: the same eight
880px-wide cards are 44–49px shorter without removing answer wording; all 34
queue task titles fit without horizontal clipping. Fixture refresh preserves an
unsent custom answer, and quick navigation focuses its exact card without losing
the draft. A subsequent late-response regression passes (46 inbox tests total).
CI `34396506702` completed successfully: web, linux-package, audit and Rust passed.
Evidence: `/tmp/swarm-ui1-deploy.N3wwzR`. No release or engine update requested.

### UI-1 next: clarification is not a final answer

The operator reported clarifying Needs You items directly with RCG Networks and
resuming work, while Needs You stayed unchanged. A live API check confirmed the
four krbtgt/A1b/A2b/licence requests still Pending, their linked tasks still
Blocked/next owner Operator, and RCG Networks running. This is server-side
reconciliation debt, not just a refresh defect or something the layout fixes close.

Required regression journey: an operator opens an unclear request, asks its
context-owning worker a question, gets clarification, and settles the issue there.
Returning to Needs You must show the authoritative outcome without another answer
or a manual refresh. A clarification alone is not a final decision. A final answer
requires verified operator provenance and exact request/scope correlation. If the
question has instead become obsolete, use explicit audited withdrawal with a
reason, not an invented operator approval. Preserve the original exchange in
history and invalidate the attention count and linked task/queue projections.
Resolving one request must not clear other requests merely because they share a
worker, project or similar wording. Resumed worker activity is never sufficient.

Cover pending clarification, lost/failed send with retained draft, idempotent
retry, a late response after resolution, and direct-worker resolution while the
inbox is open. Do not add an optimistic Done button or treat Say something else
as an informal question: it currently submits a final answer. The direct-answer
backend gap remains separately visible in the handoff; no new native-provider
experiment is authorized merely to make this UI checkpoint look complete.

The current `/decisions/{id}/resolution` action is final; Say something else
also resolves. Do not disguise clarification as a resolution. Finish a distinct
non-authorizing question/reply path attached to the original request, preserving
exact operator text, context and history. A clarification must not grant command
permission or make its request appear answered. Distinguish waiting for a reply
from waiting for an operator decision. Correlate a proven final operator answer
given in the worker to the original decision; resumed activity or a worker's
prose claim alone is not proof. Keep QUEEN-03 open until that provenance and
reconciliation work is verified. This directly blocks the selected UI journey,
so it is an allowed bounded backend exception, not a return to engine work.

UI-1 is therefore not closed. Native-device acceptance also remains separate.

### UI-1 — Needs You, Queues and task detail form one understandable workflow

Requirements: ATT-01, QUEUE-01, user-facing QUEEN-01/03, UX-01.

Remaining implementation and inspection:

1. Make decision cards compact and scannable: clear question, owner/project,
   why the operator is needed, Queen recommendation, immediate actions and
   always-available custom text. Disclose supporting history progressively.
   Do not silently rewrite permission scope or truncate the actionable question.
2. Preserve drafts on failed submission; prevent double send; distinguish saved,
   resolved and delivered states. Verify the same card/count/history updates
   after a confirmed response, including refresh and an overtaken event response.
3. Finish Queues within existing owner sections, then worker and actual task
   order. Each item explains the current hold and what resolves it. Keep moving
   work secondary; show unknown or stale evidence honestly.
4. Make linked task/decision/dependency details useful without a wall of text:
   current status/owner/next move first, concise evidence next, full history on
   demand. Preserve reassignment and the established /task workflow.
5. Keep notification links direct to the relevant Needs You request, with no
   unnecessary intermediate confirmation/navigation page.

Exit: rendered desktop and narrow-screen light/dark evidence; fictional quick
answer and custom-text answer, failed-send recovery, refresh, history and linked
queue/task navigation pass. No real customer/operator decision is answered by a
test. Backend-originated stale/duplicate decisions remain explicitly open if the
source state cannot yet reconcile; a visual improvement does not close QUEEN-03.

### UI-2 — Terminal shell, worker switching and mobile input/attachments

Requirements: TERM-01/02, MOB-01/02, relevant PERF-01/02, UX-01.

September 9 controls checkpoint: optional key-visibility storage can no longer
crash the composer or stop its toggle from working. Drafts explain connecting,
disconnected and viewing-only states without automatic sending. Recovery notices
span the full composer grid instead of falling into its Send-button column.
Terminal toolbars, attachment explanations and tools wrap at narrow widths;
arrow targets use actual 44px grid tracks. Normal 390px arrow/action layout remains
compact. No worker, terminal ownership, input protocol or provider behavior changed.

Focused verification: 32 composer, 29 TerminalView and four draft tests pass;
TypeScript and production build pass. Edge fixtures at 390px in light/dark show
three simultaneous notices, retained multiline text, failed-send recovery, exact
single retry, no send on reconnect/redraw, collapsed keys with accessible redraw,
and real TerminalView Resume Here preserving an unsent draft. Transport and
redraw callbacks in these fixtures are fictional; this is not a provider or
native-device acceptance. These checks do not close the reload-jump, attachment
picker, actual keyboard/suspension or cross-device geometry requirements below.
Deployed `221e66cd` as `1.6.0-dev-221e66cdec78-20260909201955-3383452`.
All 34 worker identities and twelve running session/provider identities match
before/after; engine PID 2947820 and its start time are unchanged. Health is good
and Edge loaded the new runtime. Evidence: `/tmp/swarm-ui2-deploy.G5GcQu`.
Full CI `34400256735` passed all four jobs. No engine update or release occurred.

1. Prioritize the reported desktop reload jump and visible restore latency.
   Reproduce using the actual terminal renderer, recorded geometry/ownership
   and output sequence. Fix browser lifecycle/measurement defects where proven;
   do not add sleeps, hide output or restart the provider to mask them.
2. Preserve configured roster order, chosen worker and newest output on return.
   Verify focus, sidebar density, quick navigation and the supported shortcut
   fallback. Do not steal terminal/provider keys or promise browser-reserved
   Ctrl+Tab behavior the platform will not deliver.
3. Finish Resume Here and reconnect presentation: understandable ownership,
   stable dimensions, useful progress/failure, preserved draft and no duplicate
   input. A background/minimized test tab is not operator engagement.
4. Consolidate mobile terminal controls: arrows, Enter, redraw and attach image;
   handle keyboard/viewport changes without covering actions or losing text.
5. Verify visible upload progress, failure/retry and automatic provider insertion
   after success. Do not introduce a second Insert step or worker-switch overhead.
   Shared artifact storage remains the source used by workers and Queen.

Exit: fictional renderer journeys for reload, fixed-order switching, output while
typing, reconnect/refusal and attachment success/failure; no missing or duplicate
bytes. Desktop browser acceptance is recorded separately from real Android/iOS
keyboard, picker, suspension and device handoff. Android AskUser questions 2/3
already have operator acceptance; do not spend another session reproving that
unchanged case. Missing native-device checks stay in the release-risk checklist.

### UI-3 — Settings, runtime and recovery feel like the same product

Requirements: ATT-02, DIAG-01, DOG-01 presentation, PRES-01, PROV-01, UX-01.

September 9 runtime checkpoint: known `wake_not_admitted` observations are now
projected into one collapsed runtime safeguard notice rather than a Needs You
card/count. The resource safety rules and coordinator recovery are unchanged.
Uncertain wakes and unknown hold kinds remain attention; genuine decisions keep
their own cards. A failed observation retains and labels the last-known holds;
authoritative recovery removes the notice without acknowledgement. Reasons are
deduplicated, and diagnostics opens the existing maintenance section.

Seventy-two focused tests pass, including full-App queue/runtime placement and
badge consistency, explicit failure/recovery, and unknown-kind preservation.
TypeScript passes. Edge full-App fictional desktop and 390px phone journeys show
one decision plus a separate runtime start pause, expandable reasons, and correct
diagnostics navigation. The production build passes. This runtime change is now
deployed as recorded at the top of this plan; its full CI remains pending. This
does not close the entire Settings/finish package.

1. Use the runtime area as the compact home for system state. Avoid duplicate
   warnings and conflicting attention counts; disappearance after recovery needs
   no operator acknowledgement. Critical unresolved actions remain accessible.
2. Keep browser/server/provider/network evidence distinct. Show what is measured,
   unknown, consequential and actionable. Do not present browser timing as Edge
   CPU or server process totals as browser measurements.
3. Finish Settings hierarchy, readable update/recovery/provider states and the
   separate Developer Dogfood unit using existing dev detection. Ordinary users
   should not face developer-level verbosity by default.
4. Verify At Hive/Reachable/Night Watch controls, scheduling, timezone copy and
   desktop/mobile behavior. Do not change the operator's live schedule for a test.
5. Keep update types and interruption consequences explicit. Do not enable the
   unfinished automatic engine-admission path to make its warning disappear.
6. Verify support form progress/errors/receipt, diagnostic preview and source
   context. BFG Admin remains the customer-send owner; closure of a Swarm task
   does not authorize a send. Use only fictional submissions.

Exit: complete browser journeys for settings navigation, quiet recovery, error
details, diagnostic preview, update confirmation/cancellation and fictional
support state; no unintended service change, provider promotion or customer send.

### UI-4 — Whole-product finish and release-candidate evidence

Requirements: UX-01/P6 and the UI portion of P7.

Review the above together with Workers, Tasks, Apiary, empty/loading/error/offline
states, first/repeat login, dialogs and notification navigation. Check typography,
spacing, contrast, action hierarchy, touch targets, zoom/narrow layout, keyboard
focus/escape, nested modals, screen-reader names and reduced motion. Preserve
identity; show a mock before a materially new unapproved composition.

Exit: one concise desktop/mobile journey matrix with build identity, screenshots,
changed-code tests and explicit device gaps; latest CI result; healthy dev build
and exact worker continuity for deployments. Prepare a short operator dogfood
checklist and a factual release-risk summary. Do not cut a release or call the
entire maturity goal complete from this UI checkpoint.

## Backend/performance handoff — remains in the overall scope

Suitable for a separate Claude/backend worker when the operator assigns it.
Do not send or delegate this document automatically.

| Remaining outcome | Current boundary and next necessary evidence |
| --- | --- |
| Safe automatic engine updates, including Queen (OPS-01) | Admission library and schemas 157/158 exist and return records are deployed. Native settled-turn/background proof, combined durable admission receipts, IPC/package activation, lost-response/partial-stop recovery and an actual safe update/return remain. ADR 0085 is authoritative; do not restart loaded workers from a Resting screen or one Stop hook. |
| Correct chosen-conversation recovery (REC-01) | Finish chosen-conversation switch and safe/resume -> native continue -> clearly identified fresh fallback, with real failure/cancellation evidence. Preserve explicit provider choice; no automatic switching. |
| Queen closes the actual orchestration loop (QUEEN-01/02) | Routine and same-task idle recovery demos passed. Finish Queen-owned Awaiting Release handoff, worker-first escalation, protected-input/background-work cases and unresolved real-blocker reconciliation. Do not manually shuffle tasks to claim the system works. |
| Proven direct answers and one operator question (QUEEN-03/ATT-01) | Composer source records exist; direct terminal/AskUser authored-answer capture and exact decision correlation remain incomplete. Prose claims are not authenticated permission. Link existing decisions only within their approved scope. |
| Sustained efficiency (PERF-01/02, DIAG-01) | Match fresh/aged 10–15-worker workloads; attribute server/engine/browser cost, fix measured tails/leaks, bound instrumentation and verify plateau/output fidelity. Existing passing microbenchmarks and short idle observations are not this acceptance. |
| Useful long-term metrics (DOG-01) | Finish evidence-backed review yield, duplicate-question, recovery/false-alert and update-convergence comparisons. Run counts do not measure useful task progress. |
| Provider/presence acceptance (PROV-01/PRES-01) | Real native-provider journeys, experimental exclusions, manual/scheduled Night Watch and device return behavior. Only the builder promotes providers. |
| BFG linked development/support loop | Text intake is accepted. Finish fictional attachment and linked-task completion/reply approval paths under existing contracts. Prevent duplicates/loops and expose retries. No BFG worker communication without approval and no customer send without Admin approval. |
| Integrated maturity acceptance (P7) | Current-build normal workday, overnight and real-device evidence plus operator acceptance remain. REC-02 isolated corruption/restore acceptance is retained; do not repeat live-database risk to generate activity. |

## Usage discipline and the approximately 5-percent handoff

Read account limits at package boundaries and before beginning expensive work,
not on a tight polling loop. Remaining percent is account-wide and is not a
guaranteed number of tokens or hours. No reset credits remain.

At approximately 10 percent remaining, stop starting new packages. Finish or
safely checkpoint the current change and prepare the handoff. At approximately
5 percent, stop implementation and use the reserve for a durable final record:

- completed versus implemented-only versus unverified versus blocked requirements;
- local/main/live App/API/engine identities, commits, CI, and deployment state;
- exact remaining defects, reproduction, expected behavior and next action;
- outstanding device/mock/operator approvals, with no invented approvals;
- partial changes and running tool/job handles, owners, output paths and cleanup;
- whether the build is a recommended release candidate and every known material
  exception. The operator makes the release decision.

Keep [92](92-maturity-acceptance-ledger.md) current as packages close so that the
5-percent handoff is consolidation rather than another repository-wide audit.
Unfinished requirements remain open in the original goal. Do not reduce the goal,
mark it achieved, purchase credits, or consume a reset to make accounting fit.
