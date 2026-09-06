# Daily-driver maturity: remaining delivery and acceptance

This is the current completion checklist for the approved scope in
`45-daily-driver-maturity-plan.md`. Implementation history is in
`46-maturity-execution.md`. A passing component test does not close a live journey.
The operator authorizes commits and direct development-Hive deployments; releases
remain operator-controlled. The overall goal was restored on 2026-09-05.

## Immediate live defects

### Items-only clipboard attachment normalization (September 6 UTC)

Added regressions with an empty DataTransfer.files collection and files supplied
only by clipboard items. Both failed on the existing implementation: an untyped
PNG and an unknown extension retained an empty media type, unlike picker/drop.
The clipboard fallback now calls the shared chosenAttachment validation rather
than duplicating size checks and bypassing type normalization. Both regressions
now pass; all 92 attachment/view/composer tests and TypeScript checking pass.
This is a reproduced client-path inconsistency, not proof of the original native
camera/gallery failure. Live browser verification and deployment remain pending.

Deployment follow-up: the production frontend build passed with the existing
terminal chunk-size warning. Reload job 3084229 completed successfully, serving
`1.5.0-dev-38d8b0f9ef5c-20260906125142-3084252` with health OK and no degraded
subsystems. All 15 sessions retain fingerprint
`9ddd1bc5a3920bea8da7fd2e188f37635b3bc9e2810c7966153f0fd6b1d6b49e`; engine
identity remains `38453a84c3fa`. No release or worker restart occurred. This does
not turn the synthetic clipboard regression into native-device acceptance.

### Terminal input waits in Queues (September 6 UTC)

The current demo remains Ready/Delivered while its worker reports
awaiting_operator; Queen's latest observed run `01a075ac-e4b1-7893-92fa-52a5508f30b9`
finished no_action. No dependency task or transition was observed for the repeat.
This is not unattended recovery acceptance.

Queues now shows the worker's input-wait observation alongside Ready/Active work
only when worker and assigned session identities match and the worker is running.
An Active task with that observation stays in the visible worker queue instead of
the collapsed ordinary-activity list. Clearing the observation restores normal
presentation. Navigation uses the same projection. This is not a new operator
decision, a changed next owner, a task transition or permission to answer a prompt.
All 33 queue tests and TypeScript checking passed. Live visual verification and
deployment of this slice remain pending.

Pre-deployment source verification corrected the wording: WorkerView derives
awaiting_operator from either held answers or provider activity, so the UI must
not call it a terminal-only observation. It now says Worker reports waiting for
an answer and directs inspection to Needs you or the current prompt without
claiming a task blocker. The 390px Edge fixture shows this alongside visible
worker-owned Active work. All 33 queue tests and the production build passed;
the existing terminal bundle-size warning remains. The earlier wording was not
deployed.

Morning deployment follow-up: job 3080522 completed successfully; runtime
`1.5.0-dev-c78518d99101-20260906124325-3080545` is healthy with no degraded
subsystems. All 15 worker session identities retain fingerprint
`9ddd1bc5a3920bea8da7fd2e188f37635b3bc9e2810c7966153f0fd6b1d6b49e`; engine
identity remains `38453a84c3fa`. The final frontend suite passed 1,262 tests
across 139 files. The overall goal is confirmed active again after the overnight
capacity interruption. The separate Edge tab still requires operator unlock;
signed-in visual and unattended demo acceptance are not claimed.

### Attachment overlap status race (September 6 UTC)

An oversized second selection could overwrite the status of an existing upload
or reconnect-waiting attachment with a size error. A normal second selection
was silently ignored. Both now retain the first operation and show a separate
explanation that another file was not added. A standalone size refusal also
clears any older retry-file identity rather than offering the wrong file.
Cancel, removal, session replacement and notice dismissal clear the explanation.
No upload concurrency, automatic submission or worker-switching behavior added.

All 90 attachment/view/mobile-composer tests passed, including oversized overlap
during upload and reconnect wait, exact single insertion of the original path,
and notice dismissal. TypeScript checking passed before the final test-only
addition. Native camera/gallery and live browser acceptance remain open; this
fix is not evidence that the original phone-picker problem is fully solved.
Deployment is pending.

Deployment follow-up: the final production frontend build passed, retaining the
existing 500 kB terminal-chunk warning. Reload job 2988990 completed successfully;
health serves `1.5.0-dev-f0bef077c507-20260906075541-2989013` with no degraded
subsystems. All 15 running sessions retain fingerprint
`9ddd1bc5a3920bea8da7fd2e188f37635b3bc9e2810c7966153f0fd6b1d6b49e`, and engine
identity remains `38453a84c3fa`. No release or worker restart occurred.

### Mixed prerequisite queue scanning (September 6 UTC)

Tasks with mixed prerequisite states now show unresolved dependencies first and
fold completed ones separately. Previously all rows remained expanded whenever
even one dependency was unresolved. Removed Completed prerequisites remain
unresolved; reopened dependencies become visible on refresh. No task or owner
transition is inferred. All 31 queue tests and the final TypeScript check passed.
The 390px Edge fixture confirmed the pending access dependency visible and the
completed schema dependency folded. This is presentation evidence, not live
Queen recovery. Deployment of this change is still pending.

Deployment follow-up: reload job 2984916 completed successfully and health now
serves `1.5.0-dev-4693535b8d14-20260906074712-2984939`, with no degraded
subsystems or database recovery requirement. All 15 running session identities
retain fingerprint `9ddd1bc5a3920bea8da7fd2e188f37635b3bc9e2810c7966153f0fd6b1d6b49e`;
engine build identity remains `38453a84c3fa`. The complete current frontend suite
passed 1,258 tests across 139 files. No release was cut. Signed-in live visual
acceptance and the larger QUEUE-01 recovery journey remain separate open gates.

### Read-only 15-session resource checkpoint (September 6 UTC)

Run `20260906T073816Z-live` completed the existing 60-second observer without
service or session changes: six samples spanning 51 seconds, all 15 original
sessions running, API PID 2971127 and host PID 2876265 unchanged. API build is
`5877c8c1d54d`; host reports `3bfdcc3ec112`. CPU deltas averaged 0.57% of one
core for the API (maximum interval 1.40%) and 19.30% for the host cgroup including
all provider workers (maximum interval 25.88%). These are not percentages of
the whole eight-core machine or measurements of the engine alone.

API cgroup memory ranged 132,665,344–133,124,096 bytes; host-plus-workers ranged
6,475,927,552–6,506,606,592 bytes. Retained history grew from 536,128,417 to
536,137,683 bytes, with zero reported drops. Collection took 0–1 whole seconds
per sample; this is coarse collector duration, not measured instrumentation CPU.
The report and CSV remain under the remote `~/.local/state/swarm-next/soak/`
directory with this run ID. No browser CPU, burst workload or long-session
plateau was measured, so PERF-01/02 remain open.

Direct-answer integration reinspection still finds no production caller of
`resolve_operator_statement_interview`. The current official Claude hook
[reference](https://code.claude.com/docs/en/hooks#posttooluse) describes completed
tool input/result observations but does not by itself authenticate human origin
or supply Swarm decision identity. Its display hook handles assistant text, not
operator input; tool deferral is non-interactive only. These are not substitutes
for ADR 0065's remaining correlation boundary. No hook was installed or native
interactive session replaced to bypass that boundary.

### Custom-answer browser recovery check (September 6 UTC)

The approved always-available alternative answer remains in DecisionInbox and
each interview question. Both existing decision suites passed (47 tests).
Added `needs-you-answer-recovery`, an invented in-memory fixture using the actual
DecisionInbox: its first submission fails and retry records a visible receipt.
An Edge check through the 390px mobile-harness iframe opened Say something else,
entered a multiline answer, observed the failed-send alert, retried, and read
back the exact answer including its newline. A regression independently checks
the draft survives failure and that the successful send control disappears.
The new test and TypeScript check passed. No live decision was resolved.
Native keyboard/PWA behavior and actual worker delivery remain separate gates.
The separate live Edge tab still requires operator unlock.

### Verified earlier dependency journey; current-build repeat (September 6 UTC)

The earlier exercise is now verified from task history and committed files:
consumer `01a07193-7d88-7cb1-888a-602a0e533cb3` and Queen-created upstream
`01a07199-fef1-76e0-b908-4ad528774904` are Completed with evidence. Queen
recorded the prerequisite and performed Blocked -> Ready at 1788614360; the
consumer became Active 20 seconds later and was machine-settled at 1788614462.
Upstream completed at 1788612762, so recovery waited 1,598 seconds (26m38s).
This proves routing, explicit dependency, Queen resumption and documentation
settlement for that run, not acceptable current-build latency or the full scope.
The earlier initial folder-trust assistance remains a caveat. Both repositories
are clean. Consumer commit `728c67abb78b` cites upstream `fe873a1dde6e`; both
files were read back and agree on version 1, status and checked_at.

A current-build repeat is task `01a07595-3a79-7221-852e-d8f5fefa60ed`, created
1788679109 and assigned to the already-running, unengaged Swarm Dogfood worker.
Initial briefing is Delivered. Only the two demo repos are in scope; no push or
deployment is requested of those workers. The new consumer must ask Queen to
create/assign the upstream, add the edge, and resume it after completion. The
operator harness does not supply the edge or clear the blocker. Expected new
files are recovery-contract-20260906.md (upstream) and
dependency-return-20260906.md (consumer). Continue observing this exact task,
not creating another copy. This repeat is still in progress.

Current repeat readiness finding: the provider accepted the briefing, then
stopped at a one-time permission prompt for `swarm_list_tasks`. The host write
audit confirms coordination text (264 bytes) and submit (1 byte) acknowledged
at 1788679109/1788679110. This is not a missing-Enter reproduction. The task
remains Ready; dependency recovery has not started, so elapsed time is not a
valid dependency-recovery latency measurement. Engine metadata reports exact
restoration of conversation `019ff8e1-4a2d-7a11-acff-6a10fb57af3e`, matching
both selected and confirmed revision 1; conversation freshness reports current.
Older visible transcript dates alone do not establish a wrong-session defect.

A scoped one-time Yes attempt through HTTP input returned 422. Write-audit
sequence 33 confirms the operator submit was rejected at 1788679706; no retry
or persistent permission grant was made. Continue via the supported terminal
interaction path after verifying its ownership requirements, rather than
bypassing the write guard. This remains an assisted readiness gate, not proof
of unattended completion or a reason to create another test task.

### Fresh terminal evidence beside historical Queen attention (September 6 UTC)

Implemented read-time observations for stale Active, delivered-but-unstarted,
and never-briefed attention. Saved reasons are explicitly historical; terminal
activity/background-work visibility is a separate observation with its own time.
The saved age is not represented as continuous measured inactivity. Exact
session identity comes from the persistence projection. At most 32 observations,
eight concurrently, have a two-second host-read deadline each; unavailable
sessions remain unavailable. Durable eligibility is reread after asynchronous
observation. No input, dispatch, task transition, periodic sampler or cache added.

Isolated Linux release-profile validation: four API tests passed (two new
evidence/boundary tests plus existing Queen-only access and assignment-recovery
tests); all 51 coordinator persistence tests passed. Initial async lifetime
compile failure was corrected by owning the bounded input batch. Changed Rust
files were formatted separately; whole-tree fmt still reports unrelated existing
formatting in maintenance.rs and terminal_control.rs, left untouched.

Final exact-source `cargo check --release --locked -p swarm-api` passed.
Follow-up Unix-socket regression passed through the actual HostClient: successive
Resting/Active snapshots refresh the same historical row; wrong-session and
stopped-session responses are unavailable; an unresponsive host observes EOF
after timeout cancellation. No Write request or task/worker state change occurs.
All five coordination-attention API tests and 11 provider-activity tests pass.
The worker-engine fingerprint remains `38453a84c3fa`.

Deployed `1.5.0-dev-5877c8c1d54d-20260906071050-2970067`; job 2970044
completed successfully, with fresh health OK and no degraded subsystems. The
15-session fingerprint remains `9ddd1bc5a3920bea8da7fd2e188f37635b3bc9e2810c7966153f0fd6b1d6b49e`
and the engine identity is unchanged. This also deploys the Active assignment
switcher wording. No release cut. Queen naturally began run
`01a07590-0ddd-7f41-9ce6-3735da2a9b18`, which finished `no_action` 13 seconds
after delivery; that is liveness evidence, not yet proof
of consuming the new observations or successfully recovering a stalled task. The
durable actionable fingerprint remains unchanged; this does not claim to clear
all stale records or prove unattended recovery. Physical-device and broader
maturity gates remain open.

### Returned-work status is not execution evidence (September 6 UTC)

Task metadata formerly called every Active task In progress, including returned
work whose new briefing was queued, sending, or uncertain. It now labels those
three cases Awaiting briefing, Sending briefing, and Delivery unconfirmed.
Delivered or absent briefing metadata preserves the explicit task lifecycle;
later Blocked/Review states remain authoritative. This is presentation only:
no dispatch, task transition, worker wake, or concurrency guard changes.

Validation: all 57 task-board tests and TypeScript check pass, including five
delivery variants and transition back to ordinary/Blocked status. The isolated
Edge fixture renders the real metadata with synthetic states. Deployed as
`1.5.0-dev-98239bbf4c3d-20260906064614-2954952`; job 2954928 completed
successfully and fresh health has no degraded subsystems. The sorted running
session-ID array SHA-256 remains
`9ddd1bc5a3920bea8da7fd2e188f37635b3bc9e2810c7966153f0fd6b1d6b49e`
before/after, and engine build `38453a84c3fa` is unchanged. No release cut.
Live read-only evidence still shows held Platform/D365 briefs naming their
actual Active task IDs. That proves the hold's identity, not provider progress;
idle-Active reconciliation and complete waking/delivery/execution acceptance
remain open. No real task was changed during this inspection.

Follow-up: the switcher also relabeled a resting provider Working based only on
an Active assignment. It now says Active assignment, preserving operator-wait
and background-work priority. All 26 worker-attention/roster tests and TypeScript
check pass; this switcher follow-up shipped with `5877c8c1` above.

Live orchestration inspection found Queen enabled, latest run completed with
`no_action`, 62 actionable records, no delivery hold, and automatic admission
allowed. This does not prove Queen's judgments or equate those 62 records with
62 stalled tasks. Source trace: `observe_stale_owned_work` records an observation;
`current_coordinator_attention` revalidates task revision and session identity,
but does not refresh the observed terminal condition when returning the saved
reason. A provider resuming without a task-history change can therefore leave
old resting evidence in the response. Next verification should exercise that
transition and distinguish historical observations from current inactivity,
without auto-unblocking or escalating on elapsed time alone.

### Bounded GPU renderer recovery (September 6 UTC, deployed)

Inspection found that a lost GPU context fell back permanently until the browser
renderer was recreated. The surface now makes one recovery attempt when a
previously working context returns to an attached, visible view. Initial GPU
unavailability and failed recovery remain on the fallback; no timer, socket
replacement, worker restart, or geometry change is introduced.

Validation: 85 XtermSurface/TerminalController tests, the complete 1,250-test
frontend suite (137 files), and TypeScript check passed.
Coverage includes successful recovery, stale context callbacks, activation
failure, loss during activation, unavailable GPU, hidden/detached views, disposal,
and usable terminal writes after failure. These use addon doubles.

Edge isolated fixture verification used actual WebGL and its public
`WEBGL_lose_context` extension: before loss, one live context and no DOM renderer;
after loss, zero live contexts and one DOM renderer; after switching to another
synthetic worker and back, one live context and no DOM renderer. This exercises
the real addon and controller, not a physical driver reset or a native-phone
background cycle. Harness GPU use is explicit opt-in (`gpu=enabled`); default
photographic fixtures still use DOM rendering.

Deployed `3c61f1f26e88`, runtime
`1.5.0-dev-3c61f1f26e88-20260906063344-2949329`: reload job 2949306
completed successfully; fresh health is OK with no degraded subsystems. All 15
running session IDs and engine build `38453a84c3fa` match the immediate pre-build
baseline. No release cut. The fresh live Edge tab requires sign-in, so
authenticated live-browser acceptance remains pending operator unlock; no token
was retrieved for browser entry. Physical-device loss/background cycles remain
open. GitHub reported four expected status checks bypassed on the authorized
main push; local test success is not claimed as completed CI.
The earlier live 15-worker samples did not observe a fallback, so this is not
claimed as the cause or a measured fix for overall sluggishness.

### On-demand heap evidence for retention comparisons (September 6 UTC)

Developer Dogfood can sample the browser's optional Chromium legacy heap
estimate alongside renderer counts. This reads three numeric properties only:
no new timer, heap scan, forced GC, hourly upload or automatic eviction policy.
Unsupported, throwing or inconsistent data is unavailable, not zero. The
preview distinguishes browser build from server version. The adapter is owned
by Developer Dogfood and should be removed when a supported cross-browser
replacement covers this measurement. It cannot be used for admission or alerts.

The estimate is not browser-process or GPU memory and can over/under-count;
see [the API limitations](https://developer.mozilla.org/en-US/docs/Web/API/Performance/memory).
Fifteen focused validation/UI tests and TypeScript pass. In the isolated Edge
DOM-renderer fixture, samples were 21.4 MiB used/25.6 allocated with one renderer,
24.7/33.8 with eight, 25.4/27.6 with fifteen, and 25.2/28.0 after a second full
fifteen-terminal pass. Each visit waited for connected state. This short fixture
run is not a live GPU measurement, long-session plateau or a warm-pool rollout
decision. The default retention policy remains unchanged.

`75df589e` deployed healthy; all 15 worker session IDs remained unchanged. In a
fresh separate signed-in Edge tab, the real canvas-backed terminal baseline was
28.4 MiB used/30.4 allocated with one retained renderer. A view-only pass through
all 15 loaded workers measured 78.6/80.0 with 15 retained, and a second pass
94.3/115.9; a later unchanged-tab sample was 101.6/122.9. Every worker showed
two canvases and no DOM-row renderer on both visits. The captured terminal
connection evidence was 15 samples, mean 331 ms/max 678 ms; state application
47 samples, mean 6 ms/max 29 ms. These are not input latency or confirmed paint.

The five-renderer experiment on that same tab then completed 20 cold returns:
p95 328 ms, maximum 375 ms; 20 attempted, zero failed/interrupted/pending. The
slowest paired return was 20 ms setup and 355 ms connection through applied
state; setup was 17 ms opening, 2 ms fonts and 1 ms layout. Five inactive
renderers remained with 30 evictions (including the initial trim), and the
heap estimate was 82.4/105.4 MiB. This passes the observed desktop 500 ms gate,
not native mobile acceptance or a controlled memory-savings claim. The test
tab's experiment was stopped afterward; no worker task or terminal input was
changed. Keep the policy opt-in pending mobile/correlated resource validation.

Twice, clicking Sample immediately after opening Dogfood hit the browser-tool
three-second deadline before sampling; a subsequent read showed not sampled,
and retry succeeded. Native Event Timing later included a 15,304 ms duration
with about 1.9 ms input delay, 0.2 ms processing and 15,301.9 ms estimated
presentation. Automation/browser scheduling can affect this; it is not proof
of 15 seconds of application CPU. Source inspection also shows context-loss
fallback never reacquires GPU until renderer recreation. This is a recovery
candidate, but the two live passes did not reproduce fallback. Do not claim a
GPU-context cause or a heap leak from these observations.

### Browser update observation ownership (September 6 UTC)

The pre-existing mismatch notice is now in the runtime area, with concise copy
and a warning to finish/save unsent forms before its explicit reload action.
No duplicate notice or forced reload of other tabs was added. The initiating
build watcher now has one cancellable owner, eight-second request deadlines,
and a twenty-minute wall-clock observation limit. Logout, unmount, replacement
and rejected build requests cannot leave an old observer navigating a later
session. Server builds are not cancelled by browser observation cancellation.

Seventy focused frontend checks pass, including App integration, timeout
recovery, late responses after cancellation, install-failure evidence and
per-version dismissal. TypeScript passes. The 390px Edge fixture renders cleanly.
This does not prove that the preceding live mismatch was stale assets rather
than stale coordinator data; that attribution remains unconfirmed. Automatic
live update acceptance and unsaved-form preservation remain broader gates.

The complete frontend suite passes 1,234 tests across 136 files. `3a567a42`
deployed healthy with the exact same engine and all 15 session IDs. A separate
Edge tab retained the previous build during installation, then correctly showed
its existing mismatch notice when the new server answered. Clicking Reload
loaded the new version, cleared the notice and retained trusted authentication.
This proves that observed non-initiating-tab journey, not the earlier ambiguous
observation's cause. The progress copy no longer promises every open tab will
reload itself. Its 30 projection tests and TypeScript check pass.

The follow-up `d7942a85` deployment is also healthy with the same engine and all
15 session IDs. This time the tab running `3a567a42` showed the new Browser
update ready notice inside Runtime and system status; Reload this tab loaded
`d7942a85` and cleared the notice without another login. No releases were cut.

### Held briefings identify the actual blocking task (September 6 UTC)

Live inspection found four worker-busy briefings whose projection named an
earlier Ready task or nothing, rather than the Active assignment preventing
dispatch. The persistence read now returns the exact Active task title and ID
for that reason, or the earlier Ready task for queue-order holds. Engagement,
provider and prerequisite holds do not attribute their cause to unrelated work.
No scheduling or task state transition changes. Queues and the briefing list
offer a quiet, 44px-high "Open blocking task" action. Older API responses retain
the generic busy description rather than mislabeling a Ready predecessor Active.

The 26 Linux persistence dispatch tests and 39 frontend queue/list tests pass;
TypeScript passes. A separate Edge tab verified the isolated 390px-wide fixture
with a wrapping title and compact link. This is not native mobile acceptance.
Rust formatting and isolated Linux API compilation also pass. The existing
Needs You inbox/interview checks pass 47 tests, including custom-answer retry.

`e2a2ee7f` deployed through the development reload service. Health is clean;
the actual engine identity and all 15 running session IDs match the pre-build
baseline. The live coordinator now identifies Platform's memberStatus Active
task and D365's head-of-house Active task for their four held briefings. In a
separate Edge tab, Open blocking task focused Platform's correct task article,
visible in the viewport, without changing task state or sending terminal input.

The first installation attempt failed with an empty detail while the server
disk had only 125 MB free. Removing only our reproducible isolated Rust target
cache recovered about 1.5 GB; one retry then installed successfully. The empty
failure detail remains an operational diagnostics gap, and disk headroom remains
limited. No worker data, repositories or database files were removed.

An already-open browser tab displayed the new runtime version but retained the
old queue presentation until explicit navigation loaded the new assets. A fresh
page showed the new blocker links. Automatic update consistency therefore
remains unverified; do not infer frontend refresh from the server version label.

### Canonical application no longer awaits follow-up sizing (September 6 UTC)

`c3246a71` is deployed and healthy; the actual engine and all 15 session IDs
matched the prior baseline. A separate Edge tab observed both isolated Dogfood
terminals without typing. Its sanitized report recorded two connections:
grant/socket/initial-state 539/353/77 ms (970 ms total) and 54/142/76 ms
(272 ms total). Terminal apply maxima were 25 and 20 ms. The independent
follow-up sample still showed 1,049 ms geometry after 21 ms state application,
so the delayed sizing remains observable but no longer blocks state application.
Payloads/workloads differ from the earlier sample; these are live observations,
not a controlled before/after benchmark or a native-device acceptance claim.

The first full-suite run found one workspace test assuming an unfocused document
could publish an operator resize. The fixture now explicitly models foreground
Work here; additional deferred-fit cases prove losing focus or entering the
mobile composer prevents publication. The focused workspace/controller checks
pass 45 tests and TypeScript passes. Queen's read-only automation status was
enabled, completed/no_action with no waiting_reason; this verifies that run was
not stuck in delivery, not that all its orchestration judgments were correct.
The subsequent complete frontend run passes all 1,224 tests across 134 files.

The controller now accepts an applied canonical snapshot before its post-restore
stable-frame fit finishes. Subsequent output can be parsed instead of waiting
behind geometry work. Attach and snapshot paths share one pending follow-up fit;
completion rechecks ownership, attachment, visibility, focus and composer hold
before publishing dimensions. Stable-frame and anti-oscillation checks remain.
The paired diagnostic still includes asynchronous fit latency (including failed
sizing after successful state application), with one continuation per pending
fit and no retained payload. This is not a claim that layout itself became fast.

136 terminal controller/connection/renderer and diagnostics tests pass, including
output while 20 snapshots share a pending fit, ownership loss during sizing,
failed-fit recovery, passive views and existing geometry guards. TypeScript
passes. Two prior tests now await sizing independently while retaining their
exact dimension assertions. Real-device cutover and live latency acceptance
remain open until observed; no worker engine or protocol change is introduced.

### Snapshot application phase evidence (September 6 UTC)

Build `316a9aef` is deployed and healthy with no degradation; actual engine
`38453a84c3fa` and all 15 running session IDs remained unchanged. A separate live
Edge tab visited Swarm Dogfood and opened diagnostics without terminal input or
decision submission. Three completed applications were captured. The slowest was
1,096 ms for 83,666 bytes: 34 ms state application and 1,062 ms geometry/focus.
Fresh server diagnostics reported no pressure in measured layers. This points
the next investigation at the controller's awaited post-snapshot fit (which can
wait for fonts and stable animation frames), not at snapshot parsing alone.
It is one live observation, not a general causal or performance conclusion.
The isolated fifteen-view pass retained 15 renderers with one attached and 14
inactive, captured 16 applications and kept the same 26.5 ms maximum.

Local diagnostics and the sanitized report now retain a paired breakdown of the
slowest completed terminal snapshot: state application (reset, resize and parser
completion), subsequent geometry/focus, and payload byte count. No content or
session identifiers are retained. At most 200 samples survive for one hour in
the current page; reads/records prune without a timer. Only applications starting
and finishing in a visible attached view are sampled. Failures and interrupted
restores are excluded rather than treated as fast completion. These phases are
wall time, not CPU or paint, and do not include transport/backlog waiting.

The isolated Edge terminal-pool fixture produced one real callback sample:
1,547 bytes, 25.7 ms state application and 0.8 ms geometry/focus (26.5 ms total).
This proves wiring on synthetic content, not the cause of the live two-second
restore. 67 focused controller/evidence/diagnostics/report tests and TypeScript
pass. The live cause, representative performance comparison and overhead
acceptance remain open; no buffering, resize or worker behavior changed.

### Latest development verification and decision reading order (September 6 UTC)

The follow-up `f7b6c1a0` deployment is healthy with an empty degraded list,
the same actual engine build and the exact same 15 session IDs. The complete
frontend suite passed 1,215 tests across 133 files. An isolated Edge interaction
selected Something else, entered a custom answer and confirmed Send answers
became enabled, without submitting any real decision. Main pushes bypassed four
expected required checks; these local results are not a claim of green CI.

Development build `bc3f5057` is healthy with no reported degradation. Its reload
completed successfully; the actual engine build remained `38453a84c3fa` and all
15 running session IDs exactly matched the pre-deployment baseline. This batch
did preserve workers, unlike the preceding documented engine convergence.

Needs You now puts supporting argument/evidence and its task link after the
answers. The risk preview and exact grant command remain before the actions.
Long requester recommendations use the existing expandable preview rather than
an unbounded paragraph; source text and custom drafts are retained. The custom
answer toggle exposes expansion/control semantics and uses a neutral placeholder.
105 inbox/interview/App tests and TypeScript pass. Edge's isolated 390-by-844
fixture visibly wraps the question, recommendation, risk and options without
horizontal overflow; this is not native Android/iOS acceptance.

Contract inspection confirms `DecisionRequest` still has only the requester's
`suggested_action`, not independently attributed worker and Queen opinions.
Separate Queen/worker recommendations remain unfinished and must not be faked by
relabelling one value twice. Direct-terminal decision reconciliation and the
broader performance/recovery gates also remain open.

### UX direction approved by the operator (September 6 UTC)

The operator approved the phone-readable Needs You/Queues direction: concise
decision cards with worker and Queen recommendations, immediate answer buttons,
expandable evidence, owner-first queue groups, deliberate scheduled holds, and
healthy activity minimized. Preserve warm cream, sage and honey, distinctive
worker bees, restrained visual personality, comfortable mobile targets and the
runtime area for system notices. This approves implementation of the composition,
not completion of UX-01/P6 or changes to orchestration authority.

Explicit acceptance addition: every pending Needs You decision must offer an
answer in the operator's own words when quick options do not fit. Do not require
choosing an inaccurate quick option or treating custom instructions as dismissal.
Preserve the current free-text answer path through the redesign, including
interviews, mobile input, failed submission and actionable command requests.
Exact command authorization still requires the existing explicit grant; a custom
answer must not silently grant it. Real-device and rendered acceptance remain open.

Custom answers now stay open until the request succeeds. Answer and resolution
failures propagate to the card, preserve its draft, and allow retry. A per-request
in-flight guard prevents competing submissions before global busy state arrives;
missing answer transport reports an error rather than appearing to accept text.
The 45 inbox/interview tests pass, including retained failed replies, pending
controls, missing transport and custom command responses that never invoke the
grant action. The App suite passes 57 tests and TypeScript checking passes.
An existing action-attribution test now waits for its first request to settle
before attempting another action. These checks do not prove real-phone delivery
or direct-terminal reconciliation.

### Experimental admission UI implementation (September 6 UTC)

Settings now requires explicit per-change experimental consent and engine
availability for new bindings. Unknown/absent capability cannot silently choose
another provider. Failed creates and edits retain the entered values and error.
Consent resets after successful creation/save or discard. Temporary alternate
providers have a confirmation with the same gate, truthful capability limitations,
and pending-request duplicate/dismiss protection. A successful create followed
by roster-refresh failure reports that the worker exists rather than inviting a
duplicate retry. No existing worker is changed by opening the confirmation.

The focused Settings, temporary confirmation, roster and App suites pass all
96 tests; TypeScript checking passes. Separate Edge fixture interaction verified
consent and retained creation failure. Mobile visual review, App-level temporary
request integration, full-tree backend validation and deployment remain pending.
No experimental provider was promoted or launched by these fixture checks.

Follow-up validation: exact committed source `daf842e6` was exported to an isolated
Linux directory rather than overlaid onto an older checkout. API/engine `cargo
check` and all eight targeted experimental API/domain/IPC tests pass there.
The App-level temporary-handoff regression now passes: no request before consent,
explicit acknowledgement in the request, visible failure with retained consent,
successful retry, and no PATCH to the parent. The fixture explicitly opens Workers
before looking for its roster menu. The 390 by 844 Edge iframe viewport was
measured and inspected in consent, expanded-limitations and failed-create states;
text and buttons fit. This is responsive fixture evidence, not native Android or
iOS acceptance. `web/mobile-harness.html` provides the bounded isolated viewport
without changing a live browser window or reaching a Hive.

The full frontend suite passed 1,211 tests across 133 files with two test workers;
TypeScript passed. Commit `3bfdcc3e` was pushed to main and deployed through the
normal development reload (job PID 2873675 completed successfully). GitHub reported
bypassing four expected required checks; local validation is not CI-green evidence.
Fresh health reports `1.5.0-dev-3bfdcc3ec112-20260906035803-2873698`, no degraded
subsystems. This deployment did NOT preserve the old PTYs: engine PID 2721173
received graceful shutdown at 04:00:41 UTC and was replaced by PID 2876265, build
`38453a84…`. The observed running count rose 1 -> 4 -> 8 -> 15 automatically, with
no manual start requests and no worker runtime errors. The package reconciler's
journal attributes the swap to `swarm-host-reconcile.service` at 04:00:40 UTC,
completing at 04:00:49. Its 04:03:13 pass confirmed the new engine was current.
The existing package path drains, checks activity and records owed returns before
replacement. This was a managed update, not a host crash; do not describe it as
API-only continuity or claim the old PTYs survived.
D365's conversation report is current, not independent proof of the exact resumed
conversation. The new engine reports Claude/Codex available and all three named
experimental executables unavailable. No experimental worker was started.

Queue readability follow-up: dependency identity, owner and state remain visible,
with the narrative behind "Why this dependency" in Queues only. Task cards retain
their existing presentation. The 74 focused queue/task tests and TypeScript pass;
this follow-up is not yet deployed or a completed owner-first redesign.

Queues now includes a compact owner index linked to the actual group and count,
with keyboard-focusable targets. Counts disappear as the matching groups clear;
no task or ownership mutation is involved. The 24 queue tests and TypeScript pass.
Edge inspected the index at 390px and confirmed its dependency-group jump reaches
that group. Normal activity remains collapsed. This navigation improvement does
not yet unify transport-only waits with the task-owned groups or separate all
scheduled holds; those remain in scope.

Recorded future holds now have a separate, collapsed Scheduled group. This
presentation requires Blocked state, recorded blocked ownership and a finite
future `blocked_until`; it never interprets prose or conceals a pending operator
decision/unknown owner. An elapsed timestamp moves the row back into the visible
blocked group on refresh without resuming it. The 83 queue/App tests and TypeScript
pass, including mixed holds/decisions/missing evidence and expiry. Edge verified
the folded fixture and expansion back to the actual task. Transport-only owner
integration and the broader UX finish remain open.

### Task board and queue hold consistency (September 6)

Task cards now receive the existing coordinator hold projection through the app
and display the same specific reason as Queues. The shared projection rejects
stale holds after delivery, reassignment or task-state changes. Without known
evidence, a queued briefing says delivery has not started rather than guessing
that it waits for a quiet moment. No polling, dispatch policy or task transition
was added. All 80 focused task/queue tests and TypeScript checking passed;
Edge rendered the named predecessor in the actual TaskBoard fixture. Build
`cb2d5c4b` then deployed healthy after the soak, preserving engine PID 2721173,
build identity `a5686596…` and all 15 running sessions. A separate authenticated
Edge tab verified that live Tasks now renders the recorded worker-busy holds.
This does not close full QUEUE-01 recovery.
The broader App suite also passed all 57 tests after this change.

At 02:40 UTC a separate Edge tab on the unchanged live build captured one
2,433 ms reconnect: grant 213 ms, socket 135 ms, restoration 2,085 ms. The server
sample then reported no pressure; this is later than the observed test burst.
The renderer backlog sample peaked at 2,001 ms. These identify restoration as
the largest measured stage, not a proven parser, layout, GPU or network cause.
Three route-paint samples had median 37 ms and maximum 66 ms, but the route
instrument starts at the App effect and does not wait for lazy content readiness.
It is not click-to-ready evidence. Automated Event Timing entries had zero
grouped interaction IDs and are not human-input latency or INP acceptance.

### Sustained workload attribution (September 6)

The read-only run `20260906T021841Z-live` observed unchanged build
`17d99596` for 30 minutes. Its first 24 samples span 699 seconds, with 15 running
sessions throughout. API cgroup CPU averages 2.08% of one core (highest sampled
interval 10.41%); the terminal-host cgroup, including all worker descendants,
averages 333.47% (highest interval 736.60%). These are interval-counter deltas,
not browser CPU or terminal-host-only CPU. API memory ranges 69,586,944–155,176,960
bytes; host-group memory ranges 7,114,739,712–9,867,960,320 bytes. This changing
workload does not establish a leak, plateau or before/after improvement.

A separate process-name-only snapshot at 02:27 UTC found seven Vitest workers
with high lifetime CPU percentages, alongside CPU PSI `some avg10=39.80` and
zero sampled memory PSI. It supports workload contention at that moment, not
attribution of every interval to those processes. No worker was interrupted.
The observer completed its 1,800-second run with 60 samples spanning 1,790
seconds between first and last measurements. All 15 original sessions remained
running; API PID 2765152 and host PID 2721173 were unchanged. Full-window CPU
averaged 2.82% of one core for the API (maximum interval 15.79%) and 201.86% for
the host cgroup including workers (maximum interval 771.51%). API memory ranged
69,586,944–202,334,208 bytes; host-group memory 7,114,739,712–10,857,439,232 bytes.
History ranged 533,460,786–536,795,043 bytes, below the 536,870,912-byte cap,
with zero reported dropped bytes. Collection took 0–1 whole seconds per sample;
this coarse measure does not establish instrumentation overhead. The rising API
memory series and changing worker workload require longer comparable evidence
before claiming a plateau. PERF-01/02 and P7 remain open.

`node scripts/dogfood/live-soak-summary.cjs <samples.csv>` now summarizes these
content-free samples (use `-` for stdin). Four tests cover time-weighted CPU,
multi-core values, CRLF, incomplete data, counter resets and invalid intervals.
The report deliberately does not claim acceptance or service continuity;
continuity still requires the observer's final report. No live build change
was made during this observation.

### Attachment recovery verification (September 6)

All 88 attachment, terminal-view and mobile-composer tests passed, including
timeout/retry retention, cancellation, stale response and cross-session guards.
TypeScript checking passed. An incomplete clipboard transfer without `items`
now returns no file instead of throwing; ordinary text input remains available.
This small defensive change does not establish the cause of the reported camera
or gallery failures. Android camera/gallery acceptance was requested from the
operator; iOS and native AskUser rendering remain separate outstanding gates.

### Runtime diagnostics entry (September 6)

The footer Diagnostics shortcut now clears an old Settings search, and the
Maintenance group puts the existing diagnostic card before migration and backup.
This avoids an unrelated search hiding diagnostics and removes the initial
scroll past maintenance tools without duplicating the diagnostics owner. The
67 App/navigation tests and TypeScript checking passed, including card order,
search recovery and the original single-owner resource refresh assertions.

### Post-change live baseline (September 6, 01:57 UTC)

Read-only run `20260906T015710Z-live` on `d6047bda` collected 18 samples over
180 seconds. API PID 2745336 and engine PID 2721173 stayed unchanged, with all
13 original sessions retained. The sample count of running sessions ranged
13–15 (the browser later showed 14), so it is not a constant-workload comparison.
API cgroup memory ranged 77,512,704–173,412,352 bytes; engine cgroup memory,
including provider children, ranged 4,691,361,792–5,558,165,504 bytes. Recorded
history stayed below the configured cap, with no dropped history bytes reported.
CPU deltas averaged 17.35% of one core for API, with a 60.55% maximum sample
interval; the engine cgroup including children averaged 38.57%, maximum 124.63%.
These are not whole-machine or Edge Task Manager percentages. Worker starts and
the short window prevent either a leak conclusion or an optimization comparison.
Content-free samples remain at
`/home/bschleifer/.local/state/swarm-next/soak/20260906T015710Z-live-samples.csv`.

The separate fresh Edge tab's visible diagnostic report captured reconnect
1,248ms (grant 995ms, socket 149ms, restore 104ms), route paint median 56ms /
maximum 78ms across three samples, and event-entry maxima 1,032–1,064ms. It
reported zero grouped interaction IDs, so these automated event entries are not
unique human interaction measurements or page INP. The contemporaneous server
sample reported no pressure. Browser/OS foreground conditions were not proven;
neither causal attribution nor aged-session acceptance follows from this sample.

Live navigation also exposed a UX follow-up: the runtime's Open diagnostics
button opens the Maintenance group, with Migration and Backup preceding the
diagnostic card. Targeted diagnostic navigation remains to be improved without
duplicating that card or removing the existing maintenance tools.

### Evidence-read event request churn (September 6)

Successful `swarm_read_task_history` and `swarm_list_coordination_attention`
calls woke all waiting control-room requests despite committing no event. The
event endpoint then reread SQLite and returned an empty page to each browser.
These reads now share the existing notification exemption for read-only tools.
A registered-notification regression verifies both reads remain quiet and a
subsequent task creation still wakes the listener. This addresses unnecessary
request churn, not proof of a browser rerender, measured CPU savings, or closure
of PERF-01/02. Durable event publication and long-poll bounds are unchanged.
All 65 agent API tests passed in the isolated Linux checkout after this change.

### Truthful Queen finish response (September 6)

The finish transaction already normalizes `needs_operator` to `no_action` when
Queen has no pending decision. The MCP adapter nevertheless echoed the requested
outcome. It now returns the transaction's actual committed outcome, without a
second read or policy change. All 35 conductor tests and the MCP end-to-end
normalization regression passed in the isolated Linux checkout.

A bounded read of Queen's live terminal also clarified her current claim: the
four original drafts are held for the operator's interview, while other backlog
items have external gates. That is a claim to verify, not authority to override
operator deferrals or evidence those gates have structured records. The demo
completed normally; this does not settle the broader backlog disagreement.

### Shared ownership of blocked reassessment (September 6)

After the automated paste was submitted, Queen entered a new Running review.
The board still had four Draft tasks and 32 Blocked tasks. Four blocks had an
unresolved prerequisite; the other 28 had notes but no structured prerequisite,
pending-decision owner or hold deadline. The shared owner projection previously
left those 28 attributed to Blocked even while attention listed them for Queen's
reassessment. It now attributes the verification move to Queen without changing
Blocked state or interpreting/discarding notes. Pending decisions, unresolved
prerequisites and future explicit holds retain their current gates.

All 103 domain tests passed. The broad persistence run passed 568 tests and
identified two old owner expectations (unstructured block after decision
withdrawal and the lifecycle owner matrix). Those assertions were updated to
Queen, retaining Blocked state. The decision-withdrawal test and two agent
attention tests passed afterward; the agent assertion confirms this work appears
in Queen's own queue snapshot as well as the reassessment candidates.
The corrected owner-matrix test and all 14 prerequisite regressions also passed.
The full isolated Linux persistence suite was subsequently rerun after these
corrections: all 570 tests passed in 316 seconds. This checkout contains the
scoped maturity overlays; it is not a claim of full current-main CI acceptance.

Live development deployment `2084d7e9` was verified in the separate Edge Queues
tab: 31 Queen-owned items (four Drafts and 27 blocked reassessments), with four
remaining prerequisite-gated blocks. Task changes during deployment explain the
one-item difference from the earlier snapshot. This verifies shared presentation,
not that Queen has finished reassessing or unblocking those tasks.

The automatic engine reconciler deferred twice while one of 13 sessions was
mid-turn, then installed the desired engine at 01:18:19 UTC on September 6.
Running host identity, not just the API's packaged identity, matched build
`a568659677625c816fe3b6899ed8ef47e10c832500e79518b9bd6295a66f6b6b`.
Sequential recovery was observed progressing through four, five and twelve
workers before all 13 returned, including Queen and both demo workers. The host
then reported 13 running sessions, no unreadable sessions and no drain; API health
was ok without degraded subsystems. No manual worker start was needed. This is
one successful live idle engine update and automatic return, not proof of each
provider conversation's contents, crash recovery, or a complete overnight soak.

### Live Queen follow-through probe (September 6)

Created isolated Draft `01a07450-52ec-7521-8461-21970164aa3c` in the existing
workflow-fixture repository: document the tested heartbeat contract, run its
Node tests and commit documentation only. Triage, assignment to Swarm Dogfood
and normal settlement are deliberately left to Queen. No real project task was
changed, and no terminal input was injected to expedite the probe.

The separate authenticated Edge tab showed the demo under Waiting on Queen,
alongside 27 blocked reassessments and four other Drafts. Screenshot review still
finds the Queue presentation too dense: collapsed blocker notes expose up to
240 characters per row. Compact default presentation with full expandable
evidence remains UX work, not completed acceptance.

Queen run `01a0744e-ff42-7130-a7fd-d5953fd0d044` initially waited on pacing.
Its displayed deadline moved after a delivery, but this observation did not
establish starvation: the same run was delivered at 01:29:34 UTC and became
Running with one attempt and no waiting reason. The demo still had only its
creation event at that check. Keep the probe open and verify actual task
triage, worker execution and settlement; delivered automation is not completion.

The probe subsequently progressed Draft -> Ready -> assigned -> Active -> Review
-> Completed without manual terminal input or task transitions by this tester.
Queen routed it to the requested demo worker; the documentation-only settlement
record explicitly says every recorded commit touched documentation only.

### Compact Queue evidence previews (September 6)

Blocker notes and returned-review questions longer than 96 Unicode code points
now use a collapsed preview instead of up to 240 characters. Short statements
remain visible; expansion retains the exact full statement. The shared helper
changes presentation only, not blocker interpretation or task routing. All 23
Queue view tests and TypeScript checking passed. The isolated Edge harness
verified expansion exposes the full note and the 390px iframe layout wraps it
without clipping. This is responsive layout evidence, not actual phone acceptance
or approval of the broader visual redesign.

### Automation paste stranded across API replacement (September 6)

A read-only canonical snapshot found Queen's current prompt contained a collapsed
117-line paste, not an empty/grey suggestion. The operator confirmed it was
automated and submitted it manually. Logs show an API replacement at 23:30 UTC
followed by recovery of five interrupted task-message deliveries; this supports
shutdown interruption but does not identify the exact paste's delivery claim.
No test input was sent to Queen by this investigation.

The API previously detached six periodic tasks and joined none during graceful
shutdown. They now have a bounded owner, stop admission on shutdown, let the
current pass complete, and share a 45-second final join budget with explicit
abort/join on expiry. Existing uncertain recovery remains unchanged. Two Linux
tests pass for in-flight completion, no next pass, idle-timer shutdown and stuck
pass cancellation. A real demo paste during replacement is still needed before
claiming this incident's end-to-end recovery fixed.
All 38 coordination-delivery regression tests passed. Binary lint passed with
the existing unrelated `items_after_statements` warning excluded; unrestricted
strict lint stopped at `lib.rs`'s `RETURN_ATTEMPTS_PER_PASS` declaration.

Build 829e0759 deployed healthy with engine PID 2640474/build identity unchanged.
A follow-up Linux regression runs the production worker supervisor and delivery
path against a real host-owned shell PTY. It synchronizes on the echoed briefing
before Enter, requests owner shutdown, and verifies one consumed line, durable
Delivered state, and a still-running terminal. All three shutdown tests passed;
the real-PTY case also passed five consecutive repeats (about 1.4 seconds each).
This closes the owner-drain integration gap, not a real Claude paste-chip test,
full systemd/SIGTERM acceptance, forced-kill recovery, or attribution of the exact
117-line paste. No live worker input was used.

The follow-up `graceful_delivery` integration test runs the actual API executable
with a disposable database, isolated host socket and shell workers. After HTTP
readiness it assigns through persistence, waits for the normal 30-second
supervisor to paste, and sends SIGTERM before the shell sees Enter. The API exits
successfully, exactly one line is consumed, delivery is durably Delivered, and
the independent terminal remains running. This passed in 32 seconds on Linux.
The initial fixture incorrectly assigned implementation work to Queen and was
corrected to use a separate ordinary worker; no runtime permission was relaxed.
Actual process-signal acceptance is now demonstrated for this shell path;
systemd packaging, Claude collapsed-paste and forced-kill acceptance are distinct.

### Blocked-row explanations (September 6)

Queues now labels the recorded operator decision, unresolved prerequisite count,
and future scheduled hold directly on the task row. Without those current gates,
a historical note or completed prerequisite says Queen reassessment is needed,
not that the task is ready or that prose proved a dependency. Invalid deadlines
remain unavailable; multiple gates remain visible in the existing detail rows.
No ownership, task lifecycle, timer, or automatic recovery policy changed.

All 26 focused queue tests and TypeScript checks passed. The network-isolated
queues-prerequisites fixture was inspected in a separate Edge tab with DOM and
screenshot: operator, Queen reassessment, unresolved dependency and removed
prerequisite states rendered distinctly. This is desktop fixture evidence, not
real-device acceptance or proof that Queen has reconciled the live backlog.

Previous attention-read correction b02cad98 deployed healthy with unchanged
engine PID 2640474/build identity; all 64 agent API regressions passed in the
isolated Linux test checkout.

### Queen status stays current across runtime and task changes (September 6)

The App event adapter refreshed Queen automation only for workers_changed or a
cursor reset. Runtime replacement, task/decision transitions, session replacement
and presence changes could therefore leave its explanation/counts stale while
the rest of the page advanced. The same read swallowed failures, allowing the
event cursor to advance without the refreshed status. The existing event feed
now invalidates Queen status for every relevant event (notification-only batches
remain independent), and a failed read uses its existing bounded retry without
advancing the event cursor. No new timer or endpoint was added.

TypeScript and all 68 App/live-feed tests passed, including six event kinds,
clearing resolved reasons and recovery after a transient status-read failure.
The full frontend suite also passed: 1,195 tests across 132 files. Build a8875f85
deployed healthy with the same engine identity. A separate Edge tab displayed the
current Queen prompt-hold explanation and four Queen-owned drafts. Live prompt
recovery remains unverified: no real unsent input was altered. A Windows foreground
probe returned no window handle, so it supplies no evidence about Edge occlusion;
the browser presentation-delay attribution remains open.

The attention MCP reader also no longer substitutes empty lists when held-briefing
or unreachable-assignment persistence reads fail. Those failures propagate as
tool errors, like the other snapshot reads, instead of claiming no obligations.
The focused API regression covers a sleeping worker's assigned Ready task and
its disappearance from unreachable assignments after session binding; reading
attention never starts the task. This is not proof of Queen triage or a live
database-failure drill.

### Current Queen prompt hold and default-browser sample (September 5)

Build 79364c74 deployed healthy with engine PID 2640474 unchanged and 13 workers
running. Queen follow-through is not yet demonstrated: her current queued run
01a073e4-93fd-7761-b34d-4cfa73758700 is repeatedly held by PromptHoldsUnsentText,
while the status API returned waiting_reason=null. No prompt was cleared or sent.
The status read now exposes only an unresolved prompt observation matching the
current run and live session, after existing engagement/takeover/pacing checks.
It says last delivery check, not continuous observation, and copies no prompt
content. Clearing the refusal or replacing the session removes the explanation.
All 35 Queen-conductor tests and 21 Queue tests passed. All-target persistence
lint found an unrelated 101-line Jira test; this is not a full strict-lint pass.
Production-library strict lint passed separately. Build c84a1ed3 deployed healthy
with the same engine PID 2640474. The status API and a reloaded separate Edge tab
both showed the last-delivery unsent-text explanation, with Needs You still zero
and 13 workers running. The pre-reload tab retained its earlier status despite
showing the new runtime version; accepting its Reload banner and reopening Queues
made the new explanation visible. No terminal contents were changed. Queen's
actual triage follow-through remains blocked by the observed prompt hold, not
proven by the new response fields or deployment.

Read-only 180-second sample 20260905T234259Z-live completed with 18 observations
and all 13 original sessions, unchanged API PID 2663910 and engine PID 2640474.
Content-free CSV: /home/bschleifer/.local/state/swarm-next/soak/20260905T234259Z-live-samples.csv.
API memory ranged 45,731,840–146,948,096 bytes; engine cgroup (including workers)
ranged 4,476,952,576–4,577,308,672 bytes. Across the 176 seconds between first and
last samples, API CPU averaged 14.72 percent of one core (maximum interval 74.98);
engine cgroup averaged 24.06 percent (maximum interval 64.85). Neither is browser
CPU or eight-core machine utilization. Short warm-up growth is not a leak proof.

A fresh separate Edge tab kept the normal renderer policy (experiment off).
Six passive first visits and two retained returns produced six completed
connection samples: mean 470 ms, maximum 1,117 ms; grant mean 227/max 878,
socket mean 148/max 176, initial state mean 94/max 104 ms. Seven renderers were
retained, none evicted. Immediate snapshots during the initial rapid sequence
still said connecting; later explicit checks confirmed Admin, demo and Nexus
connected. These samples are not full journey completion or a p95 acceptance.
Event-entry timing reached 9,536 ms with only one 69 ms long-task observation.
A subsequent short navigation sequence's grouped observation measured 3,512 ms:
0.4 ms input delay, 0 ms handler processing, 3,511.6 ms presentation estimate.
The older 9,536 ms entry had expired from the one-minute grouped window, so its
phases are unknown. Browser automation/OS occlusion are not ruled out. Investigate
presentation/scheduling under confirmed foreground conditions before attributing
this to application CPU. No new observer or speculative timing fix was added.

### Queen and Queue ownership discrepancy (September 5)

Operator screenshots showed Queen claiming nothing waited on her while Queues
listed four Draft tasks awaiting triage. The live open-task read confirmed those
four unassigned drafts and 32 Blocked tasks, versus her approximate 24. The
attention tool exposed filtered recovery categories but no full-board ownership
summary or draft-triage list. Reviews returned to Member Services did agree with
the browser's worker-owned group; not every difference was erroneous.

The application now derives a Queen-only summary from the same board projection
the UI uses: complete open counts by state/owner and at most 64 ordered Queen
tasks, with explicit truncation. The MCP response labels ordinary Active work as
included and distinguishes these counts from the waiting-only navigation badge
and recovery subsets. It instructs Queen to account for triage before claiming
no obligations. This is not permission to clear real blockers or proof of Queen's
subsequent behavior. All 32 application tests and 63 agent tests passed in the
isolated Linux workspace, including full counts beyond the 64-item detail cap,
retirement clearing the item, Queen authorization and MCP draft visibility.
Strict application lint passed. Live deployment/Queen follow-through remain open.

### Engine updater survives its own restart (September 5)

The f1dfb661 app deployment completed healthy, but the subsequent automatic
host reconciliation at 23:16:40 UTC was killed by systemd during its own host
restart. The updater had Requires=swarm-terminal-host.service. All 13 workers
were subsequently recovered, but that is not uninterrupted service or proof
that the terminated updater ran its verification/rollback path.

The unit now uses Wants plus the existing After ordering. Package lifecycle
regressions reject stop-propagating host dependencies on this updater; the full
isolated Linux package lifecycle smoke passed. An isolated real user-systemd
experiment reproduced Requires killing/restarting the updater until start-limit;
the first Wants check reused that latched host and failed. After stopping and
resetting those temporary units, a fresh independent Wants fixture restarted its
host, reached the post-restart active check and exited successfully. Temporary
test hosts were stopped. No real worker lifecycle action was issued by the test.
Production unit deployment remains pending; full rolling-update acceptance is
still broader than this correction.

Deployment a86aa983 completed healthy and installed Wants=swarm-terminal-host.service
on the updater. Engine PID 2640474 and build identity eaabda97c781 remained
unchanged during this deployment. No release was cut. The isolated lifecycle
experiment above proves stop-propagation behavior; a full real engine replacement
with post-restart failure/rollback is still a separate acceptance gate.

### Recorded hold visibility in Queues (September 5)

The shared task projection now exposes the existing explicit blocked-until
deadline, only while Blocked. Queues shows the recorded local date/time for a
future hold, or Queen reassessment when it has elapsed. Missing notes/dates are
not inferred; invalid dates remain unavailable. No scheduling, authority or
automatic state transition changes. TypeScript, 21 Queue tests, the API compile
check and all 569 persistence tests passed in the isolated Linux workspace.
No real task currently carries the explicit marker in its current block note;
live visual acceptance will use an isolated fixture and is still pending.

Live f1dfb661 acceptance: a separate Edge tab showed the isolated fixture's
Scheduled hold until 9/9/2026, 8:00:00 PM, matching API Unix deadline 1788998400
(September 10 00:00 UTC). This is DOM evidence; the captured screenshot showed
the upper Queue groups, not the offscreen fixture. The normal state API then
retired task 01a073da-9816-7cc2-845f-d072d42ff58d as Abandoned; blocked_until
cleared, the open-task API excluded it, and Edge's Queue count went 46 to 45
with no fixture row remaining. Its history was preserved. This closes future
hold display/removal, not expiry scheduling or full blocker reconciliation.

### Post-build cold-return phase evidence (September 5)

Commit `023aac85` deployed healthy with no degraded subsystems and unchanged
engine PID 2456733. In a fresh separate Edge tab after compilation finished,
the five-renderer experiment traversed six resting workers without terminal
input, then returned through them. The roster showed 13 running workers.
Seven cold attempts produced six completed samples, one hidden/abandoned,
zero pending and zero failed. Sample p95/max was 2,283 ms (not enough samples
for adoption). Its paired phases were 1,980 ms setup and 304 ms connection
through state application; setup was 17 ms opening, 2 ms font readiness and
1,961 ms layout/initial sizing (rounding explains the sum). The earlier first
return was 355 ms. Thus a single quick return would have hidden the problem.
Investigate the layout/frame-wait path next; these timings do not establish
CPU attribution, confirmed paint, a leak, an aged-session baseline or acceptable
performance. The experiment was stopped and the tab left in Settings. No worker
commands, decisions or lifecycle changes were issued during this measurement.

Follow-up sizing evidence counts measured animation frames and the longest
interval from font readiness/previous frame, paired with that same slowest
return. This distinguishes many sizing attempts from delayed frame callbacks;
intervals include scheduling and preceding measurement work, not CPU time.
It adds only numeric counters to the existing bounded restore sample, with no
new timer, retained frame list, geometry change or background observer. The
new test tab reported visible, but historical visibility during the prior slow
sample is not proven. TypeScript checking and 83 focused restore/controller/
surface/settings tests passed. Deployment and measured frame-count acceptance
remain pending; the underlying performance issue is not fixed by instrumentation.

Frame-count acceptance: `1b6b98e8` deployed healthy with unchanged engine build
identity. After compilation, a fresh separate Edge tab traversed Contract,
Nexus, Public Website, Real Truth, Scout and Admin, then repeated returns without
terminal input. Visibility read "visible" after every selection (not continuous
visibility or proof the OS window was unoccluded). Seven attempts all completed;
zero interrupted/failed/pending. p95/max was 2,267 ms, paired setup 1,984 ms and
connection/application 282 ms. Setup: 19 ms opening, 3 ms fonts, 1,963 ms layout;
only TWO sizing frames, longest interval 998 ms. This rules out repeated sizing
attempts for that slowest sample, not browser scheduling or measurement cost.
The five-renderer experiment was stopped. Investigate whether passive canonical
snapshot connection must be gated on the pre-connection sizing loop; preserve
explicit geometry ownership and initial-size correctness. No optimization or
500 ms acceptance is claimed from these seven samples.

Initial attachment optimization now measures current usable terminal dimensions
after font readiness without requiring two animation frames. The measurement is
non-mutating; unavailable metrics retain bounded stable-frame recovery, and
ordinary resize/ownership guards are unchanged. Controller tests prove the
measured size precedes connection without waiting on ordinary refit; surface
tests prove immediate readiness with a never-fired frame callback and fallback
recovery without local grid mutation. TypeScript and 82 focused tests passed.
The complete frontend suite also passed: 1,187 tests across 132 files.
Live speed, geometry and real-device acceptance remain pending; the fast path
does not claim that the full performance or terminal maturity scope is complete.

Live `f921af28` acceptance: healthy API and unchanged engine build identity.
Fresh Edge five-renderer experiment, 13 running workers, six cold returns all
completed with zero failed/interrupted/pending. Slowest/p95 770 ms: 48 ms setup,
723 ms connection/application; setup was 38 ms opening, 9 ms fonts, 1 ms layout
(rounded values). Prior sample had 1,963 ms layout. This demonstrates the intended
fast sizing path on the live build, not an overall 500 ms acceptance. The sequence
used Swarm Dogfood instead of Contract plus the same Nexus/Public Website/Real
Truth/Scout/Admin views. Automation had a toggle timeout before enabling and a
label mismatch when the demo became With you; state was inspected before retry.
No terminal input was sent. Visibility was checked after successful selections,
not continuously. The experiment was stopped. Remaining work includes larger
matched fresh/aged samples, connection/application latency and real-device
geometry/interaction acceptance; the five-renderer policy stays off by default.

### Accepted evidence retry feedback (September 5)

Live follow-through on `f921af28`: the Contract demo received browser input,
ran `pwd` in the expected isolated workspace and printed HONEYCOMB-INPUT-PASS
with exit 0. It read completed task 01a07375-2e04-7f83-95c3-e02124f47792 and
made one requested duplicate no-deployment claim. Its displayed result reported
the exact corrected "already approved; this task is completed" error and
instructions to read accepted evidence/history instead of rewriting it. This
was an explicitly bounded single-call test, not proof that all agents naturally
stop retrying. SSH confirmed a clean demo tree, healthy serving revision and
unchanged engine PID 2456733. The tab was left in Settings. Basic post-optimization
terminal input and the live MCP error message are verified; real-device geometry
and sustained performance remain open.

The isolated blocked-ready recovery task auto-completed after its empty commit
report. A later no-deployment claim was rejected with misleading instructions
to supply concise evidence, causing unnecessary retries. An already-approved
claim now returns a distinct conflict with the current task state and directs
the caller to read accepted evidence/history. Approved evidence remains immutable.
Validation: 20 completion-evidence tests, 16 task-outcome tests, and the focused
API error-mapping test passed in the isolated Linux workspace. Commit `936352af`
deployed successfully: health reports that revision with no degraded subsystems,
engine PID 2456733 unchanged, and Edge shows 13 running workers. Live acceptance
of the new MCP error remains pending. This does not prove backlog recovery.

The worker roster also renders "awaiting release" rather than the internal
`awaiting_release` identifier. TypeScript checking and all 11 worker-work tests
passed; live Edge inspection confirms the human-readable labels on Platform and
Real Truth and the matching deployed runtime revision.

### Blocked reassessment discovery (September 5)

Live Edge inspection after reassessment started found five task-linked operator
decisions. One recommends "Read platform's push-subscriber count first, then
decide" while the count remains unread. This is evidence of an unfinished
worker-first investigation path, not proof that every escalation is necessary.
Verify existing read authority and route safe evidence gathering before asking
the operator to make the actual scope choice. No real decision was answered or
withdrawn during inspection. The generic blocked backlog remains unproven.

Follow-up guidance now distinguishes missing facts from missing authority in
Queen's standing brief, each automation submission (including existing sessions),
and the decision tool description. It directs already-authorized investigation
through the owning worker before escalation and preserves permission/access
refusals. Buttons must not combine operator execution with worker authorization.
This is prompt-contract work, not a deterministic classifier or proof of model
compliance. Isolated live investigation-to-decision acceptance remains open;
existing real requests are not silently withdrawn or answered by this change.
The focused standing-brief and per-run-brief tests both passed in the isolated
Linux workspace. Commit `87cad0d6` deployed with healthy API, no degraded
subsystems, engine PID 2456733 unchanged and 13 workers running. Live behavioral
acceptance remains pending; guidance delivery is not proof of compliance.

Recent task activity supplies one concrete reassessment outcome: sequence 7140
records Queen filing the previously unfiled memberStatus operator question as
a task-linked decision. It does not show that the remaining backlog has cleared.
Sequence 7142 also shows Queen interpreting routine automatic settlement as a
missing mandatory countersignature. The no-deployment tool description still
claimed universal Queen approval despite the approved routine-settlement path.
That description is corrected to distinguish coordinator-approved empty/docs-only
reports from code exemptions requiring Queen judgment. Existing sessions may
cache tool descriptions; this is not yet live provider-consumption evidence.
The focused no-deployment description-contract test passed in isolation. This
additional description correction is not yet deployed.

Queen's completed-prerequisite read missed blocks with no explicit edge. The
existing coordination-attention response now also supplies `blocked_reassessment`:
up to 64 current local Blocked tasks without a pending linked decision, unresolved
explicit prerequisite, or future recorded deadline. Notes are 240-character
excerpts with explicit truncation. Absence of a typed hold means investigate,
not auto-resume. Shared run/standing guidance names Blocked to Ready recovery,
verified dependency recording, task-linked operator questions and preservation
of genuine holds. It no longer tells Queen that no other channel prompts her or
that a quiet Hive proves she deliberately parked everything.

Fourteen prerequisite/discovery tests passed, including no-write discovery,
unlinked blocks, deadline boundary, pending decision resolution, removed/reopened
prerequisites and overflow. Queen-only MCP access and Unicode excerpt bounds,
standing/run guidance and the real-PTY review delivery regression passed in the
isolated Linux overlay workspace. Live interpretation of the real backlog and
the full owner-oriented Queues composition remain unproven.

The broader agent-contract suite passed 62 tests. Commit `924a7d48` deployed
healthy as `1.5.0-dev-924a7d4886d3-20260905214246-2582089`, engine PID 2456733
unchanged. Queen policy was already Coordinate at Hive/away and Local execution
at Night Watch; no policy was changed. Review `01a07383-1e1b-7ca1-8a2b-fa21dfda2c7d`
kept its 21:47:24 pacing boundary, then waited for a fresh resting prompt while
Queen handled another coordination turn. Separate Edge inspection showed that
turn creating a linked D365 user-provisioning decision after a classifier denial;
Needs You displayed the task, summary and recommendation. No answer was supplied.
After leaving Queen's terminal, the review became Running with one confirmed
delivery at 21:49:48 UTC. That proves the updated run can reach her; it does not
yet prove she consumed the new reassessment list or recovered the real backlog.

### Cleared blocks must produce a new briefing (September 5)

An isolated regression reproduced Blocked to Ready retaining a Delivered
dispatch even after its explicit prerequisite completed. The transition now
re-arms the assignment with a new generation. The regression passes and proves
unresolved prerequisites still refuse recovery, another Active task is preserved,
the recovered briefing waits for that task, an old receipt cannot settle the new
briefing, and Ready to Active does not duplicate it. All 26 dispatch tests passed.
The full isolated persistence suite subsequently passed 568 tests (two test
threads, 235.65 seconds). Commit `2f37f050` deployed healthy as
`1.5.0-dev-2f37f050d21e-20260905212155-2571126`, with engine PID 2456733 unchanged.

Live fixture `01a07375-2e04-7f83-95c3-e02124f47792` ran on Swarm Dogfood Contract:
initial briefing -> worker Active -> intentional Blocked checkpoint -> operator
Mark ready through the separate Edge tab -> new queued briefing -> paced delivery
-> worker Active -> Review -> automatic Completed at 21:30:21 UTC. The worker
re-read checkpoint sequence 7126 and the UI transition 7127 before running
`printf HONEYCOMB-RECOVERY-PASS`; its output and exit 0 were visible in the live
terminal screenshot. The fixture repository remained clean by independent SSH
git status; worker session `01a072d4-9a60-7051-802d-50670541636e` was unchanged.
No real blocked task or operator decision was changed. The existing five-minute
returned-brief pacing remains a latency source, not a missing-delivery defect.
Broader blocker classification and autonomous reassessment remain open.

The fixture also exposed a follow-up error-quality defect: its empty commit
report auto-settled the no-change task, then two attempted no-deployment claims
were refused with "completed work requires concise verification evidence". The
worker initially interpreted that as a note-length problem before reading the
history and recognizing the task had already closed. Make this completed-state
refusal explicit without widening evidence-write authority; not fixed here.

Separate rendered Settings verification confirmed Developer Dogfood spans the
desktop content width and does not include the Your Hive identity card. This
closes that desktop composition check, not narrow-layout or real-device approval.

### Queen review delivery fairness (September 5)

At 21:06:27 UTC the same review requested at 20:30:03 remained Queued with
zero attempts; the eligibility deadline had moved again to 21:08:45. Delivery
ordering now gives a queued review first consideration after an acknowledged
delivery during its wait, using persisted run/session evidence. It retains the
shared cooldown and all terminal/operator guards; no task is bulk-unblocked.
The isolated Linux overlay workspace passed 34 conductor tests, 38 delivery
tests, and the real-PTY regression that now queues a second outcome, proves
both remain held during cooldown, then proves the review lands first while
the second outcome stays Queued.

Commit `75f03f46` deployed successfully through the development reload. Health
reported `1.5.0-dev-75f03f4672ca-20260905210838-2563328`, no degraded subsystems,
and unchanged engine PID 2456733/build identity. All ten pre-deployment running
worker session IDs were preserved. The previously starved exact run became
Running with one attempt and confirmed delivery at 21:09:52 UTC. The separate
Edge tab showed Queen Buzzing and the deployed Queues page. At 21:13:01 the run
was still Running, 31 tasks remained Blocked, and no open task's updated_at was
newer than delivery. Delivery recovery is proven; task recovery is not. The
rendered queue still groups 31 diverse holds under "Blocked on something else"
with note excerpts rather than a complete actionable owner/dependency model.
Blocked-task reassessment and QUEUE-01 remain open.

### Queen pacing visibility (September 5)

Run `01a07343-913c-7d12-9684-f494bbdf2b56` remained Queued with zero
delivery attempts for more than 16 minutes, reporting RecentDelivery in live
API logs. The shared coordination cooldown is 300 seconds; other channels run
before Queen reviews and acknowledged deliveries renew it. This establishes a
possible starvation path, not yet attribution of each renewal. The status now
names the exact UTC eligibility deadline from the same persistence calculation
as the delivery gate, so a moving deadline and a bad timestamp can be separated.
Three focused tests passed for status expiry, delivery-window expiry and
cooldown persistence. This changes diagnostics, not priority or admission, and
does not claim that the 31 Blocked tasks have resumed.

### Renderer setup breakdown (September 5)

The opt-in cold-return experiment now pairs initial-fit and font-ready milestones
with its existing connection-start boundary. Developer Dogfood shows opening,
font readiness and layout/initial sizing for the same slowest completed return.
Missing or invalid ordering stays unavailable. No sizing policy, frame wait,
font-loading behavior, queue bound, timer or backend storage changes. TypeScript
and all 1,183 frontend tests passed. The initial full run exposed a source guard
counting only argument-free fit calls; it now counts all direct fit calls while
still requiring the single geometry-authorized helper. Live attribution and
performance acceptance remain open; this is instrumentation, not a speed fix.

### Blocked recovery and API-preserved Queen reviews (September 5)

Live inspection found 31 Blocked tasks with recorded notes, but only one with
explicit prerequisite edges; 13 had no assignee. These are measurements, not
permission to infer dependencies or bulk-unblock work. Queen automation was
Uncertain. Source inspection found API startup invalidated even confirmed
Running reviews while the independent Queen session remained open.
Recovery now preserves that confirmed run only for its exact open session;
unconfirmed delivery and missing/ended sessions still become Uncertain. All
33 conductor tests passed in the isolated Linux workspace, including preserved
run identity, duplicate-run refusal, continued autonomy restrictions, exact
session replacement, missing identity and interrupted delivery. The broader
isolated persistence library suite also passed all 566 tests. Commit `06ee7660`
deployed healthy with engine PID 2456733 and all ten worker session IDs unchanged.
Queen subsequently showed a queued review held by normal pacing; this is not
proof of end-to-end blocked recovery or a live Running-to-Running update trial.
Full QUEUE-01/QUEEN-02 remain incomplete:
cleared blockers must return through Ready, actionable queue ordering must be
reconciled by Queen, and genuine operator decisions need concise cause/action
copy with supporting history collapsed, not a prose-only recovery mechanism.

### Blocked work can return to Ready (September 5)

The operator found that Tasks offered only Resume work, which requested Active
and returned 409 when the assigned worker already had current work. The primary
blocked-task action now says Mark ready and requests the existing Blocked-to-Ready
transition. It does not request activation or worker startup; unresolved
prerequisites still prevent the action. The one-active-task guard is unchanged.
TypeScript and all 50 TaskBoard tests passed, including same-worker active work
and a sleeping assignee; the full frontend suite passed 1,181 tests. Commit
`9a2d1570` deployed successfully with healthy API and unchanged engine build
identity. Live isolated-demo acceptance remains open.

### Paired cold-return attribution (September 5, 20:03 UTC)

Commit `a7248caf` adds a connection-start boundary to each bounded cold-return
experiment sample. The UI/report shows renderer setup and connection-through-state
for the same slowest return, not independent maxima or p95 phases. Missing or
invalid boundaries are null; late callbacks after stop/reset are inert. Existing
200-sample/one-hour limits remain, with no timer, backend field or export added.
TypeScript and all 1,178 frontend tests across 132 files passed, including paired
phases, duplicate boundaries, expiry, unavailable phases, lifecycle wiring and UI.

Build `1.5.0-dev-a7248cafdc46-20260905200300-2513916` deployed with healthy API,
no degraded subsystem and unchanged engine PID 2456733. In a separate Edge tab,
the experiment revisited idle views without terminal input to evict and return
to the demo. Two completed cold returns had a slowest total of 2334 ms, split
into **1991 ms renderer setup and 343 ms connection through applied state**.
Rendered text and a screenshot confirmed the paired readout. This localizes that
sample before transport start but does not identify font versus layout/frame
readiness, browser scheduling or automation effects as its cause. The experiment
was stopped and the off control verified. No default-retention change or release.
The screenshot also exposed a UX issue for the finish pass: the shared Settings
grid stretches the short Hive identity card beside the entire Dogfood panel,
leaving a large blank column. That layout is not accepted as final polish.

### Bounded output and five-renderer experiment (September 5, 19:49–19:56 UTC)

Serving build `7ba532deb5b3` was tested in a separate Edge tab at 1465×1339 CSS
pixels, DPR 1, with eleven running workers initially. The Contract demo alone
received a content-only request for 200 numbered lines and an end marker, without
tools, file edits or task changes. Screenshots showed streaming output and the
final 200/end-marker after switching to the other demo and returning. This is a
bounded streaming fixture, not a sustained high-throughput transport benchmark.

Across the 39.21-second submit/output/two-switch interval, CDP counters measured
458.88 ms main-thread task time, 162.35 ms script time and 37 layouts. Heap was
31.02→29.95 MB; DOM nodes 957→1116. The five concurrent one-second server samples
(excluding vmstat's since-boot first row) showed 95–98% idle CPU, no swap I/O and
no I/O wait. CDP collection was disabled afterwards. These counters include test
interaction overhead and do not measure Edge Task Manager CPU or screen latency.

The existing five-renderer experiment was then enabled only in that tab. Two
ten-worker cycles visited Platform, Nexus, Public Website, Real Truth, Member
Services, D365, RCG Networks, BFG Watchfaces and both demos, waiting for connected
state after each visit without sending terminal input. One measured cold cycle
took 61.53 seconds wall time including automation: 2.054 seconds task time,
634.64 ms script, 109 layouts, heap 37.87→40.11 MB and nodes 4700→5531. No forced
GC was used; these snapshots do not prove a leak or retained-memory savings.
Counters were disabled before the next attempted cycle.

Platform subsequently showed operator engagement, so it was skipped. A Nexus
connected-state wait timed out but a fresh inspection showed connected; it was
not restarted. Other fleet state changed during that follow-up (eleven to ten
running workers and D365 engagement), so the controlled comparison was stopped
rather than assuming an unchanged workload or waking a worker. API/engine health
remained ok and the engine PID stayed 2456733.

Rendered Dogfood evidence reported **13 completed cold returns, p95/max 3321 ms**,
13 attempted, zero pending/interrupted/failed, five retained renderers and 17
evictions. Cold-return timing covers attachment through applied snapshot, not
confirmed paint or ownership. This small automation-influenced sample does not
meet the 500 ms target and cannot justify adopting five renderers by default.
The experiment was stopped through its UI and the off-state button verified.
The next performance investigation must correlate pre-connection renderer setup
with transport/restore for the same attempt; independent maxima cannot identify
which stage caused that slow cold return. Normal-day/real-phone and aged-session
acceptance remain open.

The local diagnostics report also showed a grouped native interaction of 3048 ms
with 2.2 ms input delay, 0.1 ms processing and 3045.7 ms estimated presentation,
despite no long-task samples and fast terminal apply samples. Treat presentation/
automation effects as unresolved, not as proof of a server or JavaScript CPU
bottleneck. Do not relabel these samples as unique slow actions or full-page INP.

### Worker access to completed task history (September 5)

Live Queues contained a worker report that `swarm_read_task_history` denied its
own completed work. Code inspection confirmed history authorization depended on
the current-session visible task list. Commit `51ef3233` instead uses the existing
stable-worker evidence ownership check: Queen or the task's durable assigned
worker may read its bounded history. Completion and session replacement no longer
remove access. Unrelated work remains denied; reassignment changes ownership.
Mutation and review permissions are unchanged. This also removes the task-list
scan from a single-task history read.

All 31 application tests passed in the isolated Linux validation directory. The
first new fixture correctly hit WorkerAlreadyRunning; after releasing the original
session before binding its replacement, the real lifecycle regression passed.
Development build `1.5.0-dev-51ef32333da1-20260905192819-2495193` deployed with
health ok and no degraded subsystem. All eleven running workers still had matching
launch, selection and restored-conversation records, including corrected D365
conversation `c4d71311-c854-4ac8-b637-542b4b280401`.

The live Contract worker then called the actual MCP tool for completed task
`01a07199-fef1-76e0-b908-4ad528774904`. It still returned NotAuthorized: the adapter
assembles history, evidence, messages and review request, and evidence/messages
retained their old current-session filters. This contradicts end-to-end completion
of the initial fix despite the green activity-service test. The follow-up aligns
both reads with durable worker ownership and adds a complete authenticated MCP
response regression after session replacement, including unrelated-task denial.
All 31 application and 62 MCP adapter tests pass for the follow-up in the isolated
Linux validation directory. Follow-up commit `7ba532de` deployed as
`1.5.0-dev-7ba532deb5b3-20260905194311-2502635`; health is ok with no degraded
subsystem. Engine PID 2456733 and all eleven worker session IDs were unchanged.
Without restarting its conversation, the same Contract worker repeated the exact
task/limit request successfully: six events, approved documentation-only exemption,
no deployments or messages, null review request and truncated=false. The expanded
native tool response was inspected in the separate Edge tab, not just the worker's
summary. This closes the observed completed-history access defect, not the wider
Queen/queue acceptance program. No real worker received test input or decisions.

### Ten retained views: bounded browser baseline (September 5)

On `034f24db`, CDP Performance counters were enabled only for bounded samples
and disabled afterwards. Diagnostics idle: 16.26 seconds, 39.47 ms task time,
8.19 ms script time, one layout, 716 DOM nodes and 24.68→25.27 MB JS heap.
Selected Contract idle: 44.60 seconds, 155.39 ms task time, 36.51 ms script,
three layouts, 1,397→1,395 nodes and 27.17→29.92 MB JS heap.

The separate Edge tab then visited Platform, Nexus, Public Website, Real Truth,
Member Services, D365 Solutions, RCG Networks, BFG Watchfaces and Swarm Dogfood,
returning to Contract without terminal input or task changes. Across the 72.88
second navigation interval: 1.787 seconds task time, 0.771 seconds script time,
102 layouts and 28.77→38.39 MB heap. Subsequent 52.11-second idle: 203.38 ms task
time, 62.06 ms script, six layouts, 2,258→2,261 nodes and 38.39→40.83 MB heap.
Listener counts rose during samples, but these short ungc'd snapshots do not prove
a retained listener leak or a memory plateau. CDP counters are not Edge Task
Manager's CPU percentage or total browser/GPU memory.

Rendered Dogfood UI confirmed 10 retained / 0 attached / 10 inactive / 0 evicted
after entering Settings, with the five-renderer experiment still off. It showed
10 completed connection samples: mean 319 ms, max 415 ms; access mean 59 ms,
socket mean 173 ms, initial state mean 87 ms. These are completed phase attempts,
not a matched p95 distribution. Event-entry latency simultaneously had 233 samples,
mean 4,700 ms and max 18,448 ms; only two long-task samples (max 106 ms) were
recorded. Do not equate those event entries to separate slow actions or attribute
the discrepancy to a specific component without further evidence. Foreground/
presentation and longer aged-session evidence remain necessary before declaring
the original sluggishness fixed or adopting renderer eviction by default.

### Accurate event and interaction diagnostics (September 5)

Commit `034f24db` preserves the historical `interaction` wire field's event-entry
meaning and labels it Event-entry latency in current and saved Dogfood summaries.
Local diagnostics now separately groups positive native interaction IDs across
callbacks, retaining up to 200 IDs observed in the last minute. It reports the
slowest retained entry's input delay, handler processing and estimated presentation
time, with threshold/coverage limitations. No ID, event name, target or input is
reported or persisted; hourly data is not silently reinterpreted. No new timer,
backend schema or worker lifecycle change is introduced.

TypeScript and all 1,175 tests across 132 files passed. New tests cover grouping,
preserving the worst entry's paired phases, malformed/unknown IDs, bounded volume,
expiry, clock reversal, quantized durations, numeric-only reporting and capture
owner restart. Live development deployment and phase-report acceptance are pending.
This improves causal evidence; it is not a claim that browser sluggishness is fixed.

Live deployment passed on `1.5.0-dev-034f24db876d-20260905191154-2487493`, with
health ok, no degraded subsystem, engine PID 2456733 unchanged and eleven awake
workers in Edge. The dedicated tab was reloaded; the expanded diagnostic layout
and displayed report were inspected. At 19:14:20 UTC, thirteen event entries did
not establish a valid grouped interaction. After keyboard activation of Preview
report, the 19:14:45 UTC report had one grouped interaction: 264 ms duration,
0.30 ms input delay, 0.20 ms processing, 263.50 ms estimated presentation.
These are observed phases, not attribution to Swarm, Edge, an extension or the
server. No native timing entries or identifiers were exported. Positive native
grouping is live-verified; broader aged-session and real-device acceptance remain.

### Idle baseline and Event Timing interpretation (September 5, 19:02 UTC)

On build `906a4d42`, eleven workers were loaded. Five one-second vmstat intervals
showed 92–99% CPU idle, no swap-in/out and no IO wait. The nearby resource sample
reported 15.81% memory used, normal pressure and 8 logical CPUs. This is a short
mostly-idle baseline, not burst or aged-session acceptance. Queen's latest review
was completed with `needs_operator`; the conductor rechecks changed fingerprints
or unchanged actionable work after fifteen minutes, without a global pending-
decision veto. A later worker event burst coincided with that next review window;
this does not prove all unrelated task routing is correct.

The displayed diagnostic report at 19:02:36 UTC separates current-page evidence:
terminal grant 170 ms, socket 439 ms, restore 1,427 ms, total reconnect 2,036 ms;
three renderer samples totaled 1,385 ms (maximum 1,381). Route samples were
18/43/51/67 ms. Three Event Timing buckets counted 11/15/12 entries with a maximum
of 1,064 ms each. These are NOT established unique human-interaction counts:
`installBrowserPerformanceCapture` records every event entry as `interaction`
without grouping by `interactionId`. The W3C Event Timing specification explains
that several entries can belong to one interaction, and its example groups by ID
using the maximum duration: https://www.w3.org/TR/2026/WD-event-timing-20260223/

Next diagnostics work must distinguish observed event entries from grouped
interactions, preserve bounded ownership and numeric-only privacy, and keep old
hourly evidence semantically distinguishable. Do not deduplicate by matching
duration, infer that automation caused the latency, or call the delays fixed.
The old persisted route summary still shows 35,535 ms across twenty samples;
do not attribute that cross-reload summary to this current build's four routes.

### Queues opens the operator's decision directly (September 5)

Operator-owned queue task navigation now uses the exact linked pending decision,
opens Needs You and focuses its card. Multiple pending requests use the oldest
as the entry point, leaving the full inbox available. Missing, resolved, withdrawn
or unrelated requests fall back to task evidence, as does non-operator ownership.
No decision mutation or worker start is sent. The 50 App tests and TypeScript
passed. The first full run had one existing blocked-age navigation test time out
(1,169 passed); that test then passed alone. A second full run and live deployment
acceptance are pending. Do not treat this navigation fix as completion of queue
ownership reconciliation or direct-terminal answer capture.

The second full run passed all 1,170 tests across 131 files. Commit `906a4d42`
deployed as `1.5.0-dev-906a4d4216b8-20260905185109-2478114`; health was ok with no
degraded subsystems and engine PID 2456733 unchanged. Edge inspection timed out
and reset the browser tool session, but reconnecting recovered the same dedicated
tab. After its explicit Reload, clicking the D365 operator-owned queue row went
directly to Needs You and focused decision `01a072b0-0fff-7301-bb05-6f9813db60ba`.
Read-only DOM evidence confirmed the exact title, focus ID and offered buttons.
No response was submitted. The browser roster still showed eleven awake workers.

### Conversation observations versus operator actions (September 5)

Commit `fc04d95d` routes transcript-recency-only observations to collapsed runtime
details, with worker links and the original timestamps. These do not prove the
saved conversation is wrong and no longer contribute to Needs You or its count.
Server-marked filesystem faults retain their actionable card; explicit decisions
are unchanged. No worker is woken or conversation switched by observation or
refresh. Deliberately opening the worker keeps the existing wake behavior.
All 1,165 frontend tests across 131 files and TypeScript passed. Mixed fault and
recency tests verify matching attention, retained evidence, and automatic removal
after a successful current-history read. Development deployment and live visual
acceptance were pending at the initial entry. Live acceptance then passed on
`1.5.0-dev-fc04d95d45ad-20260905184201-2473761`: health ok, no degraded subsystems,
same engine PID 2456733 and all eleven session IDs preserved. The separate Edge
tab offered Reload; after loading the frontend, Needs You and its tab both showed
one genuine pending approval, with the two recency observations retained in
expandable runtime details. Expanding them did not wake either sleeping worker.
Desktop screenshot verified layout; the raw ISO timestamps could be more readable
and remain a polish item. No real-device mobile acceptance is claimed.

Live Queues inspection also confirmed that operator-owned rows currently navigate
to task details. The next workflow change should connect genuine pending decisions
directly to Needs You, while retaining task navigation for other owners. Do not
infer a new decision from prose saying a task is blocked on the operator.

### Selected worker continuity through process replacement (September 5)

The UI now retains configured worker identity independently of process session
identity. Live-feed refresh, manual refresh and the maintenance completion path
resolve that worker's current running session instead of falling back to Queen
when its old process disappears. With no active session, the chosen worker stays
named and the view says its place is saved; it contains no terminal input and
does not start a process. Another explicit selection wins over a later return.
Explicit sleep still moves away, and removing the selected worker permits a
normal fallback. Unconfigured live sessions retain session-only selection.

One bounded v2 selection object persists the worker across reload during a gap;
the UI owns a read-only migration of the old session preference through a current
worker binding, without guessing historical ownership. Unchanged snapshots reuse
selection identity instead of causing extra state/storage writes. Storage denial
is nonfatal. Seven model tests and 43 App tests passed, including event-driven
absence/return, manual refresh, deliberate alternate selection and no start/stop
request during passive recovery. Full frontend validation and live demo acceptance
are still pending; no provider conversation or engine lifecycle rule changed.

The full frontend run passed 1,163 tests across 131 files. TypeScript caught a
test-only mock Response type mismatch; after using a real Response, TypeScript
and all 43 App tests passed again. Roster and mobile selection markers now use
worker identity too, rather than comparing absent session IDs; the final focused
run passed all 50 App/model tests. Live acceptance remains pending.

Live acceptance passed on `1.5.0-dev-40666a5679b0-20260905182535-2465038`.
Managed activation completed successfully and preserved engine PID 2456733 and
all eleven session IDs. In the separate Edge tab, Contract demo was selected on
the new frontend. Directly stopping only session
`01a072c4-0d5e-7bd0-9c3a-569800b87295` produced the named saved-place view with no
terminal input through the live event feed, without a manual refresh. Starting
the same demo produced `01a072d4-9a60-7051-802d-50670541636e`; the browser returned
to its connected terminal without another selection click. Provider selection,
confirmed selection and exact Restored outcome all retained conversation
`c4435eee-57b0-4546-b6ef-dc182080e7b5`. The other ten sessions were unchanged.
No release was cut. Main push bypassed four expected checks; the recorded local
test runs are not GitHub CI evidence. Actual Android/iOS and reload-during-gap
browser acceptance remain separate gates; the latter is currently model-tested.

### Reconnect phase attribution (September 5)

Terminal connection capture now separates grant acquisition, WebSocket opening,
and open-to-applied-initial-state using the existing recorder and hourly Dogfood
pipeline. Captures contain only the allowlisted metric and numeric timing;
ownership, retries, snapshots, provider sessions and input are unchanged. Hidden
or suspended intervals discard pending phases and total-connection timing.
Completed phase attempts can outnumber completed connections; the UI explains
that phase means are not additive and initial state overlaps apply latency.
Older pending/stored captures read as zero samples for new fields, never zero
latency. ADR 0063 records ownership and expiry of this additive compatibility.

The final frontend run passed 1,153 tests across 130 files; the project TypeScript
check passed. Isolated Linux checks passed five domain, four persistence and
three API evidence tests, including phase round-trip, regression rejection,
old-capture defaults and existing privacy/authentication/retention constraints.
Deployment and actual demo phase measurements remain pending. This instrumentation
does not by itself close PERF-01/02 or establish a reconnect cause.

Deployed `a64b12650400` through the managed reload; health reports
`1.5.0-dev-a64b12650400-20260905180554-2453877`, no degraded subsystems.
Main push bypassed four expected checks; local/isolated test results above are
not a claim of GitHub CI completion. Initial API activation preserved all eleven
sessions and engine PID 2420200. At 18:08:40Z, a later managed engine reconciliation
replaced it with PID 2456733. This agent did not invoke maintenance or click the
engine-update action; the triggering request/timer has not been attributed.
Return progressed from Queen, through eight workers, to all eleven. Every new
provider launch, selection and Restored outcome matched the pre-update conversation
audit, including D365's corrected c4d71311 conversation and Contract's c4435eee
marker. No manual duplicate starts were issued. The reconcile unit logged TERM
during replacement, followed by a successful subsequent run; diagnostic clarity
of that transition remains to inspect.

The phase-instrumented Edge report at 18:11:57Z (1465 x 1285 CSS pixels) contained
two complete attachment samples: 666 ms (grant 50, socket 551, initial state 65)
and 258 ms (grant 55, socket 148, initial state 56; rounding is independent).
The latter followed explicit selection of Contract demo after its return. These
are sparse visible-tab observations, not p95, mobile acceptance or proof that
earlier 4–8 second reconnects are fixed. Interaction-event samples still exceeded
one second while the latest server sample reported no pressure; attribution of
that separate delay remains open.
Authenticated readback of the private hourly evidence store confirmed the new
build's two samples persisted (grant total 105 ms, socket 699 ms, initial state
121 ms, reconnect 924 ms). This verifies the live capture-to-store path, not just
local report rendering.

The engine return exposed a concrete selection-continuity defect: the test tab
was on Contract before the engine replacement but fell back to Queen as workers
returned. App selection is session-ID based and substitutes the preferred Queen
session when an old session disappears. Preserve selected worker identity across
temporary session absence and bind its replacement when it returns; this remains
unfinished UX/REC work. No real-worker input was sent during these checks.

### Performance measurement integrity (September 5)

The 17:48Z diagnostic-tab report on frontend a8e84036 / API cd7db41f
contained route maxima of 35,535 ms and a retained 63,784 ms, alongside
30–67 ms route samples. Inspection found route measurement allowed animation
frames to span hidden-tab intervals. This proves a contamination path, not that
those specific historical outliers were entirely suspension rather than delay.
Route samples now cancel on visibility loss, do not start hidden, and dispose
their listener on completion/cancellation. Late callbacks cannot publish after
cancellation. Documentation now describes the actual route-effect-to-second-frame
interval, not full click-to-ready or confirmed compositor presentation.

Nineteen route/recorder tests and the project TypeScript check passed locally.
This change is not deployed yet. Old aggregates remain historical evidence and
must not be treated as a clean baseline. No delay threshold was raised or real
slow visible sample suppressed based on duration alone.

That report also contained reconnect samples of 1,288–2,740 ms. Its latest
server sample reported 16.63% memory used, CPU pressure 0.4%, and zero memory/I/O
pressure. A separate five-interval vmstat sample at 17:51Z showed 84–98% machine
CPU idle and no interval swap-in/out or I/O wait. The initial vmstat since-boot
row is excluded. These are short observations, not causal attribution, a
process-level CPU profile, or aged-session acceptance. Only the separate test
tab was reloaded onto cd7db41f; the operator's tab and all workers were untouched.
At 17:52:31–36Z, systemd CPU accounting increased by 37,066,000 ns for the
API service and 1,115,836,000 ns for the terminal-host service group (including
providers). Over approximately five seconds that is 0.7% and 22% of one core,
respectively, not machine-capacity percentages or host-process-only utilization.
Both service PIDs remained unchanged. This sample does not attribute browser lag.

### Explicit conversation choice across restart (September 5)

ADR 0078 adds schema 140's one-row-per-worker explicit choice, scoped to provider,
workspace and conversation. A paired provider selection or explicit default
command records it atomically with the saved default. Existing pins are not
backfilled. Revision-one startup can confirm only a settled exact restoration
matching that choice and current binding; suspended, stale, mismatched, continued
or fresh results do not acquire confirmation. Workspace moves remove the choice.

Focused recovery tests passed, including disk reopen and restart, rejection before
the startup callback, old bindings, manual fences, ordinary unchosen defaults,
workspace mismatch, schema-139 migration and event-failure rollback. Full
persistence and focused API checks are running. This is not deployed or accepted
live yet; the demo must make a paired choice on the new build and retain its
confirmation after restart without a contradictory timestamp warning.

Validation completed: the full persistence run passed 564 tests and failed the
previous-schema fixture because its newest-step list still ended at schema 139.
After adding schema 140 to that list, all three previous-schema migration tests
passed, including a database carrying related rows. Runtime code was unchanged
by that fixture correction. Nine focused API conversation tests passed. Rustfmt
was applied to the changed Rust files; `git diff --check` passed. This is explicit
test evidence, not a claim that the original full-suite run was green.

Live acceptance passed on `1.5.0-dev-cd7db41f6953-20260905173609-2438090`.
Managed deployment completed successfully without replacing engine PID 2420200.
In Contract demo only, native `/resume` selected the original conversation and
then marker `c4435eee-57b0-4546-b6ef-dc182080e7b5`; revisions 2 and 3 were each
persistence-confirmed. Direct retained stop and wake created session
`01a072a8-78d7-7390-b297-5c0d1c1838d9`. Its revision-one provider selection and
confirmed selection both name the marker, with an exact Restored outcome.
The conversation freshness endpoint now reports Current instead of Stale.
All ten other worker session IDs stayed unchanged; all eleven remain running.
This verifies durable explicit-choice confirmation through an actual provider
switch and restart. No intent was backfilled for other workers. No release was
cut; main push bypassed four expected GitHub checks.

### Decision risk and answer readability (September 5)

Live desktop inspection found long risk prose squeezed beside the task into half
the card width, consuming vertical space ahead of the answers. Risk remains
visible and precedes choices, but now uses the full context width, previews three
lines above 300 characters, and expands the unchanged source text on demand.
Narrow layouts stack context labels above their values. Answer choices retain
their original order and text, use start alignment and wrapping, and specify
44px minimum height rather than 36px. No decision or approval semantics changed.

The isolated fictional long-risk fixture was inspected in Edge at desktop and
inside its configured 390px phone frame. Expansion exposed the full text, choosing
an option enabled Send answers, and tests cover folding without losing the draft.
All 68 decision tests and TypeScript check passed; the fixture-data test also
passed. The phone frame is layout evidence, not native Android/iOS/PWA acceptance.
The browser measurement attempt read the outer page, not the child frame, so no
measured mobile touch-target or overflow result is claimed. Deployment remains
pending; ATT-01 and the full Needs You content/reconciliation pass remain open.

Deployment subsequently passed as
`1.5.0-dev-a8e840369caf-20260905171747-2427143`, with healthy API, no degraded
subsystems, unchanged engine PID 2420200, eleven running sessions and no drain.
The full remaining recovery issue is visible independently: Contract demo still
appears in the timestamp-based drift card after a proven chosen-chat restart.
`confirmed_provider_selections` requires revision greater than one, while the new
binding starts at one. Preserve durable explicit-choice provenance across bindings
instead of treating every startup match as proof of intent. No fix for that issue
is included in the layout commit. Main push bypassed four expected checks;
no release was cut.

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

Retention repair deployed as
`1.5.0-dev-cfd31e1b4d81-20260905165830-2413633`: managed reload finished successfully,
health was ok with no degraded subsystems, all eleven session IDs were unchanged,
engine PID remained 2323268, and the restored original helper remained executable.
Main push bypassed four expected checks; no release was cut. Engine overlay
creation now additionally rejects missing, non-executable, relative or directory
helper paths without overwriting an existing overlay. All 26 isolated engine
library tests passed; that engine guard is not yet deployed.

Engine guard deployment and live maintenance then completed on
`1.5.0-dev-0d4929315a38-20260905170406-2418046`. All 32 terminal-host library/binary
tests and rustfmt passed before deployment. Managed maintenance reported eleven
stopped sessions and replaced engine PID 2323268 with 2420200, build
`b0178a9cc149998172b1e8b1b0b95b0a0f3c0c3489bd73edf7435e00d0ff692c`.
The new process executable resolves to its intact release, without `(deleted)`.
All eleven original worker IDs returned automatically. For every returned worker,
the exact recovery target and provider-start conversation match the pre-maintenance
selected ID, and the API reports Restored. In particular, D365 reopened its
corrected `c4d71311-c854-4ac8-b637-542b4b280401`, and Contract demo reopened
`c4435eee-57b0-4546-b6ef-dc182080e7b5` with an actual callback this time.
The first and last new session UUID creation timestamps span 58.471 seconds;
this measures the allocation span, not per-worker interactive readiness.
Health remains ok, no degraded subsystems, eleven sessions and no drain.
No release was cut; main pushes bypassed four expected GitHub checks.
This closes the demonstrated helper-deletion/restart journey, not all of REC-01
or the unproven historical point where D365's earlier default became stale.

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
- [ ] UX-01/P6: finish the approved composition, complete coherent desktop/mobile
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
