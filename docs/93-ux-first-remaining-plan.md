# Remaining maturity work — UX/UI first

Operator-directed priority change: September 9, 2026.

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
  Full CI run `34391815461` must finish before its gate is counted passed.
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
