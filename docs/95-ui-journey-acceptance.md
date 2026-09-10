# UI journey acceptance — September 10

Scope remains45/93. This matrix distinguishes delivered behavior, isolated proof
and remaining requirements; it is not whole-program completion.

| Journey | Evidence | Boundary |
| --- | --- | --- |
| Ask before deciding | Actual Queen and sleeping ordinary requester replies; explicit final answer; deployed f05f7f21, full CI34442276338 green | See94; direct-terminal answer correlation and older tool lists remain separate |
| Operator queue -> decision | Live RCG Networks queue item opens its existing request and focuses that exact article | No answer, withdrawal or task transition sent |
| Dependency -> upstream task | Live Admin prerequisite opens the Platform upstream task; exact title and ARTICLE focus verified | Navigation does not resume or assign work |
| Task -> history | Actual upstream task history loads its recorded amendments/handoffs | Currently latest30 records, with truncation notice; older-page navigation not yet exposed |
| History close/reopen | Fictional browser journey; out-of-order regression fails before correction and passes after | Cancellation/error/retry verified in tests, not by disrupting live networking |
| Apiary -> management | Live desktop and390px overview; Manage Apiary opens Connections | No invitation, membership or integration changes |
| Mobile runtime -> destination | Live390px System -> Diagnostics reveals the page; inline details stay open | fd35ca3e deployed; CI34443735026 green; not physical-device suspension proof |
| Diagnostic assessment | Capacity separate from pressure assessment; CPU-only fixture and live page verified | b01aef1d deployed; CI34444605660 green; no causal performance claim |
| Task editor on Android | Operator previously confirmed coverage and Close -> Keep editing preserves changes | Retained acceptance; no claim for other native picker/keyboard scenarios |

## History ownership correction — locally verified

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

- Investigate bounded older-history navigation: the current API/UI exposes a
  latest-page limit but no cursor, so full history on demand is not yet met for
  tasks with more than30 events. Preserve bounded reads and persistence ownership.
- Complete the remaining linked-task/prerequisite error, cancellation and
  history journeys against the delivered candidate; use fictional mutations.
- Native attachment picker, iOS, device handoff/suspension and final operator
  dogfood remain explicit. Desktop width emulation is not those gates.
- Queen orchestration, direct-terminal decision reconciliation, safe automatic
  engine admission, chosen-conversation recovery and sustained efficiency remain
  in the original goal and backend handoff. No BFG contact without approval.
