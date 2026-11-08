# 0166 — A wait on Queen is hers

- Status: Accepted
- Date: 2026-11-08
- Asked for by: the operator, 2026-11-08, after finding work stuck that
  nothing had asked them about: "things should be sitting in review, or on
  needs you if things are waiting on me." Earlier: "why is nothing asking me
  to approve it? ... The queen isn't showing anything pending either." Queen
  confirmed her side of it when she routed the fix.
- Amends: ADR 0119. A wait may now name Queen, and a wait naming her is hers.
  ADR 0119's rule that a typed wait is never Queen's to judge still holds for
  every other kind.

## Context

ADR 0119 read any `Waiting on:` other than the operator as an outside party.
An outside wait is nobody's move until its Recheck: Swarm leaves it alone, and
then sends the task back to its worker to check. Nothing treated Queen, or any
other worker on the board, differently.

Measured read-only on the operator's Hive on 2026-11-08:

- From 2026-10-22, workers named Queen as an outside party 180 times, on 95
  tasks. The waits that ended lasted 2.3 hours on average and 27.5 hours at
  most.
- 31 of them ended because the Recheck sent the task back to its worker, with
  Queen's answer still not given. The worker found nothing changed and
  blocked it again. That is the loop.
- When the operator asked, 13 open tasks of one repository's worker were parked
  on Queen this way, with at least 12 more blocked behind them. None was in
  Queen's queue, because a typed wait is never hers. None was in the
  operator's Needs you either, although nine of the thirteen needed "the
  operator's combined release". The worker waited on Queen, Queen waited on
  the operator, and nobody asked the operator.
- 32 of the 180 Needs lines named the operator's move in the possessive. So
  did 12 more under other outside parties. Ten waits named another worker on
  the board.

## Decision

1. **`Waiting on: Queen` is a wait on Queen.** Her name alone, in any case,
   is a new kind, `queen`. It has no party and no Recheck. Its next move is
   hers whatever else holds the task, an unfinished prerequisite included.
   So it sits in her queue and the Queen group of Queues, and it wakes her
   conductor. A pending decision on the task still makes the operator's
   answer the next move.

   The brief offered two fixes: refuse the wait and send the work to Review,
   or map it to Queen. **This maps it**, for three reasons:
   - The waits were on unfinished work: a GO on a head, a ruling, an answer.
     Review is for finished work (ADR 0135). A handoff written to get there
     would claim a DONE WHEN that is not met.
   - Queen's queue already is "next move: Queen". Her board listing, the
     Queen group of Queues and her wake count read that, so they need nothing
     new to learn the kind.
   - The worker's note and its Needs line stay as written.

2. **A wait on Queen has no Recheck.** One is refused, with the form that
   works. The timer was the loop, and her answer is what brings the task
   back.

3. **Her answer clears it.** When Queen messages the task's own worker on that
   task with `swarm_message_worker`, the wait records her answer in the
   transaction that records her message. The task's history says "Queen
   answered", under her profile. The coordination pass then brings the task
   back to Ready within 30 seconds, with a note telling the worker to read her
   message. If Queen moves the task herself, that ends the wait, as any move
   out of Blocked does. A Scout second opinion does not answer for the worker.

4. **Another worker on the board is not a wait.** A party that is a worker
   profile's exact name, ignoring case, is refused. The refusal names the
   link that works, `prerequisite_id`, and the form for Queen. It is checked
   in the application layer, which can read the roster. A party that only
   contains a worker's name, such as "a vendor and Queen", is not caught.

5. **Under an outside party, a Needs line that is the operator's move is
   refused.** That means "the operator's" in the possessive, with either
   apostrophe, in any case. The refusal says to name the operator for an
   action, to ask with `swarm_request_decision` for a decision, to send a
   command as a command request, or to name Queen if she must get it from
   them. A Needs line that only mentions the operator, such as "nothing from
   the operator", is let through. Under Queen the possessive is allowed: the
   task is in her queue, and asking the operator is hers to do. Her guidance
   says so.

6. **Her silence escalates; it does not rest.** A wait on Queen is
   deliberately left out of `typed_wait_holds`. So the coordinator's
   unattended blocked-work check counts it after four hours, and the
   operator's blocked escalations count it after twelve, as for any
   unattended block. This is the brief's guard against a recheck loop. With
   item 2 there is no timer left to loop on, and an unanswered wait reaches
   the operator.

7. **What each side is told.**
   - Every wait refusal carries the format, which now has Queen's form and
     says `<who>` is someone off the board.
   - The `swarm_transition_task` description says the same.
   - Queen's brief and each review's prompt carry a "WAITS ON YOU" section.
     It says the task is hers, that her message on it brings it back, that
     she asks the operator with one decision when the operator's move is what
     it needs, and that an unanswered wait escalates.

8. **The blocks already parked on Queen move into her queue.** Migration 224
   rebuilds `task_waits`, because its CHECKs name the kinds and Queen's answer
   uses `done_at`. It then converts the current wait of each open blocked
   task whose party is Queen alone (trimmed, any case): kind `queen`, party
   and Recheck cleared. The Needs line, the Blocked note and the history are
   left as they were, and each converted task gets one history line saying
   why it moved. Waits on blocks a task has since left are history and stay
   as written.

9. **Shared work names whose Queen.** An Apiary wait on Queen carries the
   sending Hive, as an operator wait does, and other Hives show "<Hive>'s
   Queen". The kind travels as a string, so a Hive on an older release shows
   it by its words.

## Threat model

- **A worker names Queen to park work it could do.** The task wakes Queen and
  sits in her queue, where she sees it. Before this, the same note hid the
  work from everyone.
- **A worker names Queen to dodge the operator.** Item 5 allows the
  operator's move under Queen. She reads the Needs line, and her guidance
  says to ask the operator. If she does not, item 6 escalates it.
- **A message from Queen that is not an answer clears the wait.** The task
  comes back to its worker, who reads the message and blocks again if it must.
  Each step is owned and visible. A clear that nobody owns cannot happen.
- **A worker clears its own wait on Queen.** Only Queen's tool sends a Queen
  message, and the store records the answer only for a message to the task's
  assignee.

## Transition

- **Workers already running** keep the older tool description until they
  restart. The form it taught for Queen, with a Recheck, is refused with the
  new format, so the first block they write on Queen teaches them.
- **Queen already running** gets the new section in her next review's prompt.
- **Migration 224 runs on the next start of the API.** It needs the reload
  that installs this release, and on this Hive that reload waits for the
  operator's word (see Not proven). On the board read for this ADR, 11 open
  tasks would convert. That count moves as those tasks are unblocked by hand.
- **Converted tasks escalate at once if they are old.** A converted task that
  has been blocked for more than twelve hours is in the operator's blocked
  escalations as soon as the migration runs, because it has waited that long
  with nobody's move on it.
- `swarm-domain` changed, so the worker engine build id moves and an engine
  update follows the release.

## Rejected

- **Refuse the wait and send the work to Review.** See item 1.
- **A detector for any outside wait whose Recheck repeats.** The rechecks of
  a real outside party (a vendor, a dependency bot) are legitimate polling,
  and every observed loop was on Queen. Item 2 removes that timer.
- **Read an outside wait whose party is "Queen" as hers, without a
  migration.** Every query that reads the kind would need to know about the
  party, and the Recheck would still send the task back.

## Not proven

- That workers stop writing Queen as an outside party. The refusal teaches
  the form, but only a block written after the release shows it.
- That Queen answers waits on her within the twelve hours before they
  escalate. Her history will show it.
- Live acceptance: the migration converting the tasks on this Hive and their
  read-back in her queue. That needs the release, the reload that runs the
  migration, and the operator's word for both.

## Evidence

### Tests

- Domain:
  - "Waiting on: Queen" in any case is her wait, and her move with or
    without an unfinished prerequisite.
  - Her answer clears it and returns it to the worker.
  - The form with a Recheck is refused.
  - An outside wait needing "the operator's" move is refused, and two live
    Needs lines that only mention the operator are let through.
  - An Apiary wait on Queen names its Hive.
- Store:
  - The form older briefs taught is refused, and the task stays Active.
  - The form that works is in Queen's queue and wakes her, and no amount of
    time clears it.
  - Her message to the worker records her answer under her profile, and the
    next pass brings the task back to Ready with its note.
  - The migration converts an open block parked on Queen with its words
    unchanged, leaves an outside party's wait alone, and the task no longer
    comes back at its old Recheck.
- Service: a wait naming another board worker is refused with the link that
  works, and Queen named alone is let through as hers.
- Tool text and briefs: the transition tool says `<who>` is someone off the
  board and gives Queen's form. Queen's brief and review prompt carry the new
  section.
- Web:
  - The task dialog says "Queen: <needs>", and "Queen: answered" once she
    has answered.
  - A shared task names whose Queen.

- Store, escalation and wake:
  - an unanswered wait on Queen is counted by the four-hour unattended check
    and the operator's twelve-hour blocked escalations;
  - behind an unfinished prerequisite, it still wakes her.

The first two domain tests, the operator test, and the first two store tests
were seen failing before the change. The first store test failed with "a wait
on Queen was left to nobody: Ok(Blocked)": the bug, as the operator met it.

### Ablations

The tests written with the change were checked by breaking each guard in
turn. Each named test failed, and the tree was restored exactly:

- The migration converts nothing: the migration test.
- Queen's message does not answer her wait: the store's queue test.
- A wait on Queen behind a prerequisite does not wake her: the prerequisite
  test.
- A wait on Queen holds like a typed wait, so it never escalates: the store's
  queue test, at the escalation check.
- A board worker's name is accepted as an outside party: the service test.
- The operator's move under an outside party is accepted: the domain test.
- A shared wait on Queen names no Hive: the Apiary test.
- The task dialog drops "Queen:": the dialog test.

The tool-text and brief tests check that literal sentences are present, so
breaking them is the same as editing the sentence. They were not ablated
separately.
