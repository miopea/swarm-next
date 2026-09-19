# ADR 0101: A provider-reported fork or compact changes the recorded conversation

Status: Accepted implementation direction for the operator-reported stale
conversation marker.

The operator finished working in two workers, put them to sleep, and Swarm
reported "Newer conversation history" against both. Their reading was right:
"the markers are not updating because I just finished working in those workers
and put them to sleep, so the conversation I was working on was the new one."

Measured before changing anything. Sculpt Studio's session ran 18:21:24Z to
21:01:22Z; its pinned conversation's last entry is 18:21:27Z, three seconds
after start, and every entry after that is in a different conversation ending
20:27:33Z. D365's session ran 01:50:14Z to 19:26:35Z and its pinned
conversation was not touched at all, while another ends 19:26:28Z, seven
seconds before the session did. Both markers pointed at a conversation the
worker had abandoned.

Swarm was told. Claude's `SessionStart` hook carries the real conversation id
and a source, and `read_claude_session_start` already projects `fork` and
`compact` onto `Forked` and `Compacted`. The lifecycle gate then discarded both
as uninteresting, because it accepted only `New` and `Resumed`. The one path
that advances a marker mid-session, `complete_resume`, requires a paired
SessionEnd(reason=resume)/SessionStart(source=resume) boundary, and a fork
produces no such pair. So the observation arrived, was parsed correctly, and
was dropped.

**A fork and a compact are conversation changes, not lifecycle noise.** Both
mint a new conversation id and both are reported over the same authenticated
startup capability. They are recorded as a SELECTION change, advancing the
revision so the existing persistence path writes the marker, and NOT as a
startup: a startup settles identity once and a retry still cannot overwrite it
with different facts. `ConversationSelection::switch` exists for a change the
provider reports rather than one Swarm ordered; it abandons a pending resume,
because the boundary that resume waited for can no longer arrive.

Bounds kept. A fork before any startup has settled establishes nothing and is
still ignored. A fork reported twice advances nothing, so a repeat cannot
inflate the revision downstream reads as movement. A fork from a wrong
capability is still denied, because accepting a new kind must not widen who may
report one. `Reset` and `Unknown` remain ignored.

This does not choose a conversation on the operator's behalf, which ADR-era
comments in `worker_runtime.rs` record them declining. It records the
conversation their worker demonstrably used. The drift report stays a report.

Acceptance: a worker that starts, forks and sleeps reports Current rather than
Stale; the saved marker equals the conversation the session ended in, so the
next start resumes the thread the work is in; duplicate and wrong-capability
reports change nothing; a fork with no settled startup changes nothing.

Not covered: a conversation change the provider never reports. Nothing here
scans transcripts to infer one, and a marker can still go stale if Claude
changes conversation without a `SessionStart`.

## Amendment, 2026-09-19: `Reset` joins them, and it was the reported case

⚠️ THE LINE ABOVE SAYING "`Reset` AND `Unknown` REMAIN IGNORED" IS SUPERSEDED FOR
`Reset`. Excluding it left this ADR fixing a defect the operator had not hit
while leaving the one they reported intact.

Both workers in the original report had been **cleared, not forked**.
`provider_lifecycle.rs:47` maps the SessionStart source `"clear"` to `Reset`, so
the fork branch never saw them. Measured: Sculpt Studio resumed its pinned
conversation, cleared three seconds in, then did 89 user and 159 assistant turns
in a conversation Swarm never recorded. The marker moved only because somebody
repointed it by hand through `PUT /api/v1/workers/{id}/conversation`.

Operator decision `01a0b8dc-3a73`, answered in session: **the marker follows the
clear immediately.**

⚠️ THE COST IS ACCEPTED, NOT OVERLOOKED. Right after a clear the new conversation
is EMPTY, so the marker briefly points at nothing and a resume taken at that
moment starts blank. That is what clearing asked for, and the previous thread
remains on disk and repointable — but nothing announces it.

Rejected: advancing only once the new conversation has content. It is the most
accurate trigger and the gate cannot see it — `SessionStart` is always empty —
so it needs something watching transcripts and writing the marker afterwards.
That is Swarm choosing a conversation on the operator's behalf, which the section
above declines to do. Rejected: leaving `Reset` ignored, which is the behaviour
that produced the report.

Bounds unchanged and re-asserted for the new arm: a clear before any startup
settles changes nothing, and a clear reported twice does not advance the revision
again. Both ablate cleanly — removing `Reset` from the branch fails exactly the
two clear tests and leaves all eight others passing, including both fork tests
and capability denial.

`Unknown` stays ignored.
