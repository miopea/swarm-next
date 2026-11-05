# 0159 — Queen's command cards are in her queue, and a card must say

- Status: Accepted
- Date: 2026-11-05
- Decided by: the operator, 2026-11-05, the same evening ADR 0158 went
  live. They approved six command cards in one sitting. Three of them were
  Queen's, and they approved those within two minutes of the cards being
  filed. Asked whether to fold Queen's cards into a collapsed section of
  their inbox, they answered: "Shouldn't the queen's tasks appear in her
  queue list and not my needs you?" Asked whether Swarm should refuse a
  command card that does not say which of their reasons apply, they chose
  "Require it".
- Amends: ADR 0158, decisions 2 and 3.

## Context

ADR 0158 took Queen's cards out of the Needs you count and push
notifications, but left them in the operator's inbox list, marked "With
Queen", with the same Allow button as their own cards. The operator does not
read the cards; that is why ADR 0158 exists. So the list kept the clicks it
was meant to remove: on the first evening, three of Queen's cards were
allowed by the operator 7 seconds after a review reached Queen.

ADR 0158 also let a card that says nothing of the operator's reasons stay
theirs. That kept cards from sessions on the older tool schema working while
workers restarted. Every session is now on the newer schema, and on the first
evening one worker still sent no `operator_only` on any card, so all of its
cards reached the operator whatever they did.

## Decision

1. **Queen's cards are listed in her queue, not the operator's.** A pending
   card that `queen_card!` says is hers is left out of the operator's inbox
   list as well as their count and pushes. The Queues page lists it under
   "Command cards with Queen": the title, the asking worker, how long it has
   waited, and the exact command. It offers no Allow. A card Queen hands on
   joins the operator's Needs you, as ADR 0158 decided.

2. **While the setting is on, a worker's command card must say.** A worker's
   `swarm_request_decision` with `command` and no `operator_only` is refused,
   and the refusal names the four reasons and the `[]` answer. Naming any
   reason keeps the card the operator's. With the setting off, a card may
   still say nothing and is the operator's. Queen's own card is the
   operator's either way and need not say.

The tool surface revision does not move. Sessions on revision 39 can already
send `operator_only`; only its description changed, and a worker that leaves
it out learns the rule from the refusal.

## Consequences

- The operator no longer has a button on Queen's cards. To overrule her,
  they turn the setting off, which makes every waiting card theirs again at
  once (ADR 0158).
- How long a card waits for Queen is her review cadence. Over the seven days
  to 2026-11-05, a review was requested every 632 seconds at the median
  (1110 at p90) and reached her 447 seconds after the request (609 at p90,
  8582 at most).
- A worker that would have said nothing now has to choose. If it chooses `[]`
  wrongly, the card reaches Queen, the residual risk ADR 0158 accepted.

## Evidence

- The service refuses a worker's command card without `operator_only` while
  the setting is on and names the reasons, accepts one with the setting off,
  and accepts Queen's own. A card naming a reason stays the operator's.
- Over the real MCP tools, the same refusal reaches a worker.
- The inbox neither counts nor lists a card with Queen, and lists it with its
  Allow once she hands it on. The Queues page lists it with its command and
  wait, with no Allow, and opens its task.
- Each of these tests was seen failing before the change.

## Not proven

- That Queen clears her cards in practice: none has been granted by her yet.
