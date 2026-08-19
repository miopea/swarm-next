# Operator-raised backlog

Status: **Running capture — 2026-08-19**

Items the operator raised during live dogfooding, written down as they arrived
rather than held in a conversation. Everything here came from using the product,
not from planning it.

The Swarm Next worker cannot create durable Swarm tasks: `swarm_create_task` is
Queen-only through MCP, and a worker holds list, transition, comment, and
decision tools. Queen should file anything here that deserves queue tracking.
This file is the durable record until then.

## Open

### 4. A task needs one operator instruction line — *fixed*

The operator frequently wants to say something that governs how a task is
approached rather than what it contains: "interview me first", "analyse this,
do not act on it". Today that has to go into the description, where it reads as
part of the work.

Wanted: a single overarching operator comment on a task, distinct from the
description and from activity notes.

Fixed across `7c61be0` and `74f88bc`: a task carries one line, bounded to 280
bytes and a single line, and the briefing states it before the work. Creating a
task cannot carry one yet — that path goes through a constructor shared with
the Jira and email intakes.

### 10. A worker-engine update loses the roster it promised to restore — *fixed*

Raised as: coming back from a worker engine update, the previously active
workers did not return and had to be woken one at a time. An error was visible
at the same time (`Runtime request returned 524`).

Root cause: the list of workers to revive exists only as a local variable inside
the maintenance request handler. The handler stops every worker *first*, then
waits up to 45s for the engine to report the new release, and only revives on
the success path. Any interruption — our own timeout, a proxy timeout, or the
API process restarting — discards the list permanently, and the workers are
already stopped. What still runs on the failure path is `supervise_workers`,
which starts *autostart* workers only, which is why one worker came back and
the rest did not.

The promise on the card is "briefly stops 7 active workers, then revives them".
That promise cannot be kept by a value that dies with the request.

Fixed: the roster is written to the database before anything is stopped, and
the supervisor brings back whatever is still owed, on this pass or a later one.
Recording it is now a precondition — if it cannot be written, no worker is
stopped. Revival waits for the engine to settle, so a worker is never handed
back onto the engine being replaced. Intents age out after fifteen minutes so
they cannot wake workers the operator later chose to leave asleep.

### 11. The update indicator does not use the words the settings page uses — *fixed*

Raised as: there was no indication at the top that a *worker* update was
running; there was one for the app, and it said "engine", which matches nothing
on the settings page. Settings names two things: "Worker engine" and "App and
API". The header must use those names, and must appear for both.

Fixed: the indicator now says "Worker engine update" and "App and API update",
and it reports a worker-engine replacement while it is running — the only
in-progress state it had was the app build, which is why the update that
actually takes workers away ran unannounced. A running engine replacement
outranks everything else in the indicator for the same reason.

### 12. The app updater should look like the worker-engine updater — *fixed*

Raised as: the worker-engine card is the better UI — it states what will happen,
then shows inline progress in place ("Updating worker engine... Stopping active
workers and preserving their conversations"). The app/API reload should work the
same way. This supersedes item 7's open question of where to put progress: the
answer is in the card, where the operator started the action.

Fixed: the App and API card carries the same live progress block as the worker
engine card while a build runs, and separates a build that has been asked for
from one that is under way. Previously a build changed only the wording on the
card, which reads as another resting state.

### 13. Adding an image fails once, then succeeds on retry — *partly fixed*

Raised as: "image could not be added" on the first attempt, worked on the
second. Seen shortly after a worker-engine update, so an API restart mid-upload
is a candidate, but the retry-succeeds shape means this needs reproducing before
it is diagnosed.

Two things found without reproducing it. The upload was the one runtime call
that did not go through the shared transient-failure recovery, so an API
restarting underneath it failed outright while every other call rode it out —
the operator was retrying by hand what the runtime already does for itself.
And the proxy timeout the operator saw the same afternoon, `524`, was not in
the set of statuses treated as transient at all, so nothing retried it either.

The cause of this particular failure is still unproven: the reason was
discarded by an empty `catch`. It is now shown, so the next occurrence
identifies itself.

### 14. Three workers claim the operator at once — *fixed*

Raised as: three workers show "with you" while the operator is plainly in one
session. Observed as BFG Operations `WITH YOU · 4M`, BudgetBug `WITH YOU · 4M`,
and Swarm Next `WITH YOU` together.

"With you" is the one worker state that is exclusive by definition — the
operator is in one place. Three simultaneous claims mean the engagement lease is
not being released when the operator moves on, and nothing enforces that only
the newest holder can claim it. The `4M` on the stale two says they are aged,
not fresh, so age is already known and simply not acted on.

Related to the presence work in `892e439`, but distinct: that fixed *whether the
operator is present*, this is *which worker they are present with*.

Fixed in `f98ee29`: typing into a worker ends that device's engagement
everywhere else.

### 6. Worker engine upgrades need care proportional to their harm

Raised as: this is the most harmful operation and currently has the least
friction around it. Wanted:

- Check whether workers are actually working, and warn before interrupting.
- Come back with the same workers on the same sessions afterwards.

Partly addressed: `d8e77a1` restores the workers an upgrade unloads, which it
previously never did. Not addressed: nothing checks whether a worker was
mid-turn before pulling the floor out, and the warning does not distinguish
loaded from busy.

### 7. App upgrade progress is close to invisible

The control-room indicator added in `64e2f13` shows a spinner and the revision
being built, but the operator did not find it, which means it is not doing its
job where they actually look. Item 12 records the answer the operator gave:
put the progress in the card, not only in the header.

### 8. Takeover is visibility only

Engagement now names the device driving a worker (`a39a95c`), but there is no
control to take it back. Claiming engagement without sending input would be a
new input-authority path and needs an ADR, not a button.

### 9. A phone cannot see what a worker is carrying

Recorded in `docs/29`. The worker context bar is desktop-only, so a phone shows
which worker is selected and nothing about its work. Reinstating the whole bar
would return the vertical chrome the phone layout reclaimed, so this needs a
deliberate choice rather than a default.

## Landed

- The unconfirmed-delivery mark now explains itself where the operator lands.
  Opening the worker states what it means and that the briefing is in the
  terminal below. Nothing is retried automatically: a briefing delivered twice
  is worse than one the operator was told about, so the next step is to read the
  terminal rather than press a button.
- The Inbox chooser can be refreshed without closing and reopening the flow.

- Worker state now reads as a scale: green is work happening, amber is work
  waiting on the operator, red is work that cannot proceed, with neutrals for a
  worker doing nothing wrong and the accent for the worker the operator holds.
  Only the live state moves, and sleeping is separated from resting by fill
  rather than hue so the distinction does not depend on colour.
- The terminal header no longer names a second task. The eyebrow was resolving
  work by session while the context chip resolved it by worker, so one bar
  carried two disagreeing answers.

- Presence no longer flips to Away when a phone changes apps, and a locked
  desktop that stops reporting stops being described as locked.
- Work waiting on an operator answer is no longer reported as stale (`f224968`).
- Touch scrolling follows the finger (`8674023`).
- Workers return after a worker-engine upgrade (`d8e77a1`).
