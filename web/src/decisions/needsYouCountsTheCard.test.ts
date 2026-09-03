import { expect, test } from "vitest";
import appSource from "../App.tsx?raw";

/**
 * A BADGE THAT DISAGREES WITH THE PAGE teaches the operator to stop believing
 * the badge, which is the one thing it has to do.
 *
 * That is not a hypothetical here: this subject has failed three times, and all
 * three scars are in App.tsx. Held deliveries were "in the queue and in neither
 * count", so Needs you read 0 with a card plainly on the page. Blocked
 * escalations were computed, passed to the inbox, and left out of the badge —
 * which also silenced the push, because the watermark only quiets sources the
 * count knows about. Conversation drift rendered and counted nothing at all.
 *
 * So the guard is not that a card renders, nor that a count is computed — both
 * were true in every one of those failures. It is that THE TWO TOTALS AGREE
 * WITH EACH OTHER, which is the step that was missed each time.
 *
 * ⚠️ THIS USED TO NAME ONE SOURCE. It asserted `unsettledReviewAttentionCount`
 * by hand, which guards that source and no other — so the fourth occurrence
 * could land in any of the others while this stayed green. It now compares the
 * whole sets, and gains a source automatically the moment somebody adds one.
 */
function countsIn(expression: RegExp): Set<string> {
  const matched = appSource.match(expression);
  expect(matched, `${expression} must still be assembled in one expression`).not.toBeNull();
  return new Set(matched?.[0].match(/\w+AttentionCount/g) ?? []);
}

test("the app badge and the inbox tab count the same sources", () => {
  const badge = countsIn(/const attentionCount = [^;]+;/);
  const inboxTab = countsIn(/additionalPendingCount=\{[^}]+\}/);

  expect(badge.size, "the badge must count something").toBeGreaterThan(0);
  // The inbox tab excludes pendingDecisionCount by design — it is the decisions
  // list itself — so it is the *AttentionCount sources that must match.
  expect([...inboxTab].sort()).toEqual([...badge].sort());
});

/**
 * QUEUE ITEMS ARE NOT NEEDS-YOU ITEMS, and the rule was written in the codebase
 * before it was implemented.
 *
 * QueuesView's own docstring: "It also exists to keep this OUT of Needs You.
 * That surface should hold only what the operator alone can act on; a card
 * there reading 'N pieces of finished work are waiting on Queen' is Queen's
 * backlog rendered in the operator's attention area, and it trains them to
 * ignore the screen that matters."
 *
 * That exact card was on Needs You. The operator, sent two screenshots of it:
 * "None of this seems like anything I can do that is actionable" and "some of
 * this is queue items not needs you items."
 *
 * Removed with BOTH of its counts, which is why the test above still passes —
 * the invariant is that the card and the counts move together, not that any
 * particular card exists.
 */
test("Queen's backlog is not rendered in the operator's attention area", () => {
  expect(appSource).not.toContain("<UnsettledReviewCard");
  const badge = countsIn(/const attentionCount = [^;]+;/);
  expect(badge.has("unsettledReviewAttentionCount")).toBe(false);
});

test("blocked age alone is queue evidence, not an operator escalation", () => {
  expect(appSource).not.toContain("<BlockedEscalationCard");
  expect(appSource).toContain("blockedWaits={blockedEscalations}");
  const badge = countsIn(/const attentionCount = [^;]+;/);
  expect(badge.has("blockedEscalationAttentionCount")).toBe(false);
});

/**
 * A self-resolving hold must not appear there either. HeldBriefingList's own
 * text says "Nothing is wrong with these — Swarm is holding them until the
 * worker is free", which is a sentence that should never have been printed
 * under a heading promising things that need the operator.
 */
test("a briefing waiting its turn is not on the operator's attention page", () => {
  expect(appSource).not.toContain("<HeldBriefingList");
});

/**
 * A worker about to resume the wrong conversation IS the operator's, and only
 * theirs — the card says so itself: "Swarm does not switch for you: which
 * thread is the right one is a judgement about your work."
 *
 * It rendered and counted nothing, which is the third instance of this file's
 * subject. The operator called this class critical when Scout hit it: "this is
 * a critical thing as it can regress a state of a worker."
 */
test("conversation drift both renders and counts", () => {
  expect(appSource).toContain("<ConversationDriftCard");
  const badge = countsIn(/const attentionCount = [^;]+;/);
  expect(badge.has("conversationDriftAttentionCount")).toBe(true);
});
