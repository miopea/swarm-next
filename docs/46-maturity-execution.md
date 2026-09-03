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

### Verification environment update

- Remote Linux reached read-only using SSH with forwarding disabled. The host has
  eight CPUs and the pinned Rust 1.97.1 toolchain, but root disk is 94% full
  (about 3.8 GiB free). No large Rust build was started and no files were cleaned.
- Remote development revision was `421eca9`; it is separate from this branch.

See the approved plan. No phase is complete solely because a patch was committed.
Real Android/iOS and normal operator soak remain separate evidence requirements.
