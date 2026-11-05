# 0156 — A waiting worker engine update applies itself while every worker rests

- Status: Accepted
- Date: 2026-11-04
- Decided by: the operator's ruling on whether a waiting worker engine update
  should apply by itself: "keep it automatic, but say so when it happens".
  Queen's go added the conditions recorded below.
- Amends: ADR 0085. Its all-or-none admission core is now enabled for Claude
  sessions.
- Builds on: ADR 0123 (Claude workers report their turns through hooks) and
  ADR 0077 (durable returns).

## Context

A worker engine update never applied on its own.

- The package layer's timer replaces the engine only when no worker is loaded.
  Autostart keeps workers loaded, so the timer always defers to the worker
  engine card.
- The card stops every running session, busy or not, so the operator has to
  choose a moment.
- The capability inventory said engine updates already applied when idle.
  They did not.

ADR 0085 built the engine's half. The engine freezes the running set, takes
every session's input and control guards, checks every session, and stops all
of them or none. Every provider still refused with
`ProviderEvidenceUnavailable`, because nothing proved that a Claude session had
finished its work. A Stop callback can be followed by a Stop-hook
continuation, a queued message looks idle, and Stop's background fields can
be missing.

## Measured first

Before any rule was written, Claude Code 2.1.318 was measured in fictional
sessions:

- UserPromptSubmit and Stop both carry `prompt_id`.
- A message queued while a turn runs fires its submit under the running turn's
  id. Its own turn then ends with a Stop under a new id that no submit named.
- Esc fires no hook. The next prompt's new id is the only sign that the
  interrupted turn ended.
- A Stop-hook continuation sends a second Stop with the same id. Only the
  screen shows the turn is still going ("running stop hooks").
- Every Stop carries `background_tasks` and `session_crons`.
- After most turns Claude draws a dim suggested prompt in its composer. On
  screen it reads exactly like typed text.

Each case is detectable from hooks, input writes and the screen together, as
set out below. A case that cannot be told apart makes the session refuse.

## Decision

1. **The engine judges every session from its own evidence.** A session rests
   only when every one of these holds. Any one failing refuses the whole set
   before anything stops:
   - It is a Claude session with its lifecycle hooks in place. Any other
     provider refuses (`NotClaude`). A session with no hook selection refuses
     as before (`ProviderEvidenceUnavailable`).
   - Its last hook phase is neither Working (`TurnInProgress`) nor Failed
     (`TurnFailed`), and it is not waiting on a question or a permission
     (`WaitingOnOperator`).
   - Every turn is accounted for by prompt id, with none open and none queued.
     A submit or Stop that the count cannot match makes the count unknown
     until the session restarts (`TurnCountUnknown`). A repeated Stop for a
     turn already closed is a Stop-hook continuation and changes nothing.
   - Nothing is typed and unsent. Every byte written since the last submit was
     taken by a submit, and the last write ended with Enter (`UnsentInput`).
     Terminal focus reports are not input. This is judged from what was
     written to the terminal, never from the screen, because a suggested
     prompt and typed text look the same there.
   - The last Stop reported no background task running (`BackgroundWork`) and
     no scheduled job (`ScheduledWork`). A missing field is unknown, and
     refuses.
   - The screen reads Resting (`ScreenNotResting`). This is what catches a
     Stop-hook continuation, which repeats the id.
   - No operator takeover lease and no interactive owner
     (`InteractiveOwner`).

   **A run in progress counts as not resting.** A running turn, a queued
   message, a Stop-hook continuation, a background shell or agent, and a
   scheduled job each refuse, however quiet the screen.

2. **Queen's own session can be stopped by this.** Queen is a Claude worker
   like any other. When she rests at the same moment as every worker, she is
   stopped with them, recorded as owed a return, and brought back like them.
   Exempting her would mean the update never applies while she runs, which is
   always.

3. **The API drives it, as the card does, from the worker supervisor.** It
   waits for the engine's own busy count to read zero and asks at most once a
   minute. It never waits for the worker lifecycle. It then:
   1. lists the running sessions;
   2. drains the engine;
   3. records a return for every running session's worker;
   4. asks the engine to admit that exact set.

   If the whole set stops, the rest is the card's path. Each stopped session's
   context is saved, the package layer is asked for the swap, and the workers
   come back once the new engine reports in.

4. **Nothing is tried while any of these holds, and the card says which:**
   - the switch is off;
   - nothing on this installation replaces the engine;
   - a protocol migration is prepared;
   - the engine is older than protocol 18;
   - the engine is already draining;
   - the engine's busy count is not zero or cannot be read;
   - an earlier attempt at this same update stopped workers and did not
     finish.

   That last one covers the operator's attempts too: whatever stopped them may
   stop this one, and the workers it stops stay down until a swap works.

5. **A refusal stops nothing, owes nothing and leaves no history row.** The
   drain is cancelled. The returns this attempt recorded are withdrawn, and an
   older promise is kept. The card names the reason and the worker. A running
   session that no worker owns refuses once the drain has begun, before the
   engine is asked to stop anything, because nothing could bring it back.

6. **A partial stop is kept exactly and never repeated.** If the engine stopped
   some sessions and then could not stop one:
   - the stopped workers keep their returns;
   - the others owe nothing and keep running;
   - the history records a failed automatic attempt that names both sides;
   - this update is not tried again by itself.

   Returns wait while the engine is behind, so the stopped workers come back
   once the operator updates it. Needs you names them.

7. **A lost answer is read back, not guessed.** An engine error comes before
   any stop. A broken connection or an unreadable reply may come after some
   stops. The engine lists sessions under the registry lock that it holds for
   the whole admission, so a list taken after a lost answer shows what the
   admission left running:
   - If nothing was stopped, it is handled as a refusal.
   - If everything was stopped, the update continues.
   - If only some were stopped, it is handled as a partial stop.

   If even the list fails, every return this attempt recorded stays owed. A
   stopped worker without one would be lost. A running worker holding one is
   harmless: returns are made only once the engine is replaced, and replacing
   it stops every running worker first.

8. **It says so when it happens.**
   - The history line names the workers stopped, the old and new engine, and
     whether each worker is back. For example: "Swarm updated the worker
     engine by itself … while every worker was resting. It stopped Ledger and
     Pantry and brought them back."
   - Needs you shows a waiting update when an automatic try stopped partway,
     naming who is not back, or once it has waited 24 hours.
   - Nothing is forced. The card stays the manual route.

9. **The kill switch is an operator setting, on by default.** The worker engine
   card has a checkbox, "Update by itself when every worker is resting".
   Changing it needs the operator's credential. It takes effect on the next
   supervisor pass, with no deploy or restart. Off, a waiting update waits for
   the card's button as before, and the card says so.

## Protocol 18 and its ordering

The admission request is new, so the terminal-host protocol moves from 17 to 18.

- An engine on 17 cannot be asked, and its updates wait for the card
  (`engine_cannot_check_workers`).
- The first engine on 18 arrives only through a protocol migration. That swaps
  the API and the engine together and stops every worker, so the operator
  applies it.
- From then on, a same-protocol engine update applies by itself.

The Claude hook helper accepts hosts on 17 and 18. Its requests are unchanged,
so an engine still on 17 keeps receiving turn reports after a reload has
installed 18.

## Schema

Schema 221 adds two tables:

- `worker_engine_auto_update` holds the switch and since when the current
  update has waited, so an API restart does not reset the wait.
- `worker_engine_update_workers` holds who each update stopped. It is kept only
  while its attempt row is.

No row means the switch is on.

## Verification

- The engine tests start a fictional Claude session and feed it the measured
  hook sequences. They cover:
  - a resting session stopped with its exact return;
  - a suggested prompt on screen not counting as unsent input;
  - unsent operator input and a takeover in progress refusing;
  - running, queued and interrupted turns refusing until they end;
  - a Stop-hook continuation refusing on the screen;
  - questions, failures, background work and scheduled jobs refusing;
  - a session whose hooks never ran refusing;
  - one busy session refusing the whole set.
- A turn-ledger unit test covers each measured case.
- The API tests run against a fake engine over a real socket. They cover:
  - the whole update, naming who was stopped;
  - a refusal owing nothing;
  - a partial stop kept and never repeated;
  - a lost answer read back in all four of its forms;
  - a session no worker owns;
  - every refusal that comes before the engine is asked, including the
    operator's own failed attempt.
- The existing manual-path tests also guard two regressions:
  - the supervisor running straight after a failed card attempt;
  - an installation with no request path.

## Not proven

- No real Claude worker has been through an automatic update. The engine tests
  feed recorded hook observations, and they do not run Claude.
- Live acceptance needs this Hive's protocol migration to 18 and then a
  same-protocol engine update to wait for. The operator applies both.
- A Claude release that changes these hook fields would make sessions refuse
  as unaccounted. It would not make them stop. That holds only while the
  refusals keep failing closed, which is what the tests pin.
