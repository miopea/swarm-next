# 0162 — The engine names every session it refuses, and Swarm settles them at once

- Status: Accepted
- Date: 2026-11-06
- Decided by: the operator, 2026-11-06. ADR 0161 worked on a live Hive, but
  one worker at a time. The operator pressed the button after seven minutes
  ("I wanted to go to bed, so I got tired of trying to wait"). Offered the
  choices, they chose this one: the engine names every holder, and Swarm
  restarts them together with no minute's wait.
- Amends: ADR 0161 (Swarm restarts a resting worker the engine cannot vouch
  for) and ADR 0156 (a waiting engine update applies itself while every worker
  rests).
- Amended by: ADR 0163. Decision 2's restarts are replaced. Swarm stops the
  sessions it vouches for and asks the engine for the rest on the same pass,
  or stops nobody. The card text in decision 3 is replaced.

## Context

ADR 0161's first live run, with every worker resting, did what it says. Swarm
restarted two workers alone, one held up by a session cron and one by a lost
turn count. It still took about two minutes per worker, for two reasons:

- The engine stops at the first session it refuses. Each admission named one
  worker, and the next could only be seen after that one was settled.
- Swarm planned each restart a minute out and confirmed it with a second
  refusal.

Four workers were named in turn over seven minutes. The fourth was due 28
seconds after the operator pressed the button.

The engine also logged nothing when it lost a session's count, and Swarm
reported a lost count and missing startup evidence under one reason. So
nobody could tell why so many resting workers needed a restart.

## Decision

1. **Admission asks every session.** When it refuses, the answer still names
   the first refusal as before, and lists every other refusing session under
   `others`. It stops nobody unless nobody refuses. The field is additive: an
   older API ignores it, and an older engine's answer reads as listing none.
   The protocol version does not change.

2. **Swarm settles every named session on that same answer.** It restarts
   each one that a new session clears (ADR 0161) and that its own reading
   shows resting with nothing in the background, unless the operator is using
   it or it was already restarted for this update. There is no minute's wait.
   The restarts are one after another, under the delivery lock and the
   lifecycle lock as before, and the engine is asked again at the next
   admission. A restart that cannot be tried now, because a delivery holds
   the lock or a start is not admitted, is tried at the next admission.

3. **The card lists every holder, with what Swarm does or why it leaves it.**
   For each one Swarm says it restarts it at the next check, that the operator
   is using it, that Swarm does not read it as resting either, or that it was
   already restarted once and the button is the way on. A holder the roster
   already shows working is listed once. Workers restarted at the same moment
   are named in one note.

4. **The two causes of an uncounted session are told apart.** A lost count
   stays `turns_unaccounted`. A session whose startup report the engine never
   received is `no_startup_evidence`. A new session clears both.

5. **The engine logs the report that lost a count, once.** The log line
   carries the session, which unaccounted case it was, the prompt id
   reported, and the ledger's open, last closed and queued values. It never
   carries what a prompt said.

## Consequences

- A waiting update applies about a minute after every worker rests, however
  many workers it must restart first.
- The swap onto this engine still goes through the engine already
  installed, which names one session per admission. Without the minute's
  wait, that is about a minute per worker. Later swaps get every holder at
  once.
- Without the second refusal, a session whose turn report is a moment late
  can be restarted when it did not need to be. The cost is a cold start of a
  worker that was resting anyway. Swarm still restarts nobody while the
  engine counts any session busy.
- Several workers restart at once, so the Hive pays for several cold starts
  together.
- Why counts are lost is still unproven. On 2026-11-06 Queen's case fitted a
  fresh prompt arriving while the ledger still counted a queued message. One
  explanation would be that a message typed mid-turn is folded into the
  running turn at a tool boundary, so its own turn and `Stop` never come.
  That was not measured, and the ledger is unchanged. The new log line is
  what will show which case fires.

## Verification

- The engine's admission: a three-session set where the first and third
  refuse is answered with both and stops nobody. Every session is asked. The
  answer reads back without `others` from an older engine.
- The ledger names the case that lost its count, once, for each of the four
  cases, and an ordinary report names none.
- End to end against the fake engine: one answer names both workers, nothing
  is restarted while a delivery is in flight, and then both are restarted on
  the same pass. Each is stopped with a retained stop, ended with
  `engine_update_restart` and started again on its own conversation with its
  return settled. The card shows both as restarted and nothing still holding
  the update. With the delivery lock removed, the test fails.
- `settle_step` is tested for every reason it restarts and every reason it
  must not, for a worker in use, one not resting and one already restarted.
- The card's holder lines, the grouped note and the new reason's sentence
  are tested. The harness was looked at at 390, 768 and 1280 px.
