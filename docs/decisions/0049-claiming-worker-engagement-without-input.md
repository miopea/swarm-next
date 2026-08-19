# ADR 0049: Claiming worker engagement without sending input

## Status

Proposed. Needs an operator ruling before any of it is built.

Raised by `docs/29`, which records takeover as visibility only, and by
`docs/31` item 8. Extends [ADR 0045](0045-engaged-device-terminal-geometry.md)
and the engagement model it depends on.

## Context

Engagement marks that the operator is present at a worker. It does two things.
It suppresses the coordination Swarm would otherwise write into that worker's
terminal, on the reasoning that the operator is already there and a briefing
arriving under their cursor is worse than one that waits. And it pins terminal
geometry to the device that is actually typing, so passive viewers on other
screen sizes cannot fight over the PTY width.

It is claimed by one act only: sending operator input with `engaged: true`.
There is no other way to acquire it. The lease is five minutes, held per worker,
and a device that types takes it from whichever device held it before.

Two consequences follow, and both were observed rather than predicted.

A phone showing "On another desktop" tells the operator that a device they may
have walked away from owns the worker, and offers nothing to do about it. The
only remedy is to type into the worker, which sends real input to a real
provider process. Reclaiming a screen and instructing an agent are not the same
act, and today they are the same button.

Until `f98ee29` a device also accumulated engagements: every worker typed into
within the lease window kept claiming the operator, so several workers reported
"with you" at once and each stale claim held back the coordination that worker
was owed. That is fixed — engaging a worker now ends that device's engagement
everywhere else — but it fixed the accumulation, not the absence of a way to
let go or take over deliberately.

The framing that matters: engagement identifies a **device**, but every device
here belongs to **one operator**. A claim from a second device is not two people
contending for a resource. It is one person moving between screens. Designs
borrowed from multi-user locking would answer a question nobody is asking.

## Decision

Add an explicit claim, and grant it immediately.

1. **A device may claim engagement on a worker without sending input.** The
   claim is granted, not negotiated. There is one operator, so a second device
   asking for the worker is that operator saying where they now are. Refusing,
   queueing, or prompting the other device would model a conflict that does not
   exist.

2. **A claim made without input takes a shorter lease than one earned by
   typing**, and viewing does not renew it. Engagement holds back a worker's
   coordination, so a claim that costs nothing to make must not be able to
   silence a worker for as long as demonstrated presence does. Typing converts
   it to a full lease through the existing path.

3. **A claim does not move terminal geometry.** ADR 0045 gives resize authority
   to the device that most recently sent input, precisely so passive viewers
   cannot resize a PTY under an active one. A claim is not input, so it does not
   take geometry; the first keystroke does, through the rule that already
   exists. A phone claiming a worker therefore reads it at the desktop's width
   until the operator actually types, which is correct: nothing has changed
   about who is driving.

4. **A device may release its own engagement explicitly.** Release is already
   owner-checked in persistence and is currently reachable only by ending the
   session. Exposing it lets the operator say "I am done here" instead of
   waiting out a lease, which is what makes a worker available to Swarm again
   promptly.

## Consequences

Coordination delivery becomes sensitive to a new, cheap action. A claim
suppresses briefings for that worker, so the shorter lease in decision 2 is
load-bearing rather than cosmetic; if it is dropped, an idle claim becomes a way
to stall a worker silently. Any implementation must show the two lease lengths
are actually different, and that a claim left alone expires on the shorter one.

The roster gains a state that is true without being earned: a worker can read
"with you" when the operator has claimed it and typed nothing. That is honest —
the operator did say they were there — but it means "with you" no longer implies
recent input, and anything reasoning about worker activity from engagement must
be checked against that.

Takeover becomes possible from a phone, which is the case that motivated this.
It also becomes possible *by accident* from a phone, so the control needs to be
deliberate rather than a tap target next to something common.

This does not address a worker being driven by a Swarm automation rather than a
device. Queen's deliveries are gated by engagement but do not hold it, and
nothing here changes that.

## Alternatives considered

**Leave it as it is.** Typing already takes the worker, so the capability exists
and only the ceremony is missing. Rejected because the ceremony is the point:
the operator's report was that reclaiming a screen currently requires sending an
instruction, and those are different acts with different consequences for the
provider's conversation.

**Ask the holding device to confirm.** Models the operator as two parties
negotiating. The holding device is frequently the one they have walked away
from, which is exactly when a prompt is worst — it would make the unreachable
device the arbiter of whether the reachable one can work.

**Let a claim take geometry too.** Simpler to explain, and wrong: it reintroduces
the resize fight ADR 0045 exists to prevent, and lets a phone reflow a desktop's
terminal without a keystroke.
