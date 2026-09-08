# ADR 0087: Local browser focus attribution

Status: Accepted implementation detail of the approved diagnostics maturity pass.

Fresh live samples showed slow presentation estimates without evidence that the
page remained focused during the measured entry. Visibility at callback time
does not establish foreground coverage across an asynchronous event interval.

Extend the existing ADR 0060/0063 browser capture owner with at most 200 observed
focus/visibility changes over one minute, retaining one preceding boundary within
that count. Native event entries receive an allowlisted interval classification:
foreground, unfocused, hidden, changed, or unknown. Foreground requires visible
and focused throughout the observed interval; callback-time focus alone is
insufficient. Missing/evicted coverage, invalid bounds or an unobserved future
endpoint are unknown. This is observed state, not proof of scheduling or paint.

Only the classification accompanying the same slowest local entry is reported.
No transition timestamps, targets, names, event IDs or input are exported. The
history uses existing focus/blur/visibility events, adds no timer or storage,
resets with the capture owner and removes listeners on teardown. Historical
hourly metrics and incident semantics remain unchanged. Samples without a
classification display unknown rather than assumed foreground.

Tests cover interval transitions, missing/evicted/expired evidence, monotonic
clock reversal, repeated identical states, matched slowest-entry context, and
listener/late-callback disposal. Live acceptance must observe classifications on
the deployed build; these changes alone do not establish a performance fix.
