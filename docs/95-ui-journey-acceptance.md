# UI journey acceptance — September 10

Scope remains45/93. This matrix distinguishes delivered behavior, isolated proof
and remaining requirements; it is not whole-program completion.

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
