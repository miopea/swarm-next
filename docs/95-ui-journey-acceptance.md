# UI journey acceptance — September 10

Scope remains45/93. This matrix distinguishes delivered behavior, isolated proof
and remaining requirements; it is not whole-program completion.

## Decision search state — b20e2e22

Delivered runtime1.6.0-dev-b20e2e2294eb-20260910094656-183627 is healthy, no degraded
subsystems. Reload completed successfully. Pre/post-decision-search-workers.json
in /tmp/swarm-clarification-deploy.cvm5VI match all34 records/12 running identities;
enginePID3408834/start unchanged. No release or worker-engine update. CI34462450125
remains unconfirmed. Prior queue-retry CI34459969300 is green.

Search previously called every decision Attention and omitted state. The inbox
already supports focused history navigation; it did not need a second history
implementation. Search now preserves every result, identifies Answered/Withdrawn
as Decision history, and uses the existing clarification predicate to distinguish
Waiting for reply from Needs your answer. Original reason remains searchable.

All132 App/palette/presentation/inbox tests and build pass. New tests cover all
states and history focus with no answer controls or decision send. Edge390px
fictional fixture shows all four labels and fits; selecting answered history
checks Show history and focuses exactly fixture-answered. No real approval was
answered, withdrawn or changed. Fixture server stopped; deployment pending.

## Quick navigation includes Queues — fd0385d2

Delivered runtime1.6.0-dev-fd0385d26bf8-20260910093249-176487 is healthy, no degraded
subsystems. Reload completed successfully. Pre/post-quick-queues-workers.json in
the retained private evidence directory match all34 records/12 running identities;
enginePID3408834/start unchanged. Edge used Reload this tab. No release/engine
update. Current CI completion remains unconfirmed.

Rendered fixture search for Queues returned No matching result despite Queues
being a primary destination. The new App regression failed on the missing option.
The palette now uses the existing queuedTaskCount and opens the existing page.
No extra queue calculation, task change or worker action is added.

All83 App/palette/modal-focus tests and production build pass. In Edge, searching
Queues shows the same4 waiting as navigation; Enter opens Queues. Reopening and
Escape returns focus to Open quick navigation. At320px there is no horizontal
overflow; Close measures44px and the result74.3px. Search, Enter and close remain
available without a mouse. Fixture server stopped; live deployment pending.

## Queue observation recovery — 2d7b1cd7

Delivered runtime1.6.0-dev-2d7b1cd7c2b5-20260910091951-170000 is healthy with no
degraded subsystems; development reload completed successfully. Pre/post-queue-
retry-workers.json in /tmp/swarm-clarification-deploy.cvm5VI match all34 worker
records and12 running session identities. EnginePID3408834/start unchanged.
No release or worker-engine update. Edge used Reload this tab after observing
the newer runtime; fixture server stopped. CI completion is still unconfirmed.

When observations are unavailable, Retry queue details uses the existing bounded
read owner. It neither dispatches Queen nor starts/restarts a worker. Existing
tasks remain visible until fresh evidence arrives; partial observations alone do
not offer an ineffective retry or claim all-clear.

All124 App/Queues/read-owner tests pass. The integration test recovers from a
failed read with exactly one coordinator GET, clears the warning and removes the
retry action. Both populated/empty Queue branches are covered. Existing owner
tests cover in-flight deduplication, deadline cancellation and later recovery.
Production build passes with the existing chunk-size advisory.

Edge390/320px fictional empty-work fixture: retry remains available when the
response still lacks recovery evidence; no false all-clear. The button measured
44px after correcting its initial38.7px height. Page scrollWidth equals viewport
width at both sizes. This is responsive proof, not native OS/offline proof.
Fixture server stopped. Deployment/continuity receipt pending below.

## Support and diagnostic preview — 1e410476

Delivered runtime1.6.0-dev-1e41047644a9-20260910084326-153185 is healthy with no
degraded subsystems; reload job completed successfully. Pre/post-saved-preview-
workers.json in /tmp/swarm-clarification-deploy.cvm5VI match all34 records and12
running session/provider identities. EnginePID3408834/start unchanged. Edge used
Reload this tab; fixture server is stopped. No release or engine update.
CI34456655548 remains in progress at this checkpoint.
Live desktop verification opened an existing saved report and exposed its6,867-
character bundle with the new Preview saved report action. No content was copied,
uploaded or sent. The preview was closed and the tab returned to Your Hive,
releasing diagnostic sampling. This verifies the delivered component against
existing data; the clipboard-refusal path remains explicitly fixture-tested.

At390px, the isolated support fixture recovered two explicitly selected fictional
text files, showed a lost first-upload response, and kept the original report for
explicit retry. Retry settled as Saved to Hive — waiting to send, not confirmed
central delivery. No real Hive or central request was made. This supplements
earlier desktop attachment proof, not native camera/gallery or paired Admin proof.

Saved diagnostics lacked a bundle preview, and a failed clipboard write was
silent. New regression tests failed on those missing behaviors before correction
(after fixing the test's missing clipboard stub). The UI now optionally previews
the exact retained bundle, mounts only one saved preview at a time, and reveals
that same bundle with manual-copy guidance on clipboard refusal. Retry success
clears the failure. It never regenerates old evidence, adds diagnostics, uploads
files or sends to a developer automatically. Existing local report bounds remain.

All45 focused diagnostic/report/support/file/download tests and production build
pass. Edge's fictional clipboard-refusal journey showed the explicit message and
original captured-version text at390px; saved preview width304px, page width390.
At320px, preview width249.33px and page scrollWidth320; hide/show remains usable.
Existing current-report preview states freshness and unavailable measurements
without including fixture task text or worker names. No clipboard was written
during browser verification. Real failed-copy recovery is covered by the test
adapter and component tests, not by changing the operator's browser permissions.

App/API-only deployment requested; confirm matching live bundle and worker
continuity before claiming delivered. Prior d69e9b16 CI34454887235 is green.

## Responsive finish — September 10, d69e9b16

Delivered:1.6.0-dev-d69e9b165493-20260910082408-144014 is healthy, with no degraded
subsystems. App reload service finished successfully. All34 worker records and12
running session/provider identity projections exactly match pre-update evidence;
enginePID3408834 and September9 17:01:49EDT start are unchanged. Receipts are
pre/post-prerequisite-layout-workers.json in /tmp/swarm-clarification-deploy.cvm5VI.
The dedicated Edge tab loaded the new bundle with Reload this tab. Fixture server
stopped. No real prerequisite edit, schedule save, engine update or release.
CI34454887235 remains in progress; check it once at the next package boundary.

The phone-width prerequisite recovery check found a genuine layout defect:
TaskPrerequisiteDialog has header/form/footer, but inherited the four-row
TaskDetailDialog template. At320x740 its close-confirmation footer was only26.7px
high while its content was205.7px high, overlapping the form. A scoped three-row
template now reserves the footer and lets only the form shrink/scroll. The new
stylesheet regression failed before the fix; all118 focused style/task/modal
tests and the production build pass afterward.

Settled Edge screenshot and DOM geometry after the fix: form bottom507.67,
footer top507.67, footer bottom740, confirmation521.33–727; viewport320x740 with
scrollWidth320. Warning and both44px actions are visible without overlap. Keep
editing retains the fictional draft; explicit close returns to Tasks. At1280x800,
form bottom and footer top both678.67, footer bottom775.33. No task mutation
reached a real Hive. This is responsive browser proof, not native keyboard proof.

Additional paired checks: fictional75-event history pages at390px show readable
Older/Latest controls and replace the page with events16–45. Night Watch search
at320px reveals schedule and autonomy together; time-zone/start/end controls are
44px tall within the viewport and the page has no horizontal overflow. A fictional
timezone edit was not saved. Physical Android/iOS and actual presence transitions
remain open. Prior build80c7d1c4 CI34452563704 is now confirmed successful.

| Journey | Evidence | Boundary |
| --- | --- | --- |
| Ask before deciding | Actual Queen and sleeping ordinary requester replies; explicit final answer; deployed f05f7f21, full CI34442276338 green | See94; direct-terminal answer correlation and older tool lists remain separate |
| Operator queue -> decision | Live RCG Networks queue item opens its existing request and focuses that exact article | No answer, withdrawal or task transition sent |
| Dependency -> upstream task | Live Admin prerequisite opens the Platform upstream task; exact title and ARTICLE focus verified | Navigation does not resume or assign work |
| Task -> history | Delivered cb7d4206; fictional75-event traversal and live exclusive-cursor reads pass | One30-event page; Older/Latest navigation; narrow/native paging gate remains |
| History close/reopen | Fictional browser journey; out-of-order regression fails before correction and passes after | Cancellation/error/retry verified in tests, not by disrupting live networking |
| Apiary -> management | Live desktop and390px overview; Manage Apiary opens Connections | No invitation, membership or integration changes |
| Mobile runtime -> destination | Live390px System -> Diagnostics reveals the page; inline details stay open | fd35ca3e deployed; CI34443735026 green; not physical-device suspension proof |
| Diagnostic assessment | Capacity separate from pressure assessment; CPU-only fixture and live page verified | b01aef1d deployed; CI34444605660 green; no causal performance claim |
| Task editor on Android | Operator previously confirmed coverage and Close -> Keep editing preserves changes | Retained acceptance; no claim for other native picker/keyboard scenarios |

## History ownership correction — delivered 444f7a56

The existing TaskCard started an unowned read each time history opened. A test
resolving the second read first and the first read last replaced Current handoff
with Obsolete handoff. Closing the view did not cancel the network request.

The card now uses the existing useVisiblePolling owner with a null interval:
on-demand/visibility reads only, one current read, existing eight-second deadline,
abort on hide/unmount and no late success/error application after cancellation.
The App callback is stable across unrelated renders and passes the signal through
the task API adapter. The30-record limit and task/authorization behavior do not
change. Failed history remains retryable; it does not mutate the task.

All146 focused App/task/API/read-owner tests passed. The new typed test fixture
initially omitted actor metadata; after correction the production build passes,
and the final task/API69-test subset plus TypeScript pass. The full-app harness
also needed a proper typed activity page instead of its generic empty-array
fallback. Its actual browser close/reopen now renders two fictional events.
This harness repair is not a production backend defect.

Delivered as 444f7a56. Production-dev reports
`1.6.0-dev-444f7a565411-20260910065313-92358`, with healthy API and no degraded
subsystems; the live Edge tab displays that revision. Engine PID3408834 and its
September9 17:01:49EDT start are unchanged. All34 worker records and12 running
worker/session identity projections match the immediate pre-update snapshot.
Evidence is retained in pre-history-workers.json/post-history-workers.json under
the existing private deployment evidence directory. No release or engine update.
The fictional harness server was stopped. CI is not yet confirmed complete.
Do not rerun the already-green native clarification/backend suite.

## Remaining finish gates

### Prerequisite editor recovery — 80c7d1c4

A failing-before test proved a never-settled save left Close disabled past its
deadline. Saves now have a component-owned AbortController and eight-second
deadline, with cancellation on unmount and identity fences on late callbacks.
Timeout releases the form even if the request promise does not settle; it never
automatically replays the mutation or calls the update/close callback afterward.
Definitive409 refusal remains distinct from uncertainty. Closing after uncertainty
warns that only the local draft is discarded and the server may have applied the
change; the warning remains even if fields are subsequently cleared.

All88 focused API/task/prerequisite/focus tests passed. Two test-only selector
typing errors were then corrected; the final12 prerequisite tests and production
build pass. The full-app held-response fixture reproduced Saving, timed out to
retained choices, kept them through Keep editing and closed through the explicit
unconfirmed-change action. Desktop screenshot reviewed; no actual task mutation.
This changes browser request ownership only, not prerequisite/domain rules.
Night Watch CI34450368822 is fully green. After the integrated changes, one full
web gate passed all1,525 tests in157 files (32.18s). This includes the changed
request, history and schedule paths; it is not native-device or server-soak proof.

Delivered: runtime1.6.0-dev-80c7d1c4c654-20260910075801-131543 is healthy with
no degraded subsystems. The immediate pre/post-prerequisite-workers.json receipts
in /tmp/swarm-clarification-deploy.cvm5VI match all34 records and12 running
session/provider identities; enginePID3408834/start unchanged. The dedicated
Edge tab used its update action to load the delivered bundle. No live prerequisite
mutation, release or engine update occurred. Fixture server stopped. CI34452563704
is pending; narrow/native recovery layout remains an explicit device gate.

### Night Watch saved-versus-draft clarity — cb915ab1

The form previously treated a definitive invalid-schedule400 rejection as an
uncertain save and recommended retrying unchanged input. It now explains the
region/city time-zone format and preserves edits. Transport uncertainty retains
the distinct not-confirmed/retry wording. A saved daily-window summary remains
separate from unsaved changes; disabled/no schedule does not imply automatic
Night Watch. Overnight windows say next day, same-day windows do not. Confirmed
server values become the saved form values; this is not an authority/policy change.

All48 focused Settings/presence/controller/API tests and production build pass.
The fictional desktop browser shows a timezone edit as unsaved while retaining
the disabled saved-schedule summary. No live schedule change was sent. Deployment
uses app/API only; record the exact schedule and worker comparison before marking
live acceptance complete. Mobile/native schedule controls and actual scheduled/
manual desktop-return behavior remain separate gates.
History-navigation CI34449170913 is now fully green.

Live acceptance: `1.6.0-dev-cb915ab171d6-20260910073144-117366` healthy; Edge
loaded the matching bundle and displays the saved22:00–07:00 next-day window in
America/New_York. Search for night watch shows both presence/schedule and Queen
autonomy with its three distinct ceilings. No setting, decision or schedule was
submitted. pre-schedule-config.json/post-schedule-config.json compare identical;
the corresponding worker snapshots preserve34 records and12 running identities,
with enginePID3408834/start unchanged. Evidence directory remains
/tmp/swarm-clarification-deploy.cvm5VI. No release/engine update; fixture server
stopped. CI34450368822 is pending. Native presence behavior is not closed.

### Older task history — delivered cb7d4206

ADR0095 adds exclusive-sequence paging to retained task history. The browser
keeps one30-event page, offers Older activity/Latest activity, and resets to
latest when reopened. Older-page failure retries the same cursor; returning to
latest aborts and ignores a late older response. No state/permission/schema change.

Seven isolated Linux task-activity API/persistence tests pass, including cursor
validation/authentication, ordering with concurrent new events, unknown tasks,
empty earlier history and existing bounds. Strict library clippy passes.
148 App/task/API/harness tests and the production web build pass. Browser fixture
traverses75 events as46–75,16–45,1–15 without accumulating rows. Oldest page has
only Latest activity; the measured button height is44px. Desktop screenshot
review passed. Narrow/native paging verification remains explicit, not implied
by the button-size measurement. Deploy and record live read/worker continuity.
Previous cancellation package CI34447225894 is now fully green.

Live acceptance: runtime1.6.0-dev-cb7d420626f1-20260910071717-108659 is healthy.
Edge loaded the matching bundle through its update notice. A6 shows all26 retained
events without paging controls; authenticated live reads with limit5/before return
two disjoint, strictly ordered pages. No real task was edited. All34 worker records
and12 running identities match immediate pre/post snapshots; enginePID3408834
and start unchanged. CI34449170913 remains pending. Fixture server stopped;
the dedicated browser tab is on Settings, not an engaged worker terminal.

- Narrow/native history navigation remains a visual gate; deployed desktop and
  live endpoint acceptance above are complete.
- Complete the remaining linked-task/prerequisite error, cancellation and
  history journeys against the delivered candidate; use fictional mutations.
- Native attachment picker, iOS, device handoff/suspension and final operator
  dogfood remain explicit. Desktop width emulation is not those gates.
- Queen orchestration, direct-terminal decision reconciliation, safe automatic
  engine admission, chosen-conversation recovery and sustained efficiency remain
  in the original goal and backend handoff. No BFG contact without approval.
# Display recovery — 83a88edd

Delivered runtime1.6.0-dev-83a88edd9523-20260910090628-163177, healthy/no degraded
subsystems. Reload service completed successfully. Pre/post-display-workers.json
in the retained private evidence directory match all34 records/12 running session
identities. EnginePID3408834/start unchanged. No worker-engine update or release.

- Terminal rendering/import failures no longer assert that an app update caused
  the failure. Root and terminal recovery explain that browser reload does not
  restart workers, without claiming their unobserved current health.
- Terminal boundary records only the existing `react_render` marker. Tests prove
  the diagnostic record excludes the thrown private/path error text.
- All three focused regressions failed against the former copy, then passed;
  production build passed (existing large-chunk advisory remains).
- Edge fictional harness at390px: recovery copy/action fit; Refresh Swarm restores
  only the fictional view. This is not a real provider recovery or mobile OS test.
- Empty-work fixture: Needs You is clear while worker roster remains present.
  Queues correctly withholds all-clear when recovery evidence is unavailable.
  Missing local retry is the next UI-4 action, not accepted as complete.
- App/API deployment requested; continuity/live bundle receipt pending. No release
  or worker-engine update. Previous1e410476 CI34456655548 completed successfully.
