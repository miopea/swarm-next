# Maturity execution ledger

Approved program: [scope and acceptance](45-daily-driver-maturity-plan.md).
Branch: `codex/daily-driver-maturity`. Starting revision: `36420b3`.
Local commits authorized; no push, deployment, releases, or live worker interruption.

## P0 — Reconciliation and baseline (in progress)

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

## P3–P7 — Pending

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
