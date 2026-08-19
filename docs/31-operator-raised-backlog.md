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

### 1. Worker state colours do not read as a scale

Today: resting is green, buzzing is yellow, and both *awaiting you* and *with
you* are grey. Green–yellow–red reads as go–caution–stop, so the current mapping
puts the calmest state on the strongest colour and the state most needing
attention on the weakest.

The operator's direction: buzzing should be green, and possibly pulsing to show
it is live. Resting is a true idle worker. Awaiting you is a stopped agent that
needs attention and must stand out, though probably not red, because red reads
as error rather than as a request. Blocked is the state that has a claim on red.

Open question the operator asked directly: what other visual channels are
available besides hue — pulse, weight, outline, position — so the scale does not
have to carry every distinction alone. Accessibility matters here: hue alone is
not sufficient signal, and the product targets WCAG 2.1 AA.

### 2. The unconfirmed-delivery mark is a dead end

The mustard `!` on a worker row explains itself on hover, but clicking through
to that worker offers nothing about what it means or how to clear it. A marker
that names a problem without offering a next action moves the confusion rather
than resolving it. Raised alongside the observation that the glyph alone does
not say what it is.

### 3. The terminal header names the worker, and two different tasks

Observed: the header eyebrow carried one task title, the heading carried the
worker name, and the worker context chip carried a *different* task. Two
statements about what the worker is doing, disagreeing, in one bar.

The two come from different owners — one resolves the task by session, the other
by worker — which is the duplicate-owner problem `docs/25` exists to prevent.
The context chip is the newer of the two and introduced the conflict.

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

- Presence no longer flips to Away when a phone changes apps, and a locked
  desktop that stops reporting stops being described as locked.
- Work waiting on an operator answer is no longer reported as stale (`f224968`).
- Touch scrolling follows the finger (`8674023`).
- Workers return after a worker-engine upgrade (`d8e77a1`).
