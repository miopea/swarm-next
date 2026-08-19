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

### 2. The unconfirmed-delivery mark is a dead end

The mustard `!` on a worker row explains itself on hover, but clicking through
to that worker offers nothing about what it means or how to clear it. A marker
that names a problem without offering a next action moves the confusion rather
than resolving it. Raised alongside the observation that the glyph alone does
not say what it is.

### 4. A task needs one operator instruction line

The operator frequently wants to say something that governs how a task is
approached rather than what it contains: "interview me first", "analyse this,
do not act on it". Today that has to go into the description, where it reads as
part of the work.

Wanted: a single overarching operator comment on a task, distinct from the
description and from activity notes.

### 5. The Inbox needs a refresh

The email intake list has no way to refresh. Mail arrives while the chooser is
open, so the list is stale the moment it is rendered and the only recovery is to
close and reopen the flow.

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
job where they actually look. Worth reworking rather than defending.

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
