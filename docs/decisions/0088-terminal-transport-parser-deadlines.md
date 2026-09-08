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
