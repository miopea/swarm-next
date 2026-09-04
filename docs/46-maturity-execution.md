# Maturity execution ledger

Approved program: [scope and acceptance](45-daily-driver-maturity-plan.md).
Branch: `codex/daily-driver-maturity`. Starting revision: `36420b3`.
Local commits authorized; no push, deployment, releases, or live worker interruption.

## Cross-phase regression checkpoint — 2026-09-04

### P1i: bounded hourly browser accumulation

- Added one detachable numeric sink to the existing recorder without a second
  performance observer. A build-scoped accumulator owns at most 24 hourly
  captures; retries preserve identity and late acknowledgements cannot erase
  samples collected during an upload. Expired/overflow evidence and invalid or
  backward-clock samples increment loss counters. Identity-generation failure
  is contained rather than interrupting terminal rendering.
- Fourteen focused recorder/accumulator tests passed. The production build passed
  before the final identity-failure guard; the existing chunk warning remains.
- No per-event storage writes, timers or uploads. App-level ownership is still
  required: development detection currently belongs to Settings. Upload lifecycle,
  reload persistence, status/comparison UI and overhead measurements remain open.
  Automatic collection is not yet enabled; no deployment or release.

### P1h: authenticated development evidence API

- Added GET/POST browser-evidence endpoints with operator authentication and the
  same development-mode authority used by runtime status. Upload bodies are
  capped at 4 KiB; successful reads/writes are no-store. Invalid evidence and
  identity/revision conflicts return distinct safe errors without private SQL
  details. Existing domain and persistence rules remain authoritative.
- Added cancellable browser client functions; no collector or extra polling is
  enabled by this slice. Ten API-client tests and the production web build passed.
  Existing terminal bundle-size warning remains.
- Linux-target, all-target/all-feature API and persistence strict Clippy passed,
  including compilation of route authentication, dev gating, body-size and retry
  tests. These Rust route tests were compiled, not executed on Linux.
- Automatic bounded collection, retention/eviction visibility, comparisons and
  measured overhead remain open. No live requests, deployment or release.

### P1g: bounded durable browser evidence persistence

- Schema 126 adds an isolated hourly evidence table. Transactional writes enforce
  cumulative replacement, retry idempotence, identity conflicts, a 24-hour upload
  window, 4,096-capture capacity and 90-day age retention. Reads prune and return
  at most 100 captures; payloads have a database-enforced 4,096-byte ceiling.
- Four focused tests passed: replacement/retry/conflict preservation, invalid
  uploads, schema-125 migration and file reopen, and capacity/read/age limits.
  Strict all-target/all-feature persistence Clippy passed.
- Broader `migration` filter: 14 passed, four legacy-import tests failed with
  `InvalidWorkspace` while creating worker fixtures using home-derived paths on
  Windows, before exercising the new evidence methods. This is not an all-green
  migration suite or Linux execution proof.
- API authentication/development gating, browser uploads, comparison UI, eviction
  visibility and instrumentation-cost validation remain open. No live database
  was opened or migrated. A future install advances schema to 126; an older
  binary alone is not a safe rollback. No push, deployment, release or worker
  interruption occurred.

### P1f: durable browser evidence contract

- ADR 0063 specifies content-free hourly, build-linked aggregates with 90-day
  retention, 4,096 captures, 4,096-byte payloads, and bounded reads. This is the
  agreed engineering design, not an already-running storage feature.
- Domain validation rejects unknown fields, invalid identity/build/hour,
  impossible or unbounded timing aggregates, and cumulative replacements that
  rewrite existing samples. Upload freshness is separate from stored validity
  so retained history does not become invalid after the upload window closes.
- All 81 domain tests and strict all-target/all-feature domain Clippy passed.
  Initial test compilation found UUID v4 generation is not enabled; deterministic
  fixture IDs fixed that without expanding production dependencies.
- Persistence/migration, authenticated API, collection, comparisons, and measured
  overhead remain pending. No schema migration or live data change was performed.
  This contract does not establish measured CPU improvement or finish P1.

### P1e: development-status timeouts are unavailable, not reachable

- Corrected the development-status reader's timeout classification. It preserves
  last known build details during restart but marks timed-out reads unreachable.
  A hide/unmount cancellation is not a new failure. Later successful reads restore
  reachability without a reload or second status owner.
- Passed 27 affected hook, Settings, and Dogfood tests and the production web
  build. The regression covers hide cancellation, timed-out visible return,
  retained build identity, and recovery. Existing terminal chunk warning remains.
- Rechecked live Swarm in a new dedicated Edge tab: still at Welcome back/unlock.
  Asked the operator to unlock that tab; no credentials inspected or working tab
  used. No authenticated live performance measurement was possible this turn.
- No push, deployment, release, or phase-completion claim.

### P3l: Settings status reads follow rendered cards

- Worker-engine/tool-surface, Queen/coordinator, Jira readiness, and email
  readiness reads now run only for the cards that consume them, including search
  results. They use the shared visible-page owner, eight-second deadlines,
  cancellation on hide/section departure, and refresh on visible return.
- Queen event-triggered refreshes join an existing request rather than abandoning
  it for every event. Its paired reads settle under one deadline even when one
  fails first. Tool-surface refresh failures explicitly label retained counts as
  last known; worker-engine failures do not masquerade as a current engine.
- Passed 55 affected Settings/App/API tests, then all 21 Settings tests after
  final paired-timeout coverage. Production web build passed. Updated the broad
  navigation test to await newly visible engine data instead of assuming hidden
  cards were already fetched. Tests cover section/search visibility, deadline,
  hidden return, cleanup, and a failed-plus-stalled Queen read pair.
- Other child Settings workflows, development-status polling, and explicit
  mutations/downloads retain their own lifecycles and remain audit work. No live
  CPU reduction, phase completion, push, deployment, or release is claimed.

### P3k: App and Diagnostics share one resource-sampling owner

- Diagnostics now consumes App's machine resource state instead of fetching a
  second copy. The App-owned visible-page poll runs every thirty seconds normally
  and ten seconds while Diagnostics is mounted; departure restores the ordinary
  cadence. Manual Refresh joins that same bounded request owner.
- Standalone Diagnostics retains its existing bounded local read when no App
  owner is supplied. Host/history polling remains separate. Failed resource
  readings stay unavailable rather than reusing a successful result as current.
- All 67 affected App, Settings, Diagnostics and polling tests plus the production
  web build passed. The App integration test counts one resource read on entering
  Diagnostics and one on manual refresh, without a duplicate child request. The
  component test verifies sampling registration/cleanup and unavailable evidence.
- No cross-window cache, live CPU reduction, or real-device acceptance claim.
  Existing terminal bundle warning remains. No push, deployment, or release.

### Full browser checkpoint at `67eefee`

- Verified clean implementation revision `67eefee`; refreshed origin refs.
  `origin/main` remains `7b02058` and has no commits missing from this branch.
  No unrelated remote branch was merged and no ref was pushed.
- Full web suite passed: 120 files, 993 tests, using two Vitest workers.
  This includes terminal ownership/render fixtures, composer and attachments,
  attention/queues, diagnostics, presence, settings, and the App integration tests.
- All nine `pnpm test:dogfood` tests passed, including process-sample disappearance,
  sustained-growth calculations, and verification-entrypoint failure behavior.
  These test the measurement tools; they are not a browser or live-worker soak.
- No runtime source changed during this checkpoint. The production build passed
  at the preceding implementation checkpoint. Dependency audit, complete native
  Linux integration execution, live CPU/latency attribution, real Android/iOS
  AskUser and attachment journeys, and operator soak remain separate gates.

### P5m: stale browser presence responses and permission completions

- The presence controller suppresses a response superseded by a queued device
  observation or an independently supplied mode. It still sends the latest
  pending observation, and a queued desktop-return flag survives a later ordinary
  heartbeat. These are browser publication guards, not server mutation fencing.
- Lock permission requests coalesce. A delayed grant after stop/restart cannot
  install a detector or publish status into the replacement controller. An
  aborted detector startup returns false instead of claiming successful setup.
- Passed 42 controller/App tests, then all 11 controller tests after final abort
  handling refinement; production web build passed. Corrected overly narrow mock
  call tuple types found by TypeScript. Existing terminal chunk warning remains.
- Network request deadlines, cross-client/server ordering, and atomic ordering
  with manual-mode writes remain open. No live lock permission was requested,
  service changed, deployment performed, or full presence acceptance claimed.

### P5l: Reachable distinguishes phone use from desktop engagement

- Automatic At Hive now requires fresh desktop evidence. Phone activity remains
  Reachable, cannot override a reported desktop lock, and does not silence the
  existing away-policy notifications. Manual/scheduled Night Watch precedence
  and desktop-return dismissal remain intact. Explicit manual At Hive is still
  operator intent. Reachable is availability policy, not proof a phone is online.
- Moved device precedence from SQL into the domain over a bounded evidence list.
  Lock/idle overrides that device's recent activity; hidden activity bridging
  shares the existing active lease; expired and future recent-activity evidence
  cannot establish engagement. Reads detect over-capacity instead of truncating
  an authoritative set silently.
- Domain and UI name the middle mode Reachable. The installed `away` API/storage
  token and saved Queen autonomy field remain stable, with explicit serialization
  regression coverage and ADR 0018 ownership. No database migration or preference
  reset is needed. Capability inventory updated.
- Passed 76 domain tests before the final serialization test addition, then all
  three focused presence-policy tests; 11 presence persistence tests, nine Night
  Watch tests, nine notification tests, 58 web tests, and production web build.
  Final strict Linux-target API/persistence all-target/all-feature Clippy passed
  after replacing an unchecked count cast. Linux API tests compile but were not
  executed. Existing terminal bundle-size warning remains.
- Late device-observation ordering, real lock detection on Android/iOS/desktop,
  and actual notification delivery remain acceptance work. No live worker change,
  push, deployment, release, or full P5 completion claim.

### P3j: screenshot downloads have an owned transfer lifetime

- Diagnostics admits one screenshot transfer at a time and disables other
  download buttons while it is pending. The request has a 30-second deadline;
  leaving the view or changing authentication cancels it and removes its timer.
  A late result cannot initiate a browser download or clear a newer transfer.
- Connection failures no longer assert the screenshot has been deleted. Timeout
  and failure messages permit explicit retry. Object URLs are released even if
  browser download initiation throws. No automatic download retries are added.
- Thirteen focused download/diagnostics tests passed, including timeout/retry,
  overlap rejection, authentication replacement, late unmount response, successful
  download, and object URL cleanup. Production web build passed after correcting
  the React ref's explicit undefined initializer. Terminal chunk warning remains.
- No live/private screenshot downloaded, worker changes, deployment, or release.
  Real mobile download behavior and measured long-session performance remain open.

### P3i: saved diagnostic reports own bounded, cancellable reads

- Saved-report reads now share the visible-page request owner with an explicit
  no-periodic-polling mode. Eight-second deadlines expose a retryable unavailable
  state; repeated retries coalesce, hidden pages do not start reads, and departure
  cancels the request. Visible return refreshes the five-report list. Superseded
  results cannot publish after cancellation.
- Sixteen focused diagnostics/request-owner tests and the production web build
  passed. The integrated report test covers timeout, no automatic retry traffic,
  repeated Retry, successful recovery, feedback refresh, and unmount cancellation.
  The existing terminal bundle-size warning remains.
- Screenshot download ownership and shared server sample ownership remain open.
  No live CPU improvement, mobile acceptance, push, deployment, or release claim.

## P6 visual proposal — awaiting operator review

- Prepared an interactive, sample-only Needs You/Queues composition for review
  in the goal conversation. Honey/sage surfaces, concise decision questions,
  visible worker and Queen recommendations, expandable evidence, direct answers,
  owner-grouped queues, and runtime diagnostics preserve the approved direction.
- Separate Edge preview verified desktop rendering and local answer transitions:
  both answered cards disappear, the attention count reaches zero, the operator
  queue disappears while Queen/worker holds remain, and the affected roster entry
  no longer claims to need the operator. No real task or live API was contacted.
- The conversation preview is `swarm-attention-proposal.html` in the thread-owned
  visualization directory. Bee emoji is placeholder artwork, not a replacement
  for Swarm's production identity. This is a composition proposal, not a complete
  Workers or Diagnostics redesign. Mobile and dark-theme visual checks remain.
- Production UI adoption awaits the requested mockup approval. No phase completion,
  release, deployment, or performance improvement is claimed by this review.

## P0 — Reconciliation and baseline (in progress)

### Upstream integration through `7b02058`

- Refreshed origin/main and integrated eight commits since `36420b3`, preserving
  upstream decision discharge evidence, strict shared task/decision projections,
  operator next-move ownership, and engine-update consequence reporting.
- Resolved the Queues conflict by retaining both operator ownership and explicit
  unknown ownership, plus this branch's held-delivery and refresh-failure behavior.
  Added an operator-group assertion beside existing unknown-owner coverage.
- Verification before merge: full web suite, 119 files / 982 tests passed.
  Combined branch: 71 affected web tests and production build passed; 74 domain
  and 31 decision-related persistence tests passed; native persistence strict
  Clippy and Linux-target API all-target/all-feature strict Clippy passed.
  Linux API tests compiled, not executed. Existing terminal bundle warning remains.
- This is a local integration only, not a release or live acceptance. The package
  reconcile timer still restarts the engine outside the API revival path; reliable
  return of its formerly running workers remains P5 work. Upstream's warning
  about that consequence is retained, not mistaken for the approved final behavior.

- Read charter, architecture, resource/diagnostic boundaries and relevant decision records.
- Refreshed origin/main: no newer commits at execution start.
- Recorded recent patches as unverified against reported live failures, not closed defects.
- Dedicated Edge tab reached the unlock screen; authenticated live baseline pending.
- Web build baseline started on Windows. Rust toolchain not present on local PATH
  or at the usual user cargo location; establish an isolated verification route
  before Rust changes. Do not borrow or modify the live development checkout.
- Planning docs and resolved interview decisions are the first commit checkpoint.

## P1 — Measurement foundation (in progress)

Deliver the bounded browser recorder before more extensive Dogfood dashboards.
Keep native browser capability gaps explicit. Record checks and evidence here.

### P1a: browser timing recorder

- ADR 0060 defines the single content-free browser recorder and its lifecycle.
- Native long-task/interaction timings, route timings, terminal queue/render
  latency, and attachment-to-render readiness now feed bounded aggregates.
- One-hour/360-bucket ring, five coalesced before/after incident windows, 24-hour
  expiry, before-reload snapshot, sanitized storage reads, and optional storage.
- Settings Diagnostics exposes collection availability and historical counts;
  diagnostic reports include this evidence without creating operator alerts.
- Full web suite passed at the first verification point: 111 files, 875 tests.
  Subsequent focused recorder/terminal integration tests passed: 36 tests,
  including two new tests beyond that full-suite run. Build passed; final build
  repeated after the last recorder edits before commit.
- Existing Vite warning remains: terminal chunk exceeds 500 kB. No release built
  or deployed; `web/dist` is only a local production-build validation artifact.
- Instrumentation overhead, actual Edge rendering, Android/iOS, and the long
  operator soak are not yet measured. P1 server correlation/Dogfood UI remain open.
- Dependency restore used the existing pnpm lockfile, unchanged. Installed pnpm
  runner reports 11.19.0, versus CI's declared 11.16.0; record this environment
  difference rather than claim exact CI replication.

### P1b: diagnostics request ownership

- Runtime diagnostic requests now accept cancellation, have an eight-second
  deadline, do not overlap within the view, and stop when hidden/unmounted.
- Database status is reachability evidence, explicitly not an integrity check.
- Focused diagnostics/report recovery checks: 13 passed after correcting an
  invalid test fixture. Settings/API regression tests: 27 passed. Production
  build passed; only the existing terminal chunk-size warning remains.

## P2 — Targeted continuity fixes (in progress alongside remaining P1)

### P2a: upload and submission races

- Found a stale React closure in attachment completion: a comment promised
  current connection state but the code used the pre-upload value.
- Browser socket writes now return acceptance/refusal (not provider acknowledgment).
  Upload reference insertion reads current connection state and actual write
  result; stale connected UI cannot discard a reference and report it ready.
- Upload completion after view disposal cannot inject into an abandoned session.
- Mobile Send retains a refused draft, prevents duplicate submission during paste,
  blocks submission while an attachment is uploading/waiting, cancels delayed Enter
  on disconnect/unmount, and never auto-replays it on reconnect. Existing provider
  paste pacing remains; wire-level provider acknowledgment is not claimed.
- Terminal/controller/view/workspace tests: 67 passed. Subsequent combined mobile,
  view, and connection tests: 59 passed. Production build passed after changes.
- These fix demonstrated code races, not all camera/gallery/AskUser problems.
  Real-device reproduction and full attachment preview/retry lifecycle remain open.

## P3–P7 — Implementation checkpoints (phases remain incomplete)

### P5k: revival completion retains lifecycle ownership

- Moved promise settlement and start-failure publication into the lifecycle-locked
  revival operation. Removed post-return clearing/error writes from supervisor
  and maintenance callers, including their unguarded already-running shortcut.
  A concurrent API operation cannot record a new promise between this start and
  its settlement or have its newer success overwritten by the earlier caller.
- Recheck host drain under the lock before starting; a drain hold preserves the
  promise. A compatible older host is not itself a reason to refuse an explicit
  provider-restart return. The supervisor retains its separate engine-ready gate.
- Verification: strict Linux-target API all-target/all-feature Clippy passed.
  Extended integration test compiles for drain hold, preserved promise, and
  already-running settlement without replacing the session; not executed here.
  No live workers touched. Durable per-attempt identity, process-crash ambiguity,
  failed persistence during settlement, and full Linux recovery execution remain
  open. This is not crash-safe convergence or phase completion.

### P5j: compatible package updates record loaded workers before replacement

- Added authenticated, no-store prepare-return endpoint. It requires host drain,
  shares the worker lifecycle lock, maps running host sessions to loaded profiles,
  and records the existing bounded durable intents without stopping anything.
  Repeated preparation does not create duplicate promises; sleepers are excluded.
- Compatible-engine package reconciliation calls preparation after its existing
  idle checks and before changing the host link. Recording failure refuses the
  swap; existing exit cleanup cancels drain. Requests are bounded to ten seconds
  and use the existing protected credential/output boundary. Empty engines skip
  preparation, preserving card-requested upgrade compatibility with older APIs.
- Updated consequence copy to distinguish recovery attempts from confirmed
  context restoration. No actual updater/service was run against the live Hive.
- Verification: strict Linux-target API Clippy (including new route test) passed;
  route test compiled, not executed. Focused production shell-helper tests passed
  for empty/success/failure/missing-config and credential-file cleanup. Both shell
  files passed syntax checks. 23 update-card tests and web build passed.
- Full package lifecycle harness failed at its initial Windows symlink assertion
  (line 253), before reaching the added replacement/failure assertions. Full Linux
  lifecycle execution, real worker return, protocol-migration preparation, atomic
  idle/input exclusion, and generation-bound revival settlement remain pending.

### P6a: diagnostics belongs with runtime status

- Removed the duplicate-location diagnostics shortcut from the brand header and
  placed the single shortcut in the existing runtime footer beside system status.
  It has visible Diagnostics text, a 44px minimum height, and the existing
  diagnostics destination. Branding and bee artwork are unchanged; no polling,
  animation, notification count, or new backend work is added.
- Verified the real App against isolated synthetic fixtures in a separate Edge
  tab: screenshot reviewed, footer control present, click opens diagnostics.
  All 31 App tests passed, including unique control placement and navigation;
  production web build passed after correcting a test-only TypeScript option.
  Existing terminal bundle warning remains. Mobile runtime access and the broader
  mockup-gated visual redesign remain open; this is not P6 completion.

### P2r: Codex honors a saved conversation before its first Swarm session

- Fixed Codex startup selecting New despite a saved conversation when the
  worker had no recorded Swarm terminal history. Exact identity now takes
  precedence; history without identity selects native Continue, neither selects
  New. No provider switch or running-session mutation is introduced.
- Added a four-case request-builder regression test. Linux-target API strict
  Clippy with all targets/features passed, compiling the test. It was not executed
  on Linux; real provider startup acceptance remains pending. The complete
  interactive recovery ladder and provider session-event integration are still
  incomplete; this change does not establish restoration success.

### P3h: hidden control-room feeds relinquish presentation work

- Hidden documents no longer start or retain the control-room long poll and its
  snapshot invalidation requests. Return reconnects immediately using the existing
  cursor handshake. The last rendered snapshot remains; no provider/worker stop
  or terminal-lifecycle operation is involved. Push subscription ownership is
  independent and unchanged.
- Verification: 42 App/feed/model tests passed, including hidden startup with no
  event request, visibility return, hidden cancellation, another return, and
  unmount cancellation. Production web build passed (existing terminal chunk size
  warning). Before/after live CPU and suspended-device acceptance remain pending.

### P4f: failed coordination reads do not imply resolution

- Preserve held deliveries, briefings, blocked observations, and unsettled review
  after a failed coordinator read. Successful empty responses still clear them;
  logout clears retained state. Deadline failures qualify the observation while
  visibility/unmount cancellations do not manufacture an outage.
- Needs You and Queues show a concise last-known-work qualification rather than
  an all-clear during refresh failure. This is status, not an extra attention
  count, timer escalation, or claim that Queen is stuck.
- Verification: 61 App/DecisionInbox/Queues tests passed, including retained
  briefing visibility across failure, zero added badge count, and successful
  empty recovery. Production web build passed; existing terminal chunk warning
  remains. No live outage was induced or real worker interrupted. Real-browser
  visual acceptance and broader decision reconciliation remain outstanding.

### P4a: age is queue evidence

- ADR 0061 records the approved replacement of timer-only operator escalation.
  Removed the aged-block card and its contribution to both Needs You counts.
  Existing actionable decisions remain untouched.
- Queues retains the coordinator's age evidence, deduplicates it against the
  task list, and does not resurrect tasks already known closed. Missing next
  ownership now has an explicit group rather than “Nothing is waiting”.
- App/Queues/count-invariant tests: 39 passed. The prior App test asserting the
  superseded twelve-hour behavior failed as expected and was replaced with a
  behavioral Needs You-to-Queues routing assertion. Typecheck passed.
- No Rust runtime change or new escalation timer. Queen-generated actionable
  escalation, stale decision reconciliation, and broader queue ownership remain
  open; this is not full ATT-01/QUEUE-01 completion.

### P3a: bounded visible status polling

- Runtime-update and development-status polling now share a visible-page owner:
  one request in flight, eight-second cancellation deadline, abort on hide or
  teardown, and immediate refresh after a rapid hide/show cancellation settles.
  Polling intervals remain freshness optimizations, not correctness evidence.
- Aborted old requests cannot replace newer hook state. Existing last-known
  update information remains available across API interruptions.
- Focused checks: 37 passed, then five polling lifecycle tests including rapid
  hide/show. Production build passed. Full unchanged-tree suite: 113 files,
  899 tests passed. An earlier full run overlapped an edit and failed the newly
  added test; it was superseded by this clean rerun, not counted as a pass.
- No measured CPU improvement is claimed yet. Other App-level polls and live
  feed behavior remain under audit; terminal-cache experiments are not enabled.

### P3b: avoid hidden transcript scans

- Conversation-history and public-address polling now use the same bounded
  visible-page owner. Hidden windows do not start transcript filesystem scans;
  visibility restores freshness without waiting two minutes.
- App/API/remote-access tests: 38 passed. Added a direct hidden-to-visible App
  regression test: all 30 App tests passed. Typecheck passed.

### Pending verification permissions

- The separate Edge tab still requires operator unlock; requested asynchronously.
- A source-transfer safety check rejected uploading the tracked-source archive
  to bgsdev because prior SSH permission covered read-only diagnostics. No source
  was transferred and no remote tests ran. Explicit permission requested for an
  isolated RAM-backed test directory with one CPU, 4 GiB RAM, no swap and a
  15-minute ceiling. The empty directory is `/dev/shm/swarm-maturity.fidSZS`.
- These are verification dependencies, not claims that P0–P7 are complete.

### P1c: developer evidence home

- Developer Dogfood is a separate Settings section, gated by existing development
  detection, with no additional enable switch or release action. Search follows
  the same gate. API interruptions retain the last-known development flag.
- It separates running build from checkout revision and shows bounded browser
  samples, missing observer support, incident counts, and explicit local preview.
  Means/maxima are labeled accurately; historical evidence is not an active alert.
- Settings/component/navigation tests: 31 passed; runtime hook/component/navigation
  tests after interruption coverage: 16 passed. Typecheck and production build
  passed. Existing 537 kB terminal chunk warning remains.
- This is the evidence home, not completion of DOG-01: long-term revision storage,
  comparison, overhead validation, and real-session browser evidence remain open.

### P2b: interrupted picker evidence

- A five-minute, timestamp-only pending-picker marker survives page recreation;
  it reports interruption without retaining filenames or image contents.
- Native picker cancellation remains quiet and clears the marker. Picker-return
  timers now have real cleanup rather than returning an ignored cleanup function
  from a DOM event listener. Unexpected upload callback rejection is visible.
- Composer tests: 21 passed. Production build passed, then typecheck repeated
  after the timer-test assertion was corrected to inspect its owned timer rather
  than counting unrelated jsdom timers.

### P2c: owned attachment recovery

- One in-memory File selection owns upload/retry/cancellation across picker,
  paste, and drop. A visible size placeholder avoids decoding large animated
  images as a side effect of diagnostics. Uploads have a 60-second deadline.
- Failed selections can be retried without reopening the picker while the page
  retains the File. Send waits for retry or removal; the text draft is preserved.
- Waiting references are bound to their original session and controller. Removal,
  cancellation, unmount, and session changes cannot cause late reference insertion.
  A ready reference is dismissed, not falsely removed from the provider prompt.
- Upload replies require a usable path without terminal control characters.
  The existing content-addressed server store makes retry storage idempotent;
  cancelling a pending UI selection does not delete a shared artifact.
- Focused attachment/composer/view tests: 71 passed. Production build passed
  after correcting new refs to explicitly allow undefined (React types caught it).
  Actual camera/gallery and provider receipt remain real-device verification.

### P2d: renderer cleanup and safe question fixture

- Failed WebGL activation now disposes its allocated addon; context loss during
  activation cannot leave a stale addon reference. Renderer disposal is idempotent
  and clears renderability listeners. Renderer/controller tests: 57 passed;
  typecheck passed. Tests initially lacked the existing ResizeObserver test stub;
  adding it allowed the intended activation-failure assertions to run.
- Extended the existing no-proxy harness with `terminal-questions`. Run
  `pnpm --dir web harness` and open
  `http://127.0.0.1:5199/harness.html?surface=terminal-questions`.
- Edge plugin verified synthetic screens 1, 2, 3 at 36 columns and canonical
  snapshot replay of screen 3. DOM inspection and a screenshot showed screen 2
  without the previous screen's marker. This uses the harness's DOM fallback,
  desktop Edge, synthetic full-clear ANSI, and no provider/server transport.
- TERM-02 remains open: this is not an Android/iOS run or a recorded Claude
  AskUser sequence and does not reproduce or establish a fix for its overwrite.

### P7 foundation: trustworthy verification entrypoint

- `scripts/verify.sh` now rejects invalid/extra modes, pins Rust 1.97.1, includes
  the release-mode terminal resize test, and uses CI's pnpm audit/check/test/
  dogfood/build commands. Dependency setup and other CI jobs remain separate.
- Executable stub-command tests prove command selection, nonzero failure
  propagation while subsequent checks run, and parity with the Rust/web workflow
  command lists. Nine dogfood tests passed; these are not actual Rust test runs.
- Production build caught a duplicated catch in the fixture's post-inspection
  error handling. Removed the duplicate and reran the production build: passed.
  The terminal chunk size warning remains (about 539 kB).

### P4b: readable decisions and owned draft state

- Long summaries can be expanded without losing their text. Supporting evidence
  joins the folded explanation; risk and requested commands remain accessible
  in the main decision flow. Recommendation emphasis follows a matching allowed
  action rather than arbitrarily favoring the first button.
- Pending notes survive refresh; resolved requests release draft and confirmation
  state. Stable pending ordering now uses indexed lookups. This does not infer
  that a similar worker conversation resolved a request: reconciliation is pending.
- Updated isolated Needs You fixtures to exclude age-only waiting cards, with
  separate Queues fixtures. Edge screenshot verified the synthetic recommendation
  highlights the second action correctly and retains the existing bee styling.
- Decision/interview tests: 29 passed. Production build passed; existing terminal
  chunk warning remains. This is desktop fixture evidence, not mobile acceptance.
- Post-commit full web suite on unchanged runtime source: 113 files, 916 tests
  passed (two workers). This does not substitute for Rust or device tests.

### P2e: generation-bound ownership domain foundation

- ADR 0062 amends the historical implicit takeover rules. One constant-space
  engine-owned state models device plus view identity, generation, and monotonic
  lease expiry. A same-device popout is not the same interactive view.
- Automatic acquisition cannot displace a live owner. Explicit takeover checks
  its observed generation. Old input/resize authorization, renewals, releases,
  and delayed takeover requests fail after transfer. Disconnect itself is not a
  transition; expiry/reacquisition advances the generation. Failed proposals can
  be discarded without modifying the current owner.
- All 58 domain tests passed, including 12 new ownership tests. Domain formatting
  and Clippy (`--all-targets --all-features -- -D warnings`) passed on Rust 1.97.1
  Windows GNU. This is real compiled Rust evidence, not the earlier script stubs.
- The model is not yet connected to the engine, IPC, or browser. TERM-01 remains
  open until engine-serialized effects, rolling compatibility, and device tests
  prove the complete cutover. No live behavior is changed by this foundation.

### P2f: engine-serialized terminal effects

- Each process session now owns a control gate. Claim prepares a transition,
  applies geometry while holding the same guard, then commits. Input and resize
  validate their generation under the guard held through the effect. Failed
  input is called once, not retried, and does not earn a successful renewal.
- Generation-bound operator input goes through the existing content-free audit.
  Legacy operator input/resize cannot bypass a session that has entered the new
  contract, including after expiry/release. Coordination cannot inject beneath a
  live owner; its existing application authorization remains required.
- Seven gate tests cover guard lifetime, stale-effect rejection, failed proposal
  rollback, uncertain input, passive reads, and compatibility boundaries. Two
  additional disposable-PTY tests verify handoff geometry, stale writes/resizes,
  audit outcomes, live process continuity, and invalid-size rollback.
- All 70 terminal-library tests passed on Windows GNU after the final edits.
  Formatting and strict all-target/all-feature Clippy passed. The first portable
  run caught ten Linux-only workspace fixtures; replaced those fixture paths with
  platform-absolute paths without changing the asserted provider arguments.
- Unix socket imports/client and the Unix symlink test are explicitly Unix-only;
  Linux retains them unchanged. This permits the actual portable library and
  ConPTY tests to run locally, not a substitute test implementation or a new
  Windows server. Linux-specific process/socket tests remain unrun.
- IPC protocol and browser handshake remain unchanged, so existing sessions do
  not activate this contract yet. Next: negotiated host commands, authoritative
  control-state notification, engagement projection, and the browser cutover.

### P2g: typed engine control protocol

- Protocol 11 introduces typed status/claim/renew/release/input/resize commands,
  structured refusal codes, and controlled-output waits. The actual command
  dispatcher is exercised against disposable PTYs. Empty or over-64-KiB input
  is refused without extending ownership or reaching the writer.
- Control-change notifications are independent of terminal output. Waiters
  subscribe before observing; cursors distinguish ownership expiry from a live
  owner at the same generation. Remaining lease duration is projected without
  exposing or trusting a browser clock. Registry control claims and remote
  takeover checks exclude one another under the same lock ordering.
- Windows execution: 58 domain and 77 terminal tests passed. Strict Clippy for
  both crates passed. Linux-target compilation and strict Clippy passed for all
  terminal-library targets/features, including Unix-only source and test targets.
  Cross-checking is not execution of Linux tests.
- Full host Linux-target compilation and strict all-target/all-feature Clippy
  passed after establishing an isolated Zig C compiler. This includes the real
  AWS-LC dependency and host dispatch code, but is not Linux test execution.
  No source was transferred to the server.
- API/browser negotiation and cutover remain pending; current API requests still
  use the existing terminal path. Protocol support must be confirmed before new
  requests are sent. No worker engine update, deployment, or release was run.

### P2h: negotiated API adapter and ordered activity projection

- Added opt-in `swarm-terminal.v4` alongside v3; the browser does not request it
  yet. One-time grants bind session and protocol. Engine support is checked at
  grant issuance and attachment; unsupported engines get read-only output, not
  unrestricted writes. Capable-engine grant validation reads control status
  instead of copying a terminal snapshot solely to confirm session existence.
- V4 binds device and view identity in its handshake, validates decimal u64
  generations, rejects identity replacement, and uses the engine control gate for
  every effect. Control-aware output waits report handoffs without requiring
  output or a resize. Transport/writer waits are bounded and sibling tasks are
  cancelled on socket teardown. Uncertain input is never repeated.
- Schema 124 keeps one ordered control projection per worker, including an
  expiry/release watermark. Older generations and ended sessions cannot restore
  stale engagement. Legacy engagement endpoints cannot overwrite an activated
  projection. Typing bursts coalesce projection work without skipping engine
  authorization, and projection failure does not claim successful input was lost.
- The four new persistence tests passed on native Windows GNU. Full persistence
  run: 422 passed, seven home/path failures. All seven reproduced with the same
  failure reasons on preceding commit `346f823` in an isolated local worktree;
  they are baseline Windows environment failures, not introduced by this slice.
  New schema ceiling/data-preservation/recovery checks passed in that full run.
- Linux-target strict all-target/all-feature Clippy passed for the API and its
  local dependencies. API protocol/handshake tests were compiled, not executed;
  live Linux WebSocket/PTY and rolling API replacement tests remain outstanding.
- Browser v4 ownership state, Resume Here, foreground renewal, and corresponding
  device tests are still pending. No deployed behavior or worker process changed.

### P2i: returning to a healthy socket does not repeat resume

- Found a client/server contract mismatch: visibility return sent another resume,
  while the API rejects duplicate resume. The previous browser test supplied an
  invented snapshot response instead of the API's actual behavior.
- Replaced the second resume with one bounded, correlated read-only probe. The
  API echoes it without a PTY write, resize, engagement change, or journal copy.
  A matching reply retains the healthy socket; unrelated replies cannot confirm
  it. Repeated visibility events cannot extend the same probe indefinitely.
- Older v3 APIs lacking probes take the ordinary reattachment path quietly,
  without resending input. The terminal adapter owns this compatibility path;
  remove it with v3 support. V4 must support the same probe before activation.
- Added a constant-space browser control-state model for v4. Tests cover exact
  u64 ordering, stale handoffs, same-generation expiry, malformed status,
  read-only engines, and reconnect without treating transport loss as expiry.
  This model is not yet wired into the connection/controller or Resume Here UI.
- Final focused browser checks: 51 passed. Web production build passed (existing
  539-kB terminal chunk warning remains). An earlier full web run passed 935 tests
  before the final probe/model assertions; it is not a final full-suite result.
- Strict Linux-target API Clippy passed with incremental compilation disabled.
  The first attempt hit a Rust incremental-fingerprint internal compiler error;
  source was not changed to bypass it. The Rust probe test was compiled, not
  executed on Linux. Real PWA return latency remains to be measured.

### P2j: controlled attachments support read-only liveness

- Extended opt-in WebSocket v4 with the same correlated `probe`/`alive`
  contract as v3. Probe identifiers are nonempty and at most 64 bytes; extra
  fields are rejected. Probes are transport actions, not engine commands.
- Probes also work for read-only attachments to an older/unknown engine. They
  do not renew ownership, resize, read snapshots, or write engagement records.
  This removes a protocol prerequisite for the pending browser cutover; it does
  not enable v4 in the browser or prove stable device handoff.
- Linux-target strict API Clippy passed with all targets/features and
  `CARGO_INCREMENTAL=0`. The probe contract test compiled; Linux API tests were
  not executed. The final P2i TypeScript build check also passed.
- Rollback is API-only while v4 remains opt-in. No push, release, deployment,
  engine restart, or live-worker action was performed.

### P2k: browser-controlled terminal handoff

- Browser attachments now explicitly require v4; legacy grants fail visibly
  without creating an unrestricted socket. Older engines on v4 remain visibly
  read-only. Each retained controller owns a distinct view UUID across reconnects.
- Resume Here sends measured geometry and the observed generation together.
  Input is disabled until engine ownership is confirmed; input/resize carry that
  generation, and stale replies cannot undo a newer handoff. No input replay.
- Removed the controller's implicit one-time reclaim after snapshots. Passive
  views accept canonical geometry. Renderer fit also rechecks ownership after
  awaiting layout, preventing a late fit from changing a passive grid.
- Focused, visible ownership renews on a single bounded timer; losing focus or
  visibility stops renewals and local input permission. Returning probes the
  socket, then requests a non-displacing engine claim. Explicit worker navigation
  releases control, whereas hiding the PWA does not release its engine lease.
- The toolbar shows Resume Here for passive/available control, and does not hide
  it on mobile. A pending uploaded reference retries insertion on ownership
  change, retaining the existing session/controller identity guards.
- Verification: final terminal suite **196 passed / 11 files**; TypeScript and
  production Vite build passed. The broad web run before the final renderer fix
  had **943 passed / 1 failed**: the source invariant caught a second fit caller.
  That caller was moved back into the common helper, and the invariant plus all
  terminal behavior tests passed afterward. Do not describe that earlier full
  run as green. Terminal chunk remains above the existing warning threshold
  (**543.60 kB**, gzip 145.36 kB); no size/performance acceptance claim.
- This is local integration evidence, not real-device or rendered-browser
  acceptance. Android/iOS AskUser, repeated cutovers, PWA return latency, and
  rolling API/engine coexistence still need acceptance evidence. No deployment,
  release, push, or live-worker restart occurred.
- Rollout requires the v4 API and compatible engine for interactive control.
  Once an engine session accepts controlled input, reverting only the browser to
  v3 cannot restore legacy writes. Preserve the compatible adapter/engine during
  rollback; do not restart active workers merely to bypass the safety boundary.

### P2l: rendered handoff and ownership-aware mobile composition

- Updated the isolated visual harness from its obsolete grant/socket protocol
  to synthetic v4 messages. Added `terminal-handoff` with optional
  `terminalControl=elsewhere`; it renders the production controller, renderer,
  toolbar, and composer without a Hive, provider, engine, or network socket.
  Fixture claims/probes and close-before-open behavior have dedicated tests.
- Used the Chrome/Edge skill in a new Edge test tab. The earlier browser binding
  was unavailable; selecting the currently connected Edge succeeded. Local
  sandbox diagnostics were inconclusive and are not evidence of a broken install.
  The existing isolated harness on port 5199 was reachable; no duplicate server
  was started and no live worker tab was used.
- Desktop: passive notice and Resume Here rendered; clicking Resume Here removed
  the notice and restored interactive state. At a measured **390 x 844 CSS-pixel**
  viewport, Resume Here remained visible. The responsive override was reset.
- Browser testing found that passive Send preserved text but misleadingly blamed
  the connection. The composer now receives ownership availability: its draft
  stays editable while Send/terminal keys are disabled, then becomes sendable
  after Resume Here. Its accessible field name stays stable while the visual
  character counter changes. In the Edge fixture, the same synthetic draft was
  retained through handoff and cleared only after successful submission.
- Focused checks: **46 tests passed** across composer, terminal view, and harness
  protocol. Final TypeScript/production build passed after correcting test-only
  locator options. Existing terminal bundle warning remains (543.62 kB).
- This is rendered-browser wiring evidence, not real Android/iOS PWA behavior,
  provider acceptance, cross-process engine handoff, or a Claude AskUser fix.
  The synthetic socket has no provider and does not echo typed input.
- No releases, pushes, deployments, live-worker writes, or process restarts.

### P2m: bound the provider conversation probe

- Recovery audit confirmed the host still maps a recognized missing Claude
  conversation directly to New. That contradicts REC-01; the ladder and visible
  fallback reporting remain unfinished. Amended ADR 0011 to the approved target
  (exact/safe recovery, native continue, fresh last), explicitly marking this gap.
- Found a separate startup stability defect in that same path: waiting for exit
  before draining stderr can fill the child's pipe, and reading to EOF after exit
  can wait indefinitely if a descendant retains the pipe. Stderr was unbounded.
- The owned probe now reads nonblocking while the child runs, enforces the
  existing 20-second deadline and a 64-KiB stderr limit, and kills/reaps its child
  on errors or overflow. Only a recognized exit-1 response is absence evidence;
  all uncertain outcomes preserve the exact-resume attempt. No stderr content is
  logged. This does not itself implement continuation or fresh-fallback reporting.
- Added tests for a small missing result, misleading success, overflowing output,
  and a silent process timeout with reaping. Strict Linux-target host Clippy passed
  with all targets/features after correcting one lint. These tests compiled but
  were not executed on Linux. No live Claude process was invoked for this check.
- No provider/engine restart, push, release, or deployment. Engine rollout remains
  a separate operator-controlled action; an API-only update cannot apply this fix
  to an already-running older host.

### P2n: explicit conversation recovery transitions

- Added a domain-owned recovery operation with unique operation identity and
  numbered attempt tokens. Exact -> Continue -> Fresh is bounded to three
  attempts. Late, duplicate, forged-step, and other-operation evidence cannot
  advance it. Final failure and uncertain outcomes stop rather than loop.
- Exact restoration requires the selected conversation identity. Continuation
  results remain distinct from exact restoration, and a fresh result is never
  called restored context. Providers without supported resumption stay manual.
  No domain transition authorizes task-command replay or provider substitution.
- Verified current [Claude session documentation](https://code.claude.com/docs/en/sessions):
  interactive and print-mode continuation can consider different session sets.
  Therefore the existing exact-ID print probe must not be generalized into a
  purported proof of interactive continuation. Added this integration constraint
  to ADR 0011.
- **67 domain tests passed natively**, including nine recovery tests; strict
  all-target/all-feature domain Clippy passed. This is executed domain evidence,
  not a provider or Linux-host integration test.
- The new model is not yet wired into worker startup. The existing host's direct
  missing-to-New branch, durable recovery reporting, provider-native evidence,
  and chosen-conversation switch tracking remain open. Next integration must
  carry operation/attempt identity through the actual provider lifecycle and
  expose fresh fallback honestly; a print-mode continuation shortcut is not valid.
- No schema/protocol activation, live provider invocation, worker restart,
  release, push, or deployment occurred.

### P2o: wire confirmed missing context to native continuation

- Replaced the host's direct exact-resume-to-fresh selection with the domain
  Exact -> Continue transition. Only the recognized missing-context probe result
  advances it; inconclusive probes preserve the exact interactive attempt.
- Recovery logging says continuation is being attempted and context is not yet
  verified. The saved ID is not reused to manufacture a fresh conversation.
- Updated the selector regression and added a configuration/one-probe test:
  continuation retains explicit MCP isolation and operator settings.
- Strict Linux-target all-target/all-feature host Clippy passed, including test
  compilation. These host tests were not executed on Linux. This is not evidence
  of successful interactive provider restoration.
- Remaining: carry the recovery operation through interactive execution, obtain
  provider outcome evidence, persist/report fallback, implement the final fresh
  attempt and authoritative chosen-conversation tracking. This checkpoint does
  not claim the full recovery ladder or P2 complete.
- No live worker changes, push, deployment, or release.

### P2p: preserve and display recovery startup provenance

- The actual fallback attempt token now travels from host selection into the
  engine-owned session. A write-once value prevents later callers from replacing
  that startup provenance. Session enumeration exposes it without terminal reads,
  new polling, or coupling to browser/API lifetime.
- The existing authorized session-list API forwards the optional metadata. Older
  hosts remain readable with no field; omission is not restoration evidence.
  No protocol request or provider invocation was added.
- The terminal shows a compact continuation-fallback note, with startup detail
  explaining that Swarm has not verified which conversation was restored. It
  does not create a Needs You item, mark work failed, or claim a task resumed.
  Switching to a normal session removes the note.
- Verification: 21 terminal-view tests passed; TypeScript build checking passed;
  78 Windows-compatible terminal-library tests passed, including optional-field
  serialization. Strict all-target/all-feature Linux-target Clippy passed for
  terminal and host, including compilation of the session-retention regression.
  Linux-only tests were not executed. Rendered browser/device review remains open.
- Persistence across engine replacement, authoritative provider outcome evidence,
  final fresh fallback, and chosen-conversation switch tracking remain open.
  This is startup provenance, not the completed recovery state machine.
- No push, release, deployment, or live-worker restart.

### P2q: atomic, observable operator conversation correction

- Inspection confirmed the existing explicit conversation-correction endpoint
  saved a new ID without publishing a worker event or serializing with startup.
  The API now takes the same lifecycle guard as startup and notifies control-room
  waiters after persistence succeeds.
- Saved identity and WorkersChanged commit together. Failure to record the event
  rolls back the identity change; selecting the same ID again is a no-op rather
  than generating duplicate history. The live terminal remains unchanged.
- Five focused persistence tests executed successfully on Windows, including
  rollback, duplicate selection, live-session preservation, and existing repair
  cases. Strict Linux-target all-target/all-feature API Clippy passed. The new
  deterministic API lifecycle/notification regression compiled but was not
  executed on Linux; no elapsed-delay assumption is used in that test.
- The first native test attempt selected the Linux C wrapper and failed before
  test execution. Rerunning with the existing `zig-windows-cc.cmd` succeeded;
  production source and compiler checks were not weakened for that failure.
- Reviewed the official Claude SessionStart contract and recorded its integration
  constraints in ADR 0011. Automatic in-terminal switch observation remains
  unimplemented; this checkpoint fixes the explicit correction path only.
- No live provider invocation, worker restart, schema change, release, push,
  or deployment.

### P3c: opt-in five-renderer pool experiment

- Confirmed the browser registry retains every visited renderer/controller/socket
  until explicit close or logout. Added the approved opt-in LRU policy: five
  retained browser views, with attached and incoming views protected. Worker
  processes and canonical history remain engine-owned and are not stopped.
- Developer Dogfood exposes a reversible switch and content-free retained,
  attached, inactive, and eviction counts. No new polling or storage is added.
  Default behavior is unchanged; reload/logout resets the experiment. Disabling
  it does not eagerly recreate cold views. Updated ADR 0004 for policy ownership.
- Late output to disposed renderers is ignored. A pending snapshot cannot refit
  after disposal/detachment, and geometry ownership is re-read after async fit.
- 205 terminal/Dogfood tests passed. TypeScript then caught an ownership-property
  narrowing across await; the check now reads current ownership via a helper.
  Final TypeScript checking and 30 affected controller/Dogfood tests passed.
  Production web build passed; the existing over-500-kB terminal chunk warning
  remains (544.45 kB uncompressed), not waived as an efficiency result.
- Chrome/Edge skill used a separate local isolated fixture tab to inspect the
  rendered control and verify enable/disable. No live Swarm tab or provider was
  touched. Added a reusable Developer Dogfood fixture for subsequent visual work.
- The experiment is NOT adopted as normal policy. Representative p95 cold-restore
  timing, CPU/resource comparisons, sustained output fidelity, and real-device
  cutover remain required. Pool counts and synthetic checks do not prove savings.
- No push, release, deployment, or live-worker restart.

### P3d: bounded cold-return timing evidence

- Added content-free experiment timing from cold view attachment to the
  connection's rendered-snapshot confirmation. Only returns from the last 64
  known evictions qualify; first visits and ordinary warm returns do not.
  This is view-render readiness, explicitly not proof of input ownership.
- At most 200 completed samples from the last hour are retained. Nearest-rank
  p95/max, pending, failed, and hidden/abandoned attempts are exposed in Dogfood
  and its manual evidence preview. Small sample sets are labeled insufficient
  for rollout decisions. No timer, terminal content, worker ID export, or
  persistent storage was added.
- Stopping retains results and invalidates pending completions; a new experiment
  resets evidence. Late/duplicate completions cannot alter a later experiment.
  Remounts can start a new attempt before initial rendering completes, without
  counting subsequent warm remounts as cold samples.
- 210 terminal/Dogfood tests passed and production web build passed. The terminal
  bundle remains over the existing warning threshold; no size improvement claimed.
- Edge lifecycle smoke fixture: visited six synthetic workers, then returned to
  the first. The UI reported five retained renderers, one attached, four inactive,
  two evictions, and one completed cold-return sample. This validates wiring only:
  fixture transport is synthetic and rendering uses the harness DOM fallback,
  not production transport/WebGL, representative workload, or real mobile hardware.
- Full cold-input-readiness gate, production p95/resource comparison, and normal
  soak remain open. The warm-pool experiment stays off by default.
- No push, release, deployment, or live-worker changes.

### P4c: bounded routine settlement and consistent worker guidance

- The existing non-deployment completion sweep now evaluates at most 64 review
  candidates per pass. An AppState-owned cursor advances past unresolved work,
  wraps after exhaustion, and uses a nonblocking lock to avoid duplicate page
  scans. No additional timer or worker interruption was introduced.
- Existing completion evidence rules are unchanged. Worker brief/refusal text
  now directs workers to Review with evidence and describes automatic routine
  settlement, rather than incorrectly requiring Queen approval of every result.
  Unsupported self-approval remains refused. ADR 0013 records this distinction.
- Eight focused persistence settlement tests passed on Windows, including page
  bounds, unresolved candidates, wrap/exhaustion, missing evidence, code changes,
  and work owing an email reply. Two focused application authority/refusal tests
  also passed. Final strict Linux-target API/persistence all-target/all-feature
  Clippy passed; the added API cursor/ownership test compiled but was not executed
  on Linux. This is not a full workspace or live Queen acceptance result.
- The candidate bound limits materialization and per-task evidence processing,
  not all SQLite query cost. The deployment sweep remains a separate optimization
  item. No server CPU improvement or full completion-trust audit is claimed.
- The cursor is process-local and restarts from the beginning after API restart;
  task evidence remains durable. Existing sessions were not restarted to reload
  standing guidance. No push, deployment, release, or live task mutation.

### P4d: delivery observations are not operator escalations

- Ordinary prompt/unsent-text delivery holds now appear in Queues, grouped by
  their target, with recorded details collapsed. They no longer create a Needs
  You card or increment its count. Unconfirmed wakes and unknown hold kinds
  retain the existing recovery-attention path; explicit decisions are unchanged.
- Replaced the blanket claim that Queen cannot review or nothing can route with
  a description of last recorded delivery evidence. A hold is not proof that the
  worker stopped. Empty queue copy no longer hides a delivery-only wait.
- 48 focused App/queue/attention tests passed, including clearing refreshed holds,
  routing/count consistency, and preserving recovery-card behavior. Production
  TypeScript/Vite build passed; the existing terminal chunk warning remains.
- The first test run clicked navigation before saved-surface initialization had
  completed. Tests now wait for the actual inbox and restore real timers during
  cleanup; the corrected run passed without changing production initialization.
- Chrome/Edge skill inspected the isolated local Queues fixture in a separate
  tab and expanded the details. Existing styles remain; no broad redesign was
  adopted. The initial root URL was refused; the harness-only entry succeeded.
- This does not clear stale persistence records or complete Queen recovery and
  escalation. Those remain P4 work. No live Hive mutation, push, or release.

### P4e: prompt-hold occurrence and session reconciliation

- Recording an open-question or unsent-text hold now replaces the incompatible
  prompt reason for that delivery in one transaction. Other subjects and wake
  recovery records remain intact. Changing worker/session binding resets the
  occurrence age and observation count rather than inheriting an older session.
- Standing prompt holds exclude sessions explicitly recorded as ended, without
  waiting for freshness expiry. Unknown/unbound session evidence is preserved.
- Nine focused refusal tests passed on Windows, including reason replacement,
  recurrence, session change/end, and an injected write failure proving rollback
  preserves the prior observation. Strict persistence all-target/all-feature
  Clippy passed after the final code change. No full-suite/live acceptance claim.
- Current provider-activity reconciliation, generation-safe rejection of late
  observations, and replacement of freshness-based inference remain open. These
  fixes do not prove the original live Queen-warning incident fully resolved.
- No schema migration, new timer, live worker mutation, push, or deployment.

### P5a: preserve explicit lock and idle evidence

- Found visibility events overwriting a detector's locked/idle observation, and
  server recent-activity grace overriding that same device's fresh locked/idle
  report. The browser now retains OS state across visibility changes; the server
  applies recent-activity grace only to hidden observations. Detector callbacks
  after abort cannot update presence or report enabled.
- Six focused browser controller tests passed, including visibility while locked
  and unlock recovery. Seven native persistence presence tests passed, including
  immediate lock/idle precedence, return to active use, capacity, expiry, and
  heartbeat event deduplication. Strict persistence Clippy and web build passed.
  The existing terminal bundle warning remains.
- This preserves existing manual modes and phone grace behavior. Reachable
  semantics, scheduled Night Watch, and desktop-return cancellation are not yet
  implemented. Real OS lock/device evidence remains required; tests use synthetic
  detector events and isolated persistence, not the operator's live machine state.
- No live settings, worker permissions, deployment, or release changed.

### P5b: explicit Night Watch policy model

- Added a timer-free domain model for daily local-time windows and manual
  enable/disable/automatic/desktop-return transitions. Overnight windows retain
  their starting-date identity. Desktop return dismisses the current occurrence
  without suppressing the next night; heartbeat evaluation cannot undo dismissal.
- Equal endpoints and out-of-day minutes are rejected. Repeated local-hour input
  retains the same occurrence, including after a dismissal. This is not yet a
  real time-zone/DST conversion test; the clock adapter remains to be built.
- All 73 domain tests passed, including six new schedule/policy tests. The first
  compile found duplicate module registration from patch application; removed
  that duplication and reran validation. Strict domain Clippy passed.
- ADR 0018 records the approved replacement of indefinite Night Watch override.
  Persistence, explicit-return API events, time-zone conversion, settings controls,
  and integrated acceptance remain open. No active runtime schedule is claimed.
- No dependencies, database schema, live configuration, push, or release changed.

### P5c: durable schedule configuration and named-zone clock

- Added validated named-zone/window configuration with no implicit bedtime,
  schema 125 storage, atomic settings/event writes, and no-op repeated saves.
  Schema upgrade preserves the existing manual presence mode. One nullable
  dismissal field is reserved for the next policy integration step.
- UTC-to-local conversion uses Chrono-TZ 0.10.4's bundled IANA data, not server
  locale. Verified New York spring-forward and fall-back instants against the
  overnight domain window and exact end boundary. Invalid zones/timestamps fail.
  Source contract: https://docs.rs/chrono-tz/0.10.4/chrono_tz/ .
- Six focused tests passed, including new reopen, schema-124 upgrade, event
  failure rollback, and DST checks plus existing manual-watch/authority tests.
  Strict persistence all-target/all-feature Clippy passed after final edits.
  Initial compile required an explicit connection guard; lint required splitting
  the migration dispatcher. Both were corrected and verification repeated.
- Dependency change adds chrono-tz and three transitive crates without updating
  existing locked versions. Fetch used the previously documented process-local
  revocation workaround; tests/lint ran offline with the final lockfile.
- No live database opened. Testing this build will introduce schema 125; older
  binaries cannot be assumed compatible. Keep the pre-upgrade database backup.
  Effective schedule evaluation, return events, and settings/API remain unwired;
  this checkpoint does not claim an available Night Watch schedule.
- No push, deployment, release, or live configuration change.

### P5d: schedule evaluation and explicit desktop return

- Effective presence evaluates configured windows with a distinct `scheduled`
  source. Existing manual overrides retain precedence. Selecting Automatic clears
  the dismissed occurrence. No configuration still means no scheduled watch.
- Presence observations now accept an optional `desktop_return` signal through
  HTTP/application/persistence. Only an active desktop observation may use it.
  It clears manual Night Watch and dismisses the current scheduled occurrence in
  the same transaction as the device update and presence event.
- Browser entry/visible return and interaction during Night Watch report return;
  normal heartbeats and mobile entry do not. App changes to manual presence inform
  the controller immediately, without waiting for its next heartbeat. Missing
  fields from older clients remain ordinary observations, never implicit returns.
- Seven focused persistence tests passed; 39 App/controller/API tests passed, plus
  the expanded HTTP serialization assertion. Strict persistence and Linux-target
  API all-target/all-feature Clippy passed. Production web build passed; existing
  terminal chunk warning remains. Linux API checks are compilation, not execution.
- Schedule configuration endpoints/settings controls and real-device acceptance
  remain open. Late-return ordering across network outages and concurrent manual
  changes requires further reconciliation; this is not full presence acceptance.
- No live settings, databases, workers, push, release, or deployment changed.

### P5e: Night Watch schedule editor and authenticated API

- Added private no-store GET/PUT schedule routes through the application service.
  Validated named zones and local windows are required before persistence. Added
  a route test for authentication, invalid-zone refusal without mutation, and
  read-back; it compiled in strict Linux-target API Clippy but was not executed.
- Presence settings now expose enable, time zone, start/end, and explicit save.
  Unknown/loading settings cannot be overwritten, requests have eight-second
  deadlines, duplicate saves are blocked, and failure retains edits with retry.
  Cleanup aborts requests and prevents abandoned save completion updating state.
- 22 settings/editor tests passed; final production build passed with the existing
  terminal chunk warning. The first settings test used positional status selection;
  it now targets the named Current presence status, preserving its assertion.
- Chrome/Edge skill inspected the separate isolated editor fixture, confirmed save
  feedback, and caught joined labels. Added scoped spacing and re-inspected the
  corrected layout. Fixture saves are synthetic, not live persistence acceptance.
- End-to-end Linux route execution, real device/DST/overnight acceptance, Reachable
  semantics, and stale desktop-return ordering remain open. This completes the
  schedule editing path, not P5 or the overall maturity goal.
- No live schedule, database, worker, release, push, or deployment changed.

### P5f: experimental-provider automatic wake admission

- Night Watch filters automatic wake candidates against the builder-owned
  non-alpha provider list before the bounded claim. Deferred work remains queued;
  eligible workers can pass it, and leaving Night Watch restores eligibility.
- One focused recovery test passed with Gemini, Grok, and OpenCode cases, each
  checking approved-worker admission, continued deferral, and later resumption.
  Strict persistence all-target/all-feature Clippy passed. Patch application
  duplicated a test and removed the constant twice; inspected and corrected both
  before compilation. No live provider was started or stopped.
- Other startup and task-delivery paths, explicit experimental opt-in, full
  provider readiness validation, and late presence-change ordering remain open.
  This is not complete Night Watch provider-policy enforcement.
- No schema change, push, deployment, or release.

### P5g: experimental-provider task briefing holds

- New task briefing claims now apply the builder-owned Night Watch provider
  gate before the batch limit. Existing sessions and submitted work are untouched.
  Queues explicitly identifies experimental-provider Night Watch holds rather
  than claiming they are behind earlier work. No operator escalation is added.
- All 16 persistence task-dispatch tests passed, including repeated holds with
  zero consumed attempts and delivery to the same session after exit for Gemini,
  Grok, OpenCode, and an unknown stored provider. Strict persistence Clippy passed.
  Ten briefing component tests and the production web build passed; the existing
  terminal bundle-size warning remains. No live browser/device acceptance claim.
- Automatic startup/recovery, message and decision delivery, and presence changes
  after claim still need policy reconciliation. Full provider maturity remains
  incomplete. No schema change, push, release, or live worker interruption.

### P5h: autostart and shared coordination admission

- Ordinary autostart checks provider eligibility before recovery-attempt
  accounting. Unavailable presence defers startup and emits a diagnostic.
  Shared coordination submission applies the same domain policy before terminal
  submission, covering decisions, messages, broadcasts, outcomes and Queen runs
  as well as task briefs. Policy holds use existing durable deferral paths and
  do not create operator attention or start a delivery cooldown.
- All 74 domain tests passed, including approved/experimental/unknown provider
  policy across presence modes. Strict Linux-target API all-target/all-feature
  Clippy passed after extracting the startup lookup to respect function limits.
  A no-terminal-contact integration test compiled but was not executed on Linux.
- Maintenance revival is still open: existing intents expire after 15 minutes,
  so simply skipping an experimental worker overnight would silently lose its
  promised return. Do not claim that path guarded or repaired. Manual starts,
  rolling restart paths, and presence changes during admission/submission also
  remain to be reconciled. No running work was stopped or modified.
- No schema change, push, deployment, release, or live acceptance claim.

### P5i: restart promises survive policy holds

- Removed timer-only deletion of maintenance restart intents. Admission is
  bounded to 256 durable worker promises and reads at most that many; overflow
  rolls back before maintenance is allowed to stop the recorded batch.
- Sleep/Stop and archive explicitly cancel revival; ordinary maintenance session
  release preserves it. Immediate and supervisor revival share a lifecycle-locked
  check of the pending intent and provider policy. Deferred starts retain their
  promise, while attempted failures keep the error rather than silently retrying.
- Two focused persistence tests passed for retention/cancellation and atomic
  capacity failure; strict persistence Clippy passed. Strict Linux-target API
  all-target/all-feature Clippy passed, including compilation of a no-host test
  for policy deferral and cancellation. Linux integration tests were not executed.
- No schema migration. Restart promises created by older builds now survive age;
  no live database was opened. Presence changes after admission, durable attempt
  identity across process crashes, and real maintenance recovery acceptance remain
  open. No engine update, live restart, push, deployment, or release performed.

### P1d: distinguish browser delay from server evidence

- Diagnostics and copied reports now assess recent browser timing separately
  from server pressure, including stale/incomplete readings and clock mismatch.
  They explicitly avoid inferring cause, Edge CPU utilization, or health from
  missing measurements. Historical incidents do not remain current faults.
- Reused the bounded browser recorder and existing diagnostic refresh; no new
  timer, collector, external telemetry, or Needs You alert. Extracted the existing
  compute-pressure classifier and rejected non-finite/negative inputs.
- Eighteen focused diagnostics/report tests and the production web build passed
  after the final copy/style refinement. The existing terminal chunk warning remains.
- Chrome/Edge skill rechecked a separate live Swarm tab: still at unlock. Asked
  the operator to unlock it; did not inspect credentials or their working tab.
  Separate synthetic Edge fixtures validated simultaneous/stale/missing evidence
  and caught long explanations pushed away from labels. Scoped left alignment
  was added and visually rechecked. This is not a live performance measurement.
- Current-build CPU attribution, real session degradation, mobile acceptance,
  instrumentation overhead, and long-term Dogfood comparisons remain open.

### P3e: visible-page ownership for background status reads

- Machine resources, coordinator holds, and unanswered email tasks now use the
  existing single-flight visible-page polling owner. Requests carry cancellation,
  have an eight-second deadline, stop on hide/unmount, and refresh on return.
  Canceled old responses cannot replace current UI state. Timeout is distinct
  from hide/unmount so unavailable machine metrics are not silently called healthy.
- Forty-six focused App/API/polling tests passed, including integrated hidden
  startup/visible return for all three endpoints and signal propagation. Full
  web regression passed: 119 files, 976 tests. Production build passed with the
  existing terminal chunk-size warning. No runtime source changed during that run.
- Read-only SSH `vmstat 1 4` on bgsdev around 2026-09-04 02:28 UTC showed 99%,
  98%, and 79% idle in the three interval samples, zero I/O wait, and zero swap
  in/out. The first line is a since-boot aggregate, not an interval observation.
  This short sample rules out saturation only during those seconds; it does not
  establish the cause of the operator's intermittent sluggishness or prove a fix.
- Remaining health-version polling, shared resource-sample ownership, live
  profiling, and before/after CPU/latency acceptance remain open. No push,
  deployment, release, service change, or worker interruption.

### P3f: finish health polling ownership and diagnostic return recovery

- Health/version rechecks now share visible-page ownership with no duplicate
  initial health request. Returning from hidden state still refreshes immediately.
- Diagnostics replaces its parallel polling implementation with the same helper.
  A quick hide/show waits for cancellation then refreshes immediately; abandoned
  results cannot overwrite the view. Repeated Refresh now clicks join the current
  request instead of tearing down ownership and launching another request group.
- Sixty-four focused App/Settings/Diagnostics/polling tests passed, including
  rapid return, repeated manual refresh, timeout recovery, and deferred initial
  reads. Production web build passed; terminal chunk warning remains.
- Remaining one-shot saved-report/download ownership, shared resource snapshots,
  control-feed refresh cost and measured live performance acceptance remain open.
  No push, deployment, release, or worker interruption.

### P3g: avoid unrelated snapshots and cancel superseded feed refreshes

- Once a control-room snapshot exists, presence/notification-only pages retain
  it and refresh their own auxiliary state instead of rereading seven core
  endpoints (eight for an Apiary Hive). Startup, reset, mixed task/worker/session/
  runtime events and unknown event kinds retain the full-refresh path. Bounded
  recent-event evidence is still updated. No task or decision notification delay.
- Feed invalidation carries the poll's owned cancellation through snapshot and
  auxiliary reads. Reconnect cancels the previous run; its late completion cannot
  publish Connected or replace current App state. Cursor reset now refreshes
  auxiliary settings too. The existing feed deadline also bounds these reads.
- Fifty-eight focused model/feed/App/API tests and the production web build
  passed, including snapshot preservation, reset, cancellation propagation to all
  eight endpoints and late invalidation after reconnect. Terminal chunk warning
  remains. Earlier tests expected Connected after explicitly stopping the feed;
  corrected those expectations to forbid that stale status publication.
- No measured CPU improvement is claimed. High-volume worker-event coalescing,
  live profiles, field-level snapshot cost and long-session acceptance remain
  open. No push, deployment, release, or worker interruption.

### Verification environment update

- Remote Linux reached read-only using SSH with forwarding disabled. The host has
  eight CPUs and the pinned Rust 1.97.1 toolchain, but root disk is 94% full
  (about 3.8 GiB free). No large Rust build was started and no files were cleaned.
- Remote development revision was `421eca9`; it is separate from this branch.
- Established isolated local Rust 1.97.1 GNU with rustfmt/Clippy/LLVM helpers in
  `%TEMP%/swarm-rust-5c449ae8c47144dc815fce6fa5fe3c9a`; no system PATH, service,
  or existing Rust profile changes. Official rustup installer SHA-256:
  `6D5B5709ADDC0122C916D8C810DA8D8A7B086A5D64FA805EF404D506392AADC8`.
- Public dependency fetching needed process-local `CARGO_HTTP_CHECK_REVOKE=false`
  because Windows revocation lookup failed. TLS certificate/hostname validation
  remained enabled; final tests and lint ran offline. No private source upload.
- Local test environment uses temporary `RUSTUP_HOME`, `CARGO_HOME`, and
  `CARGO_TARGET_DIR`, adds only the temporary cargo bin to the process PATH, and
  sets `RUSTFLAGS=-C link-self-contained=yes -C dlltool=<toolchain>/lib/rustlib/x86_64-pc-windows-gnu/bin/llvm-dlltool.exe`.
  The DLL helper is a copy of the official LLVM archive tool under its supported
  dlltool-mode name. Minimal GNU's original dlltool required a missing assembler;
  LLVM mode plus bundled self-contained libraries resolved that local gap.
  Commands: `cargo test --offline --locked -j 2 -p swarm-domain`,
  `cargo fmt -p swarm-domain -- --check`, and
  `cargo clippy --offline --locked -j 2 -p swarm-domain --all-targets --all-features -- -D warnings`.
- Linux engine/PTY and persistence verification are not established by this
  domain-only Windows route; isolated Linux source transfer is still unapproved.
- Linux cross-compilation uses the installed Rust Linux target plus official
  Zig 0.16.0 under the same temporary root. Archive SHA-256:
  `68659eb5f1e4eb1437a722f1dd889c5a322c9954607f5edcf337bc3684a75a7e`.
  Compiler wrappers translate only the Rust target argument to Zig's target spelling.
  Build artifacts and Zig caches stay in the temporary root. No source upload.
- API cross-checks use checksum-verified Debian OpenSSL development headers and
  static archives under the same temporary root, with no system installation:
  `libssl-dev_3.5.7-1~deb13u2_amd64.deb`, SHA-256
  `f245b646a4fe5beb31d6a387da9db3399e3b14a904efef1a23227ff5465a1d11`.
  Provenance: [Debian package download metadata](https://packages.debian.org/trixie/amd64/libssl-dev/download).
- Native persistence tests compile bundled SQLite through the isolated Zig
  wrapper. `CFLAGS_x86_64_pc_windows_gnu=-fno-sanitize=undefined` matches the normal
  C dependency build; Zig's default sanitizer references otherwise caused the
  GNU linker to fail. This is not a sanitizer test run. No Rust checking or
  production security feature was disabled.

See the approved plan. No phase is complete solely because a patch was committed.
Real Android/iOS and normal operator soak remain separate evidence requirements.
