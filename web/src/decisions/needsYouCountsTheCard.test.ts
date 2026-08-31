import { expect, test } from "vitest";
import appSource from "../App.tsx?raw";

/**
 * A BADGE THAT DISAGREES WITH THE PAGE teaches the operator to stop believing
 * the badge, which is the one thing it has to do.
 *
 * That is not a hypothetical here: this file's subject has failed twice
 * already, and both scars are in App.tsx. Held deliveries were "in the queue
 * and in neither count", so Needs you read 0 with a card plainly on the page.
 * Blocked escalations were computed, passed to the inbox, and left out of the
 * badge — which also silenced the push, because the watermark only quiets
 * sources the count knows about.
 *
 * So the guard is not that the card renders, nor that the count is computed —
 * both were true in each of those failures. It is that the count reaches the
 * total, which is the step that was missed both times.
 */
test("unsettled review reaches the Needs you badge, not just the page", () => {
  const total = appSource.match(/const attentionCount = [^;]+;/);
  expect(total, "attentionCount must still be assembled in one expression").not.toBeNull();
  expect(total?.[0]).toContain("unsettledReviewAttentionCount");
});

test("and the card is rendered, so the badge is not counting something invisible", () => {
  expect(appSource).toContain("<UnsettledReviewCard");
  expect(appSource).toContain("waiting={unsettledReview}");
});
