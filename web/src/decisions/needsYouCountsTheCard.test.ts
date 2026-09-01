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

/**
 * THERE ARE TWO TOTALS, AND THE TEST ABOVE ONLY GUARDED ONE.
 *
 * `attentionCount` is the whole-app badge; `additionalPendingCount` is what the
 * inbox's own "Needs you" tab renders beside the word. The first test asserted
 * the first, so it stayed green on 2026-08-31 while the operator was looking at
 * a tab reading "Needs you 0" directly above a card listing fourteen rows —
 * this file's stated failure, reproduced through the counter it did not name.
 *
 * A guard that covers one of two paths is not a guard against the third
 * occurrence of the same bug. Both are asserted now.
 */
test("the inbox tab's own count includes it too, not only the app badge", () => {
  const passed = appSource.match(/additionalPendingCount=\{[^}]+\}/);
  expect(passed, "additionalPendingCount must still be passed in one expression").not.toBeNull();
  expect(passed?.[0]).toContain("unsettledReviewAttentionCount");
});
