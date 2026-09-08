# ADR 0088: Hand off confirmed snapshot work to the parser deadline

Status: Accepted correction within the approved terminal reliability scope.

A regression using the real default timeouts demonstrated that the browser
closed a responding socket at three seconds while its valid first snapshot was
still applying. The existing parser deadline allowed eight seconds, but the
shorter attachment timer interrupted that stage and scheduled another attach.

After validating snapshot framing, sequence range and geometry, clear the
attachment confirmation timer before awaiting the renderer. The already-owned,
bounded parser deadline governs application completion. Do not report connected,
advance the applied cursor, reset retry attempts or permit input before that
completion. Malformed frames retain their existing failure path. A return probe
still needs its correlated response and retains its confirmation timer.

This changes neither engine ownership nor worker/session lifetime, adds no new
timer, and does not lengthen the deadline for a silent transport or attach-grant
request. A genuinely stalled renderer still requires view recovery after its
existing visible-time allowance; no worker restart or input replay is implied.

The test reproduced the unnecessary reconnect independently. The observed live
10.5-second reconnect did not retain failed-attempt details, so it cannot yet be
attributed to this particular race. Live and real-device acceptance must not be
claimed from the regression alone.

## Reconnect ownership correction

A second regression reproduced an old socket's asynchronous snapshot completing
after a replacement socket opened. It incorrectly reported that silent replacement
connected and cancelled its confirmation deadline. Queued render work now retains
its originating socket; batches never mix sockets. Applied bytes still advance the
canonical cursor, but only the current open socket can confirm its transport or
cancel its deadline. This preserves output continuity without treating old parser
completion as new network evidence.

The regression verifies refusal of input after stale completion, expiration of
the silent replacement, and successful canonical recovery on the next socket.
All 94 focused terminal tests and the TypeScript/Vite build pass. Live acceptance
of this second correction is not yet recorded here.

## Failed-attempt attribution

The September 8 live reload recovered in 4,292 ms but retained only successful
phase durations. This is insufficient to prove where its first attempt failed.
The existing 20-entry session-local client failure history now records four
allowlisted timeout categories: grant, socket opening, initial restore, and
correlated return probe. Each entry contains only its category and timestamp;
storage reads project away all unexpected fields. No new timers, polling,
terminal identifiers, text, grants, URLs, or operator alerts are introduced.
Existing timeout owners record the category only when their current attempt
actually expires; disposed or replaced attempts do not report a failure.
This improves future diagnosis and does not establish the slow reload's cause.
It is a post-1.6.0 maturity change, not part of that frozen release artifact.
