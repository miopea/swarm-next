# 0140 — A held-back delivery goes when its cooldown ends

- Status: Accepted
- Date: 2026-11-01
- Decided by: the operator, who asked for the server follow-ups to ADR 0137 on
  2026-11-01, and Queen's review of ADR 0138. She asked for proof that "a
  message or an outcome that used to be delivered inline must not now wait for
  a sweep", and to say so if the watch showed a delay.
- Amends: ADR 0138, whose consequences said a delivery a read used to set off
  "now waits for the next mutating call, operator action or supervisor pass".
  Does not change the cooldown, what is delivered, or to whom.

## Context

Coordination does not write to the same terminal twice within five minutes.
A delivery that arrives during that cooldown is held back, and the next pass
after the cooldown ends delivers it, along with anything else that has
accumulated.

Before ADR 0138, any agent call ran a pass, reads included, so a held-back
delivery usually went within seconds of its cooldown ending. ADR 0138 stopped
reads from running a pass. Since then the next pass after a cooldown is the
next call that changes something, an operator action, or the supervisor's
thirty-second tick.

Measured on the Keeper after the reload onto ADR 0138:
- A worker's message to Queen was created at 03:29:33 by a mutating call. Its
  first delivery attempt came a second later, as ADR 0138 intends.
- Queen's terminal was cooling down until 03:33:42, so the attempt was held
  back.
- It was delivered at 03:34:12, at the supervisor's tick, 30 seconds after the
  cooldown ended. Another worker's message held by the same cooldown arrived
  in the same pass.

The thirty-second bound is the same as before ADR 0138. What changed is how
often something happened to come sooner.

## Decision

1. **The supervisor also wakes when the next cooldown ends.**
   - Every coordination pass reads the earliest end of a cooldown running on a
     live session, and leaves it in one deadline the supervisor's loop waits
     on, beside its tick and its wakeup.
   - When the deadline arrives, the supervisor runs its pass, which delivers
     what the cooldown held back.
2. **The deadline is durable state, not a timer.**
   - The end of a cooldown is a stored timestamp plus the fixed window, so the
     wake comes exactly when delivery becomes allowed.
   - Reaching the deadline spends it, so it fires once. The pass it starts
     sets the next one, so it is never more than one pass out of date.
   - It holds one value, not a queue.
3. **No new background task.** The deadline is one more thing the existing
   owned supervisor waits on, and shutdown joins that supervisor as before.

## Consequences

- A delivery held back by a cooldown goes when the cooldown ends, rather than
  at the next call or tick. That is sooner than before ADR 0138 too, when it
  waited for whatever call came next.
- A cooldown ending with nothing held back still costs one supervisor pass.
  Cooldowns start only on an acknowledged write, so this adds at most one pass
  per delivery, five minutes after it.
- A recipient that is busy, rather than cooling down, is still noticed at the
  next pass. The end of a turn is not a stored timestamp, so there is no
  deadline to wake at.
- A pass the deadline starts can stop early, because the terminal host or the
  store failed. It then spends the deadline without setting the next one, and
  the wait falls back to the tick until a pass completes. A pass that stops
  early delivers nothing either way.

Tests:
- The store returns the earliest running cooldown end on a live session. It
  ignores ended sessions and cooldowns that have finished.
- The deadline:
  - wakes once when it arrives, and not again until it is set again;
  - waits while nothing is set;
  - honours an earlier deadline set while it waits;
  - does not wake once cleared.
- A pass leaves the supervisor the end of the cooldown it finds.
- The supervisor's loop runs a pass when the deadline arrives, long before its
  tick and with no wakeup, and only once.

## Rejected

- **A shorter supervisor tick.** It would cost every pass more often to
  shorten a wait that has an exact end.
- **Restoring a pass after every read.** That is the cost ADR 0138 removed.
- **Waking only for cooldowns that are holding something back.** Every
  delivery kind would have to be listed in one query, and a kind added later
  and missed would fall back to the tick without anyone noticing. One extra
  pass per delivery is cheaper than that risk.

## Not proven

- Not measured on the running Keeper yet: that needs a merge and a reload.
- A busy recipient is still noticed only at the next pass, as described above.
