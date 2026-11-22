# 0181 — A card waiting on the operator holds the merged-work row

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), answering "Revised scopes": put "no
  operator card" as a gate on the merged-work row, instead of refusing to park
  work that has no card.
- Amends: ADR 0180, which left this open ("It does not hide the row while a
  card waits on the operator. That is a separate decision.").

## Context

Parked work that needs the operator could wait without the operator ever
seeing it. The fear behind the ticket was a production step that nobody filed
a card for, so nothing reached Needs you and the work "just dies".

The ticket first asked Swarm to refuse moving work into Review or Awaiting
release when its next move is the operator and no card is linked. Measured,
that does not work:

- In Awaiting release, `next_move_owner` is always `release`, never the
  operator. Swarm has no stored fact that says a task needs the operator.
- Only Queen moves work into Awaiting release. A refusal would stop Queen,
  not the worker who knows whether a card is owed.
- In the case that prompted the ticket, the card existed, and the operator
  answered it.

ADR 0180's row already names merged work that has waited too long, and says
whether a pending card is linked: Queen's, the operator's, or none. Two of
those three were wrong for what the row is for:

- **The operator's card.** The operator already has it in Needs you. A row
  asks Queen to chase a wait that is on the operator, and ADR 0061 and ADR 0082
  say age alone never makes an operator decision. Swarm's reminder asks the
  releaser to release work they cannot release until the operator answers.
- **No card.** This is the case the ticket feared, and the row said only "No
  pending card is linked to it."

Cards on parked work are common. In the 30 days to 2026-11-22, the operator
answered 158 cards while their task was in Awaiting release, on 119 tasks;
Queen granted 41 more. Half were answered within 15 minutes of being filed.

## Decision

1. **A pending card that waits on the operator holds the row.** A task is
   held when a card linked to it is pending and no pending card linked to it
   is Queen's to grant. While it is held:
   - no row hangs off it, and no reminder goes to its releaser;
   - a row already raised for it clears at once, whether the card was filed
     on the task, linked to it later, or handed on to the operator by Queen;
   - its hidden row does not hold its repository's one row, so other merged
     work there is still raised.

   One SQL rule decides this for the pass that raises the row, for the rule
   that keeps a row live, and for which row holds a repository, so they cannot
   disagree.
2. **Held work is still named.** Another task's row in the same repository
   lists it as merged and waiting on the operator's pending card, and says a
   release there carries it too. A release line carries every merged task in
   it (ADR 0180), so Queen must not release around it unknowingly.
3. **The clock starts again at the operator's answer.** The wait starts at
   the latest of the merge, the park, and the latest answer to a card linked
   to the task during the stay. Only then is the release back with Queen and
   the releaser.
   - Queen's grant is not the operator's answer and does not restart it: while
     Queen's card was pending, the row was raising.
   - A withdrawn card does restart it, from its withdrawal, because it no
     longer holds the release.
4. **Measured waits use the same clock.** Each repository's 90th percentile
   is measured from that same start, so time on the operator's card does not
   lengthen the threshold for everyone else.
5. **No card is said plainly.** The row says nothing about the work is in the
   operator's Needs you, and that if its release needs the operator, it waits
   until a card is filed. Queen decides what to do; the row still never tells
   the operator (ADR 0180).

## What this does not do

- It does not refuse any move. Parking work with no card is allowed, as
  before.
- It does not infer that a task needs the operator. Swarm stores no such
  fact; the row says what is stored.
- It does not tell the operator anything, or age a card into a row. A card
  past its deadline is already raised by its own row.

## Consequences

- No schema change. The rule reads `decision_requests` and the
  `task_decision_membership` view.
- `ReleaseWaitCard` loses its operator value: a held task is never a
  candidate.
- Queen's brief says that work on the operator's card raises no row, so a
  missing row is not read as a missed stall. No tool or argument changes.
- The release-wait clock takes the operator's answer, so `swarm-domain`
  changes and the worker engine build changes with it.

## Evidence

- Persistence tests on a real SQLite store:
  - a pending operator card holds the row and the reminder;
  - a raised row clears when a card is filed, with the task's revision put
    back so only the card can clear it;
  - a hidden row does not keep other merged work in its repository from
    being raised;
  - the wait starts again at the answer and at a withdrawal, and not at
    Queen's grant;
  - held work is named on its repository's row;
  - a measured wait counts from the operator's answer.
- A domain test of the clock.
- Each piece removed in turn fails its test: the hold, the liveness rule, the
  holder rule, the clock, the measured clock, the Queen-grant exclusion, the
  withdrawal, and the naming of held work.

## Not proven

- No Hive has run it. The row itself first runs in 1.23.0.
