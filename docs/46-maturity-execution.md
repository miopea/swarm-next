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

### P2b: interrupted picker evidence

- A five-minute, timestamp-only pending-picker marker survives page recreation;
  it reports interruption without retaining filenames or image contents.
- Native picker cancellation remains quiet and clears the marker. Picker-return
  timers now have real cleanup rather than returning an ignored cleanup function
  from a DOM event listener. Unexpected upload callback rejection is visible.
- Composer tests: 21 passed. Production build passed, then typecheck repeated
  after the timer-test assertion was corrected to inspect its owned timer rather
  than counting unrelated jsdom timers.

### Verification environment update

- Remote Linux reached read-only using SSH with forwarding disabled. The host has
  eight CPUs and the pinned Rust 1.97.1 toolchain, but root disk is 94% full
  (about 3.8 GiB free). No large Rust build was started and no files were cleaned.
- Remote development revision was `421eca9`; it is separate from this branch.

See the approved plan. No phase is complete solely because a patch was committed.
Real Android/iOS and normal operator soak remain separate evidence requirements.
