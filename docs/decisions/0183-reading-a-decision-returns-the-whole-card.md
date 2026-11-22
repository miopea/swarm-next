# 0183 — Reading a decision returns the whole card

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), answering "Revised scopes": show the
  whole card in the worker's read, instead of refusing cards whose options
  refer to other text.

## Context

The operator answered a card "Yes, with the same three conditions". A worker
on other work had to act on that ruling. It read the decision by its full id
with `swarm_list_decisions`, which the tool describes as reading one decision
in full. The read returned the title, the summary and the answer. It left out
the card's reason, risk, evidence, suggested action and options. The three
conditions were in the card's reason, so the worker rebuilt them from Queen's
note and two other sources.

The ticket asked Swarm to refuse a card whose option text refers to other
text, such as "the same three conditions". Measured, that would have refused
a correct card: the conditions were on it. The defect was the read.

## Decision

1. **Reading one decision returns its whole card.** Under `card` it carries
   everything the operator saw: urgency, summary, reason, risk, evidence,
   suggested action, the offered actions, the questions with their options
   and descriptions, the deadline, and any requested command or script. That
   holds for pending, answered and withdrawn decisions alike.
2. **The card goes wherever its answer goes.** Anyone who may read the
   answer, by the decision's full id, reads the card too. An answer is a
   choice among what the card offered, and it may name the card's own text,
   so reading one without the other is reading half of it.
3. **Images keep their own rule.** Screenshots the operator attached are
   still shown only to Queen, the worker that asked, and a worker that can
   see the decision's task (ADR 0174).
4. **The reader is told.** A note beside the card says that an answer that
   refers to something means what the card says.

## What this does not do

- It refuses no card. A card whose options refer to its own text is correct.
- It does not change the listing without an id, which stays an index.
- It does not reach text that is only on another task. A card that points
  there is read through a read link (ADR 0182).

## Consequences

- No schema change and no change to any tool's arguments, so the agent tool
  surface does not move.

## Evidence

- An MCP test: a worker with no part in a card reads it by id, and gets the
  answer and the conditions in its reason, with risk, evidence and options.
  An interview card's option descriptions are read while it is still pending.
- Removing the card from the read fails that test.
- On this Hive's store, the card in the ticket holds the conditions in its
  reason: 1,495 bytes, the conditions starting at byte 1,248.

## Not proven

- No Hive has run it.
