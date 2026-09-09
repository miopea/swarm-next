# ADR 0062: Generation-bound terminal control in the worker engine

Status: Accepted for the operator-approved daily-driver maturity program;
implementation in progress. Domain and engine gates and protocol-11 engine
commands, v4 API adapter, and browser Resume Here integration are implemented
locally. Real-device and rolling-update acceptance remain pending; not deployed.

## Context

The approved TERM-01 contract replaces implicit keystroke takeover with one
interactive owner. Resume Here must move input and geometry together. An API
database check followed by a separate engine write is not sufficient: another
attachment can take over between the check and the PTY operation. Same-device
popouts also share a presence identity but must not share write authority.

## Decision

- Each engine session owns one bounded control state, including device identity,
  a distinct browser-view identity, a monotonically increasing generation, and
  a lease deadline measured using the engine's monotonic clock. Browser time is
  never authoritative. Ownership dies with that immutable engine session, not
  with the API process or socket.
- Foreground automatic resume acquires only an unowned/expired session, or
  renews the same view. Other views remain passive. Explicit Resume Here uses
  the observed generation to compare-and-swap ownership; an old delayed claim
  cannot undo a more recent takeover. Same-device windows are distinct views.
- Input and resize carry the accepted generation. The engine serializes their
  authorization checks and PTY effects with takeover. No adapter may check a
  grant, release its guard, and later write as if the check were still current.
- Prepare takeover on a copy, apply validated geometry while holding that same
  session guard, then commit ownership and acknowledge it. Failed geometry must
  not report a completed handoff. A failed/uncertain input is never replayed.
- Retain the existing 90-second viewing and 300-second typing protection;
  foreground presence may renew the shorter lease without shortening typing
  protection. Hidden views stop renewals and resize requests. Disconnect itself
  does not revoke ownership; reconnect of the same view retains its generation
  while the lease is valid. Expiry is checked at operations, not by a timer.
- Expired reacquisition and explicit release invalidate old generations. Counter
  or deadline overflow fails closed without modifying the current grant.
- Engagement remains the orchestration interruption guard, not terminal control
  authority. The API must synchronize its engagement projection without becoming
  a second authority for PTY ownership. Presence is not authorization.

This supersedes the input-implicitly-steals geometry rules in ADRs 0012/0045 and
the claim-does-not-move-geometry decision in ADR 0049 when the new protocol is
enabled. It does not change provider sessions, task ownership, or scope of work.

## Rolling compatibility and activation gate

Operator clarification on September 7: an unfocused, hidden, minimized or
detached view is not engagement. It releases its own generation immediately,
retaining canonical geometry and the provider session. A delayed claim/renew
response received while inactive is also released. This supersedes retaining
ownership on blur/visibility loss above; transport loss alone still relies on
bounded engine expiry. Return uses non-displacing acquisition or Resume Here.
General presence also requires focus before reporting active desktop return.

The terminal adapter and engine must negotiate support explicitly. Never silently
downgrade a generation-bound attachment to unrestricted legacy input. Existing
workers on an older engine must remain alive; expose that engine capability gap
and its update requirement rather than pretending stable takeover is available.
The terminal protocol owner removes the old control path after supported engines
and clients have migrated. The browser now requests v4 explicitly and refuses
legacy grants. A v4 attachment to an older engine remains visibly read-only.
Each retained controller has a unique view id that survives socket reconnects.
Resume Here claims input and measured geometry in one generation-bound command.
Hidden/unfocused views cannot write or resize and stop foreground renewal;
explicit worker navigation releases ownership without ending the worker.
Visibility return probes transport, then requests a non-displacing engine claim
before allowing input. Probe replies alone cannot authorize input. A renderer
rechecks ownership after asynchronous fit waits.

Once a session has accepted generation-bound control, legacy operator writes and
resizes remain refused even after release or expiry. Authorized coordination may
proceed without an active owner, under the same engine guard; it cannot bypass a
live interactive owner. Before activation, the existing legacy contract remains.

An ownership refusal of a coordination payload has the typed host error
`coordination_control_held`. This is definitive evidence that this write did not
occur, not an exhausted delivery attempt. The API leaves a first-payload refusal
queued without consuming its retry allowance or creating an operator question.
Ownership acquired after a payload was accepted still leaves submission uncertain;
the API must not replay that payload. Generic host failures and legacy operator
generation errors retain their existing failure semantics. This preserves the
engine gate rather than weakening it to make orchestration move.

Protocol 11 adds a typed `Control` command and a `WaitControlled` output request.
Control cursors include generation and occupied state, so lease expiry is not
mistaken for an unchanged live owner. Waiters subscribe before observing state;
claims/releases notify them even without output or a geometry change. They may
wake at the authoritative expiry deadline and recheck renewals, not poll guesses.
Wire status exposes remaining lease duration rather than the private engine clock.
Nested command variants are pinned alongside the top-level protocol surface.

WebSocket v4 is explicitly requested at grant issuance. Grants are bound to the
selected protocol as well as session and expiry; a controlled grant cannot be
consumed as v3. Engine support is checked again at attachment. Older or unknown
engines receive an explicit read-only v4 surface, never legacy input/resize.
The handshake binds device and unique view identity. Subsequent commands cannot
replace it, and generations use decimal strings to preserve the full u64 range.

Schema 124 projects engine observations into one row per worker. It retains a
generation watermark across release/expiry, refuses older observations, and only
accepts observations for the active immutable session. Same-generation expiry
cannot be undone by a delayed live-owner reply. Legacy engagement writes cannot
replace or delete an activated projection. This is an activity indicator and
additional coordination guard, never permission to write to the PTY. Repeated
typing observations are coalesced; handoff/expiry changes are not. Projection
failure after a successful PTY operation must not claim the input was unsent.

Combined schema 136 repairs an integration collision discovered in live dogfood:
upstream Ops tickets and the maturity branch had both published schema 124. A
database that received the upstream step could therefore advance through 135
while lacking `worker_terminal_control`. The final, idempotent repair creates the
projection when missing and advances the version only after the Ops migration.
The repair preserves workers, sessions, tasks, and Ops tickets; rollback requires
the verified pre-update database backup.

## Verification

### September 9 attachment-bound browser sizing

A measured fit belongs to one controller attachment revision. Detaching or
reattaching invalidates its result even if that same controller is attached and
owns geometry again when the promise completes. A pending initial fit cannot
start transport using an abandoned container's size; a pending follow-up fit
cannot publish it. Completion schedules one fresh measurement only when an
actual attachment change occurred and a current view remains. Failed old fits
follow the same rule; disposal or staying detached schedules nothing. There is
no timing retry, new connection, provider restart or authority change.

The regression failed before the fence: a remount reused the old pending fit
without measuring its new container. Coverage includes successful/failed stale
fits, rapid multiple remounts before initial connection, detached/disposed
completion and existing snapshot/control guards. All 105 focused tests and the
full 1,435-test browser suite passed, as did TypeScript and production build.
The separate no-proxy Edge fixture visited fifteen fictional workers and returned
between retained views: fifteen retained, one attached, fourteen inactive,
readable real-WebGL terminal and no restore cover at inspection. One automation
batch timed out at worker eleven; inspection established its position and the
remaining workers were completed without restarting the fixture. This is not a
real-provider latency benchmark or blanket desktop reload-jumping acceptance.

### September 8 canonical-resize feedback correction

The snapshot parser also owns its local grid until its write callback completes.
An observer fit during parsing records one deferred fit; an asynchronous measured
fit waits for the same completion before sampling again. Only snapshot completion
removes the restoring cover. Disposal cancels waits and schedules no deferred fit.
This is a browser rendering fence, not new ownership or delay-based readiness.

A delayed-parser regression failed before correction: the observer applied 120x40
while canonical 80x24 bytes were still pending. It now waits for completion, then
fits the current viewport. A second test covers the asynchronous fit path. All 52
surface tests pass. Live desktop reload-jumping acceptance remains open: these
tests prove a race, not that every reported jump has this cause.

Restoring a server snapshot changes xterm's grid but is not a new viewport
measurement. The controller must not forward restore-origin resize notifications
to the connection: doing so overwrites its measured target and can send an older
size back while a newer resize is in flight. Initial measured attachment sizing,
real viewport changes and owned post-snapshot refits retain their existing paths.

The surface separately scopes the synchronous resize cause and the coalesced
queued publication cause. A same-size restore emits no xterm resize event, so its
restore marker must be cleared even then; otherwise the next genuine viewport
event is mislabeled. Queued events retain the cause of their latest actual size
change. No generation, ownership, stable-frame or oscillation guard is relaxed.

Both regressions failed before correction: old canonical sizes were reissued as
resize requests, and a same-size restore mislabeled the next viewport resize.
All 147 focused terminal tests and TypeScript checking pass after correction.
This addresses a demonstrated feedback path; the operator's desktop reload
jumping still requires live verification and must not be declared fixed from
unit tests alone. This is a post-1.6.0 change, not a modification of its frozen tag.

Post-snapshot sizing is one controller-owned, coalesced asynchronous follow-up.
Once canonical bytes have applied, queued output and the connection's applied
cursor do not wait for font/layout frames a second time. The existing stable-fit
and oscillation checks still run; completion rechecks attachment, visibility,
focus, mobile-composer hold and geometry ownership before publishing a resize.
Failed follow-up sizing leaves the canonical screen usable and permits a later
fit. Local evidence retains the follow-up duration rather than hiding it from
timings; at most one diagnostic continuation is attached to a pending fit.
This changes neither engine authorization nor the meaning of a granted takeover.

Initial browser attachment may read usable terminal-cell and container metrics
after font readiness without waiting for scheduled animation frames. This is a
non-mutating measurement, not a control grant or local screen reflow. When those
metrics are unavailable, the bounded stable-frame fallback remains. The measured
size still precedes the v4 handshake; the engine alone grants control and supplies
canonical screen geometry. Later refits retain stable-frame and oscillation
guards. No guessed dimensions, protocol downgrade or unmeasured claim is added.

Domain tests cover passive reads, competing views, compare-and-swap takeover,
stale input/resize/renew/release, reconnect, expiry, generation exhaustion, and
discarded failed proposals. Engine tests must additionally prove serialization
with real PTY effects and failure rollback. Protocol tests must exercise API
replacement and old-engine refusal. Android/iOS and desktop acceptance must prove
a single stable cutover with no worker restart or stale input. Unit tests alone
do not close TERM-01.
