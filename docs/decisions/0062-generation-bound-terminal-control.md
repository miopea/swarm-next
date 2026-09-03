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

## Verification

Domain tests cover passive reads, competing views, compare-and-swap takeover,
stale input/resize/renew/release, reconnect, expiry, generation exhaustion, and
discarded failed proposals. Engine tests must additionally prove serialization
with real PTY effects and failure rollback. Protocol tests must exercise API
replacement and old-engine refusal. Android/iOS and desktop acceptance must prove
a single stable cutover with no worker restart or stale input. Unit tests alone
do not close TERM-01.
