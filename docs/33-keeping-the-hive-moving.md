# Keeping the Hive moving

Status: **Scoped 2026-08-22.** Trapdoors first. The drone surface is parked;
the measurement half is not.

## What prompted this

The operator, after a week of use: tasks sit idle, almost nothing appears in
Needs you, and the deterministic coordinator is invisible so there is no way to
tell whether it is working. The worry was that the coordination design is wrong
— that Queen is not earning her place, the way she failed in Legacy when it
became more efficient to talk to workers directly.

Measured before answering, and the measurement changed the question.

| Fact | Value |
| --- | --- |
| Queen automation runs, all time | 1, state `queued` |
| That run requested at | 2026-08-22 01:49 |
| `attempts` on it | 0 |
| `Queen automation is held behind an open provider question` | **1503 times in 24 hours** |
| Task briefings held the same way | 7 |
| Task outcomes held the same way | 1 |
| Decisions raised in the last two days | 0 |
| Open tasks | 22 drafts idle 1–4 days, 1 ready idle 8 days |

Queen's terminal is sitting at an unanswered provider question. Swarm correctly
refuses to type into a session with an open prompt, defers the review, and
resets the attempt counter — so the run has been queued for twelve hours with
zero attempts, and every thirty seconds it tries again and defers again.

**No Queen review means nothing reaches Needs you, nothing gets verified, and
nothing moves.** The two quiet days are not evidence about the design. They are
one wedged terminal.

This is the failure `moving-from-legacy.md` claims Swarm eliminated: "stranded
input was a real category — something waiting in a terminal nobody was looking
at." Swarm reproduced it, for Queen, which stops the whole coordination system.

## The principle

**Autonomy is not more automation. It is fewer dead ends.**

A Hive that acts more often but still has places where work stops permanently
and silently is not more autonomous — it is faster at reaching a trapdoor. Every
dead end below stops work forever and tells nobody.

## Part one: close the trapdoors

Failing closed is correct. Failing closed *silently and forever* is not.

1. **A stranded prompt is never surfaced.** A delivery held behind an open
   provider question retries every thirty seconds indefinitely. After a grace
   period this becomes a decision in Needs you: something is waiting on a prompt
   in a terminal nobody is looking at. Silent for a minute or two first, because
   a prompt answered in ten seconds should not generate an item.

2. **An `uncertain` wake is never retried.** Two are sitting in the database
   from 2026-08-21. `mark_coordinator_worker_wake_uncertain` records the wake
   "without permitting replay", which is right — a briefing delivered twice is
   worse than one you were told about — but nothing distinguishes a task parked
   behind an unreplayable wake from one that is merely queued.

3. **An `uncertain` automation run can never be closed.** The finish tool
   refuses it and the marker re-fires on a stale fingerprint.

All three are the same shape: a coordination step that fails quietly and leaves
work looking routed when it is not.

## Part two: the drone surface

The name is the operator's, over a recorded objection. In Legacy a drone was an
actor that read terminals and matched regexes; the deterministic coordinator is
a function with no identity, no session, and no model call. The atlas already
settled the substance — "the useful part of legacy drones becomes a boring,
**auditable** coordinator below Queen" — so this is not a revival. It is the
half of that decision that was never built.

The risk accepted: naming it after an actor invites the question "why didn't the
drone do it", which is unanswerable, because the answer is always "a query
returned no rows". Naming the **record** rather than an actor — drone activity,
not a drone — keeps most of the value and little of the confusion.

### Two surfaces, different jobs

**Day to day: proof of life.** What it did, when it last acted, and anything it
wanted to do and could not. Small enough to glance at.

**Troubleshooting: what it will and will not do.** Read-only, in Settings.

### The requirement that makes it work

**The coordinator must record its refusals, not only its actions.**

Today it records what it did. A feed built on that alone would have been
*completely empty for the twenty-four hours the operator was troubleshooting* —
the only evidence was a log line repeated 1503 times. A record of successes is
blank exactly when something is wrong, which is the only time anyone opens it.

So a refusal is a first-class entry: considered waking this worker and did not,
because start admission was paused; wanted to deliver to Queen and held, because
a prompt has been open since 01:49.

**And it must aggregate.** One row reading "held since 01:49 · 1503 checks", not
1503 rows. Otherwise this is the journal with a stylesheet.

### Deliberately not built

- **No knobs.** The coordinator's value is that it has no policy surface: it
  acts only when every fact and authority is present. Every dial is a new way
  for it to be wrong, with the operator's fingerprints on it. This is Legacy's
  confidence threshold in a different costume.
- **No thresholds shown.** "A build is called stalled after 20 minutes" invites
  someone to want 5, and read-only creates that desire with no outlet. Show
  reasons instead: it will not close a task without a recorded deployment; it
  will not write into a session with an unanswered prompt.
- **Not an actor.** No presence, no state, nothing to address. The moment it can
  be talked to, it is a worker, and the fleet has one more thing negotiating
  ownership.

## Measuring whether it is working

Unparked 2026-08-22. The operator's ideal is a day spent on system judgment,
design, and finalisation choices, and nothing else. Two measures, both from data
that already exists.

### Time stuck versus time moving

Per task: how long it sat unable to progress, and why. Not a score — a
diagnostic that names what to fix.

This week is the argument for it. Twenty-two drafts idle one to four days and a
ready task idle eight, none of which was visible as *stuck* rather than merely
*open*, while the actual cause was one wedged terminal. A number that said "this
task has been unable to move for eight days, held behind an unreplayable wake"
would have found it in a glance.

The refusal record built for the trapdoors is where the "why" comes from. The
two measures share a source.

### How often the offered actions were wrong

When Queen files a decision she proposes actions. The operator can press one, or
type something else. **Typing something else means the proposal was wrong**, and
that is a measure of Swarm's judgment rather than its throughput — the only one
here that gets at quality.

Measured on the existing record: of sixteen resolved decisions, twelve offered
actions and the operator chose one of them every time. Four were interview
answers or dismissals. **The historical override rate is zero.**

Two things follow. It is a real baseline, and it is currently detecting nothing,
so this only earns its place if the rate moves.

And it has a blind spot worth writing down before anyone reads it as a score: a
zero rate can mean the options were right, or that typing costs more than
accepting a near-enough option. It measures friction, not correctness. A rising
rate is meaningful; a flat zero is ambiguous.

### Deliberately not measured

No percentage-autonomous figure. It is the closest thing to the stated ideal and
the worst thing to build: a single number invites gaming, says nothing about
what to fix, and would have read as healthy all week while nothing moved.

## Order

Trapdoors first, surface second, on the operator's ruling — with the surface
built to show the trapdoors when they open. A feed that reassures you the Hive
is alive while it is wedged is worse than no feed.
