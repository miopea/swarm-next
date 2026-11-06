# 0161 — Swarm restarts a resting worker the engine cannot vouch for

- Status: Accepted
- Date: 2026-11-06
- Decided by: the operator, 2026-11-06. Shown a waiting update that would
  not apply with every worker paused, they asked for it to be made stable
  ("otherwise the engine will NEVER update on its own"), said the card should
  make "clear what is happening and if not why", and declined a Hive-wide
  hold on new work: "if the behaviour works we don't need to make it quiet.
  most are not running a dozen workers, they only have 3-4."
- Amends: ADR 0156 (a waiting engine update applies itself while every worker
  rests).
- Amended by: ADR 0162. The engine now names every session it refuses, and
  Swarm restarts every qualifying one on that same answer, with no minute's
  wait. Decision 2's plan-then-confirm and the card text in decision 8 are
  replaced.

## Context

ADR 0156 asks the engine to stop the whole running set only if every session
in it rests by the engine's own evidence. On 2026-11-06 the operator paused
every worker so a waiting update could apply. It did not, for over an hour,
while the card's own roster said no worker was working. The engine refused
one session at a time, and each refusal was about its own turn count, not
about work:

- Queen's count went unknown. ADR 0156 makes that permanent until the
  session ends. A `/restart` inside it changes nothing, because the engine
  session is the same one. It cleared only when her session ended.
- The operator had paused another worker with Esc. Claude sends no report
  for Esc, so the engine held that turn open until the worker's next prompt.
- A third worker held a session cron. Claude's own description of its
  scheduler says a job is gone when Claude exits, so the swap would end it
  anyway.

The card named one holder at a time, said "(for 1 minute)" about how long it
had named that one, and repeated the roster's census in a second sentence.

## Decision

1. **The engine still decides the swap.** Admission is unchanged: the whole
   running set, its own evidence, refused whole or stopped whole.

2. **A refusal a new session clears is settled by giving that worker one.**
   When admission refuses one session because the engine counts a turn in
   progress, lost its count, never saw its hooks, saw its last turn fail, or
   holds a scheduled job, and Swarm's own reading of that session's screen
   says it rests with nothing running in the background, Swarm plans to
   restart that worker and says when on the card. If the next admission, a
   minute later, refuses the same session and the screen still says resting,
   Swarm restarts that worker alone and the engine is asked again.

3. **The restart is Sleep's stop and a return's start.** Swarm records a
   return for the worker, stops its session keeping its context, ends the
   session with the reason `engine_update_restart`, and starts it on the same
   conversation. A start that fails is a return owed like any other, shown in
   Needs you.

4. **Never for anything else.** Background work, unsent input, a question or
   permission prompt, the operator holding or engaged with the terminal, a
   provider other than Claude, or a screen that does not read resting: the
   engine's answer stands and the card says why. Admission is asked only when
   the engine's busy count is zero, so nobody is restarted while another
   worker works.

5. **Once per worker per waiting update.** If the engine still refuses a
   worker Swarm already restarted for this update, the card says so and the
   button is the way on. The bound is kept in memory with the planned
   restart; an API restart forgets both and may restart that worker once more.

6. **Nothing Swarm sends lands in between.** The restart holds the delivery
   lock, which every delivery takes, and gives up for this minute if a
   delivery holds it.

7. **A restarted worker is told.** Once no update waits, each worker Swarm
   restarted is told, when it next rests, what happened and that any
   scheduled jobs it had set are gone, to set again those it still needs.
   Swarm cannot list a session's jobs, so it says this to every worker it
   restarted. Like ADR 0122's carry-on message, this is held in memory and an
   API restart forgets it.

8. **The card gives one answer.** While an update waits and the switch is on,
   the card says what happens and lists each worker holding it, from the
   roster and the engine both, with what Swarm will do: for example "The
   engine counts Ledger as mid-turn. Its screen shows it resting, so Swarm
   restarts Ledger alone at 09:17 if it still is." The roster's census
   sentence is shown in the card's body only when the switch is off, where it
   is the button's cost; the confirmation still shows it.

No Hive-wide hold: the operator ruled a natural quiet moment enough for the
three or four workers most Hives run.

## Consequences

- An update that waited on a lost count, an interrupted turn or a cron now
  applies within a few minutes of every worker resting, with the 1.18.0
  engine as installed. Nothing here changes the engine or its protocol.
- A restarted worker resumes its conversation on a new session, which costs
  it a cold start.
- The engine stops at its first refusal for a session. A turn-in-progress
  refusal can hide typed text the engine would also have refused for. Swarm
  does not restart a worker the operator is engaged with, which covers text
  being typed; text typed and abandoned past the engagement's expiry would be
  lost.
- Subagents and other background kinds Claude lists only in its turn report
  are not on the screen, so an interrupted turn that left one running cannot
  be seen. That case is not handled.

## Verification

- `settle_step` is tested for every reason it acts on and every reason it
  must not, for a screen that does not rest, for an engaged worker, for the
  minute between plan and restart, and for the once-per-update bound.
- An end-to-end test against a fake engine: a disputed refusal is planned and
  shown, a working screen drops the plan, a held delivery lock prevents the
  restart, and then only that worker is stopped with a retained stop,
  released, ended with its reason and started again on its own conversation
  with its return settled. With the lock removed the test fails.
- The card's status is tested for the roster's holders, the engine's holder
  with a planned restart, a worker already restarted, a reason that is
  nobody's, and the switch off. Looked at in the harness at 390, 768 and
  1280 px.
