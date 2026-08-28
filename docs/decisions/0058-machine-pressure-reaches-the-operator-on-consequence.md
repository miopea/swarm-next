# ADR 0058: Machine pressure reaches the operator on consequence, not on a threshold

## Status

Accepted for dogfooding. Answers the question
[ADR 0040](0040-resource-pressure-admission-for-automatic-starts.md) left open
and that task 01a04982 was filed to settle: whether pressure should reach the
operator through Needs you or push, beyond the header badge added in `101f929`.

## Context

The operator's framing was "we don't want to crash a machine by not informing an
operator". A header badge only informs someone looking at that screen. Someone
away, on another tab, or asleep is not informed by it, so the header alone does
not answer what was asked.

Three candidate channels were weighed.

**Needs you** is a queue of things needing a DECISION, with a badge count and the
operator's own twelve-hour escalation ([01a0418f](../../README.md)). Machine
pressure needs no answer. Putting a gauge reading in a decision queue gives the
operator something they cannot resolve, which is how a queue stops being a queue.

**Push** reaches someone who is not looking, which is exactly the point, and is
the channel most easily made worthless. The existing notification model is
decision-shaped — dispatches carry a decision id, an urgency and a kind — so
pressure would have to be forced through it. Worse, a push driven by a threshold
fires whenever the reading crosses the line, including every time the machine is
merely busy and nothing is actually at risk. The acceptance bar for this work was
that a channel must be shown NOT to fire when nothing is wrong, and a
gauge-triggered push cannot clear that bar by construction.

**Doing nothing** was explicitly allowed if the header were judged sufficient.
It is not sufficient, for the reason above.

## Decision

**Pressure reaches the operator a second time when it has actually blocked work,
through the refusal that blocking already produces — and not on any threshold.**

Admission already refuses automatic worker starts under pressure (ADR 0040) and
already records that refusal, which already becomes a held-delivery card in Needs
you and already escalates after twelve hours. That path was complete except for
one thing: the refusal said the Hive "is not currently admitted to start it",
which names the mechanism and not the cause. `CoordinatorStartAdmission` now
carries `refusal_reason()`, and the refusal names the machine's state in the
operator's terms.

**No push, and no threshold-driven entry into Needs you.**

## Consequences

The property that makes this safe is structural rather than tuned. A refusal
exists only when a start was OWED and denied, so an admitted Hive records nothing
however much work is queued — there is no quiet-period threshold to get wrong,
and no soak test needed to believe it. `an_admitted_hive_records_no_pressure_refusal_however_much_is_waiting`
pins it, and fails if `permits_start` is broken.

It also fires on the thing the operator actually cares about. A machine at 88%
with nothing waiting has cost them nothing; a machine that has stopped work
starting has. The header still carries the earlier, quieter signal for anyone
looking.

The gap this accepts: an operator who is asleep, with work blocked, learns
nothing until they next open Swarm or the twelve-hour escalation fires. That is a
real limitation and it is the reason to revisit this — **if** a real episode
shows the escalation was too slow. It should not be revisited by adding a
threshold push, which would trade a known gap for an alarm nobody reads.

Whatever reads pressure must read the same source of truth the client-principal
exclusion applies to (spec `docs/39-connecting-an-outside-tool.md`): a connected
outside tool has no PTY and no engine, and counting one as load would misreport
the machine. This decision inherits that requirement rather than restating it,
because it reads admission rather than counting workers itself.
