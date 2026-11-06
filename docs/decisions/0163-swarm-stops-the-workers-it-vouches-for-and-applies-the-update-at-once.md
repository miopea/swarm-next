# 0163 — Swarm stops the workers it vouches for and applies the update at once

- Status: Accepted
- Date: 2026-11-06
- Decided by: the operator, 2026-11-06. After ADR 0162 merged, the swap onto it
  still went through the older engine, one worker a minute. Swarm restarted
  three workers in turn, then a fourth held the update for unsent input on a
  prompt with nothing typed, and the operator pressed the button: "why would
  we restart workers to check their state when the engine restart restarts
  them anyway? automatic engine updates are useless if we are too paranoid to
  restart them. … It should check ALL workers states each time. There are
  enough system processes that generate noise that taking 5-10 minutes to
  figure out stage means there will never been idle."
- Amends: ADR 0161 (Swarm restarts a resting worker the engine cannot vouch
  for) and ADR 0162 (the engine names every session it refuses, and Swarm
  settles them at once).

## Context

The engine's admission refuses a session for two kinds of reason. Some are
about what is on the screen or who is at it: a question waiting for the
operator, the operator holding the terminal, a screen that is not at rest.
Others are about what the engine counted in that session: a turn it saw start
and not end, a count it lost, hooks that never reported, a failed turn, a
session cron or background work the last turn report named, and any write to
the terminal since the last prompt.

ADR 0161 and 0162 met the second kind by restarting each such worker, so that
a new session's clean count would satisfy the next admission. The swap then
restarted every worker again. On 2026-11-06 that cost a minute per worker.
It also restarted workers when something else held the update anyway.

The engine counts every write as unsent input, and a terminal sends writes
nobody typed: scrolling, Esc, arrow keys and the terminal's own replies to
Claude's queries. Swarm already reads the prompt itself for its deliveries,
grey suggestions included.

## Decision

1. **Swarm vouches only for what the engine counted.** Those reasons are a
   turn in progress, a lost count, missing startup evidence, a failed turn,
   a scheduled job, background work and unsent input. For each one, Swarm's
   own reading must say the worker rests with nothing in the background, the
   operator must not be using it, and it must not have been stopped already
   for this update. A question waiting for the operator, the operator holding
   the terminal, a screen not at rest, a keystroke in flight, and a non-Claude
   session are never overruled.

2. **All or nothing, on one pass.** When every session the engine names is one
   Swarm vouches for, Swarm looks at each screen once more. That look must find
   it resting with nothing typed at the prompt. Swarm then stops those sessions
   itself, with the stop Sleep uses, and asks the engine for the rest straight
   away. If the engine stops the rest, the update applies on that pass, and the
   swap brings every worker back once. When even one session holds the update
   for a reason Swarm does not overrule, Swarm stops nobody.

3. **Nothing lands in between.** Swarm holds its delivery lock from that last
   look until the engine answers for the rest. It stops a worker only when a
   start would be admitted now.

4. **When the update cannot apply after all, the stopped workers come back
   now.** If the engine refuses the rest, Swarm cancels the drain and starts
   each worker it stopped on its own conversation. Such a worker is not
   stopped again for this update, so a race cannot cycle it. A partial stop or
   a lost answer is handled as before, counting Swarm's stops among the
   stopped.

5. **The engine is asked every 15 seconds at most, down from 60.** Workers
   rest together only in the gaps between deliveries.

6. **The card lists only what holds the update.** A worker Swarm vouches for
   is not listed. For each other worker the engine named, the card gives its
   reason and why that reason holds the update. If every named worker is one
   Swarm vouches for, the card says the update applies within a minute. A
   worker with text at its prompt is shown as such, not as the engine's count.

7. **Each worker Swarm stopped is told once it rests again.** The message says
   any scheduled jobs it had are gone. The engine refuses a session that has
   one, so these are the workers that had them.

## Consequences

- No worker is restarted to be checked. When every worker rests, the update
  applies on one pass, however many the engine could not vouch for.
- No engine change is needed. This applies on the engine that ADR 0162
  installed, which names every session it refuses. An engine that names one
  session still gets one per pass.
- Swarm's reading now decides whether some workers are stopped. A worker
  that starts a turn between Swarm's last look and its stop can be stopped
  mid-turn. The delivery lock rules out Swarm's own messages. A session cron
  could still fire in that gap, and the engine's own admission for the rest
  is as strict as before.
- A worker whose prompt holds text still holds the update until that text is
  sent or cleared.
- This change does not touch the engine, so a reload leaves no engine update
  waiting. The first live run is the next engine change.

## Verification

- `settle_step` overrules each reason in decision 1 when Swarm reads the
  worker resting, and none of the others. It never overrules a worker the
  operator is using, one not resting, or one already stopped for this update.
  The second look refuses text at the prompt and a screen not at rest.
- End to end against the fake engine, with the operator's own case, a
  keystroke on an empty prompt:
  - Nothing is stopped while a delivery is in flight.
  - On the next pass Swarm stops that worker itself, and the engine stops the
    other.
  - The update applies on that pass, and nobody is started before the swap.
  - With the delivery lock removed, the test fails.
- Nobody is stopped and no screen is read when another worker waits for the
  operator. Nobody is stopped when the second look finds text typed. With the
  composer check removed, the test fails.
- When the engine refuses the rest, the stopped worker is back at once on its
  own conversation, nothing is owed, and it is not stopped again. With the
  bring-back removed, the test fails.
- The card lists only holders, says the update applies within a minute when
  Swarm vouches for every named worker, and words the new reasons.
